#![allow(dead_code)]
// DMNE QUANT // ISOLATED HISTORICAL REPLAY & ABLATION ENGINE
// Evaluates 7 Microstructure Counterfactual Strategies across Historical Trades
// 100% Zero-Lookahead / Zero Target Leakage via PreEntryContext Firewall
// Strictly Walk-Forward Queue: Market outcomes revealed only at exit_epoch <= current.entry_epoch

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::collections::HashMap;
use serde::Deserialize;

use dmne_meme_alpha::config::EngineConfig;
use dmne_meme_alpha::risk::compute_net_realized_sol;
use dmne_meme_alpha::sandbox::{
    PreEntryContext, AntiChurnQuarantineGate, AmmElasticityAndLiquidityGate,
    SybilAndEntropyGate, WalkForwardBayesCohortGate, compute_v071_gross_pnl_pct,
    simulate_v071_exit
};

#[derive(Debug, Clone, Deserialize)]
struct OnChainCacheItem {
    pub token_mint: String,
    pub symbol: String,
    pub entry_epoch: u64,
    pub entry_price_sol: f64,
    pub size_sol: f64,
    pub liquidity_usd: f64,
    pub fdv_usd: f64,
    pub bonding_curve_pct: f64,
    pub consumed_cu: u64,
    pub jito_tip_lamports: u64,
    pub pre_entry_arrival_timestamps: Vec<f64>,
    #[serde(default)]
    pub pre_entry_recent_rejections: usize,
    #[serde(default)]
    pub pre_entry_total_observations: usize,
}

impl From<OnChainCacheItem> for PreEntryContext {
    fn from(item: OnChainCacheItem) -> Self {
        PreEntryContext {
            token_mint: item.token_mint,
            symbol: item.symbol,
            entry_epoch: item.entry_epoch,
            entry_price_sol: item.entry_price_sol,
            size_sol: item.size_sol,
            liquidity_usd: item.liquidity_usd,
            fdv_usd: item.fdv_usd,
            bonding_curve_pct: item.bonding_curve_pct,
            consumed_cu: item.consumed_cu,
            jito_tip_lamports: item.jito_tip_lamports,
            pre_entry_arrival_timestamps: item.pre_entry_arrival_timestamps,
            pre_entry_recent_rejections: item.pre_entry_recent_rejections,
            pre_entry_total_observations: item.pre_entry_total_observations,
        }
    }
}

#[derive(Debug, Clone)]
struct ArchivedTrade {
    pub idx: usize,
    pub exit_time: String,
    pub entry_time: String,
    pub hold_secs: u64,
    pub symbol: String,
    pub mint: String,
    pub pair_address: String,
    pub size_sol: f64,
    pub entry_price_sol: f64,
    pub peak_price_sol: f64,
    pub trough_price_sol: f64,
    pub exit_price_sol: f64,
    pub mfe_pnl_pct: f64,
    pub mae_pnl_pct: f64,
    pub realized_pnl_pct: f64,
    pub realized_pnl_sol: f64,
    pub liquidity_usd: f64,
    pub fdv_usd: f64,
    pub exit_rule: String,
    pub outcome: String,
    pub is_toxic: bool,
}

#[derive(Debug, Default, Clone)]
struct StrategyPerformance {
    pub name: String,
    pub trades_taken: usize,
    pub toxic_blocked: usize,
    pub winners_preserved: usize,
    pub wins: usize,
    pub losses: usize,
    pub gross_sol: f64,
    pub net_sol: f64,
    pub total_gain_sol: f64,
    pub total_loss_sol: f64,
    pub peak_equity_sol: f64,
    pub max_drawdown_sol: f64,
    pub current_equity_sol: f64,
}

impl StrategyPerformance {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn record_trade(&mut self, net_sol: f64, gross_sol: f64, _was_toxic: bool, was_winner: bool) {
        self.trades_taken += 1;
        self.gross_sol += gross_sol;
        self.net_sol += net_sol;

        if was_winner {
            self.winners_preserved += 1;
        }

        if net_sol > 0.0 {
            self.wins += 1;
            self.total_gain_sol += net_sol;
        } else {
            self.losses += 1;
            self.total_loss_sol += net_sol.abs();
        }

        self.current_equity_sol += net_sol;
        if self.current_equity_sol > self.peak_equity_sol {
            self.peak_equity_sol = self.current_equity_sol;
        }
        let dd = self.peak_equity_sol - self.current_equity_sol;
        if dd > self.max_drawdown_sol {
            self.max_drawdown_sol = dd;
        }
    }

    pub fn record_blocked(&mut self, was_toxic: bool) {
        if was_toxic {
            self.toxic_blocked += 1;
        }
    }

    pub fn win_rate_pct(&self) -> f64 {
        if self.trades_taken > 0 {
            (self.wins as f64 / self.trades_taken as f64) * 100.0
        } else {
            0.0
        }
    }

    pub fn profit_factor(&self) -> f64 {
        if self.total_loss_sol > 0.0 {
            self.total_gain_sol / self.total_loss_sol
        } else if self.total_gain_sol > 0.0 {
            99.99
        } else {
            0.0
        }
    }

    pub fn expectancy(&self) -> f64 {
        if self.trades_taken > 0 {
            self.net_sol / self.trades_taken as f64
        } else {
            0.0
        }
    }
}

fn load_archived_trades(csv_path: &str) -> Vec<ArchivedTrade> {
    let file = File::open(csv_path).expect("Failed to open archive CSV");
    let reader = BufReader::new(file);
    let mut trades = Vec::new();

    for (i, line) in reader.lines().enumerate() {
        if i == 0 { continue; } // Skip header
        if let Ok(l) = line {
            let cols: Vec<&str> = l.split(',').collect();
            if cols.len() < 25 { continue; }

            trades.push(ArchivedTrade {
                idx: i - 1,
                exit_time: cols[0].to_string(),
                entry_time: cols[1].to_string(),
                hold_secs: cols[2].parse().unwrap_or(240),
                symbol: cols[3].to_string(),
                mint: cols[4].to_string(),
                pair_address: cols[5].to_string(),
                size_sol: cols[7].parse().unwrap_or(0.02),
                entry_price_sol: cols[8].parse().unwrap_or(0.0),
                peak_price_sol: cols[9].parse().unwrap_or(0.0),
                trough_price_sol: cols[10].parse().unwrap_or(0.0),
                exit_price_sol: cols[11].parse().unwrap_or(0.0),
                mfe_pnl_pct: cols[12].parse().unwrap_or(0.0),
                mae_pnl_pct: cols[13].parse().unwrap_or(0.0),
                realized_pnl_pct: cols[14].parse().unwrap_or(0.0),
                realized_pnl_sol: cols[15].parse().unwrap_or(0.0),
                liquidity_usd: cols[17].parse().unwrap_or(0.0),
                fdv_usd: cols[18].parse().unwrap_or(0.0),
                exit_rule: cols[19].to_string(),
                outcome: cols[20].to_string(),
                is_toxic: cols[24].trim().eq_ignore_ascii_case("true"),
            });
        }
    }
    trades
}

fn load_onchain_cache(cache_path: &str) -> HashMap<String, OnChainCacheItem> {
    let file = File::open(cache_path).expect("Failed to open onchain cache json");
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).expect("Failed to parse onchain cache json")
}

#[derive(Debug, Clone, Default)]
struct TradeCsvRow {
    pub idx: usize,
    pub symbol: String,
    pub mint: String,
    pub historical_pnl_pct: f64,
    pub historical_net_sol: f64,
    pub is_toxic: bool,
    pub strat0_net_sol: f64,
    pub strat1_adm: bool,
    pub strat1_net_sol: f64,
    pub strat2_adm: bool,
    pub strat2_net_sol: f64,
    pub strat3_adm: bool,
    pub strat3_net_sol: f64,
    pub strat4_adm: bool,
    pub strat4_net_sol: f64,
    pub strat5_adm: bool,
    pub strat5_net_sol: f64,
    pub strat6_adm: bool,
    pub strat6_reject_reason: String,
    pub strat6_net_sol: f64,
}

fn main() {
    println!("================================================================================");
    println!(" DMNE QUANT // ADVANCED MICROSTRUCTURE HISTORICAL REPLAY ENGINE (v0.8.0 ABLATION)");
    println!(" Testing 7 Microstructure Counterfactuals (100% Zero Lookahead / Honest Replay)");
    println!("================================================================================");

    let engine_cfg = EngineConfig::load();
    let risk_cfg = engine_cfg.risk;
    let sandbox_cfg = engine_cfg.sandbox;

    println!("[CONFIG] Sybil Entropy Floor: {:.2} bits", sandbox_cfg.min_holder_shannon_entropy);
    println!("[CONFIG] Liquidity Floor: ${:.0} | Max FDV/Liq: {:.1}x | Danger Zone: [{:.1}%, {:.1}%]",
        risk_cfg.min_entry_liquidity_usd, risk_cfg.max_entry_fdv_to_liquidity_ratio,
        sandbox_cfg.graduation_danger_zone_min_pct, sandbox_cfg.graduation_danger_zone_max_pct);
    println!("[CONFIG] Empirical Bayes: alpha_0={:.1}, beta_0={:.1}, min_score={:.2}",
        sandbox_cfg.bayes_prior_alpha, sandbox_cfg.bayes_prior_beta, sandbox_cfg.bayes_min_wallet_score);
    println!("[CONFIG] Quarantine: Permanent Toxic Ban={} | Reentry Cooldown={}s",
        risk_cfg.toxic_exit_permanent_ban, risk_cfg.reentry_cooldown_secs);

    let archive_path = "logs/trades_forensic_v070_archive.csv";
    let cache_path = &sandbox_cfg.cache_path;
    let report_path = &sandbox_cfg.report_path;

    println!("[LOAD] Reading archived historical trades from: {}", archive_path);
    let trades = load_archived_trades(archive_path);
    println!("[LOAD] Loaded {} archived trades.", trades.len());

    println!("[LOAD] Reading on-chain pre-entry initial conditions from cache: {}", cache_path);
    let cache = load_onchain_cache(cache_path);
    println!("[LOAD] Loaded {} un-leaked pre-entry profiles.", cache.len());

    // Initialize 7 Strategies
    let mut strat0 = StrategyPerformance::new("Strategy 0: Historical v0.7.0 Baseline");
    let mut strat1 = StrategyPerformance::new("Strategy 1: Entry Gates - Anti-Churn Quarantine Only");
    let mut strat2 = StrategyPerformance::new("Strategy 2: Entry Gates - AMM Reserve Elasticity & Liq");
    let mut strat3 = StrategyPerformance::new("Strategy 3: Entry Gates - Bayes Cohort + Sybil Entropy");
    let mut strat4 = StrategyPerformance::new("Strategy 4: Exit Upgrade - v0.7.1 Multi-Tranche Ladder");
    let mut strat5 = StrategyPerformance::new("Strategy 5: v0.7.1 Entry Gates + Historical Exits");
    let mut strat6 = StrategyPerformance::new("Strategy 6: Full v0.8.0 Ensemble + Multi-Tranche Ladder");

    // Initialize Walk-Forward Microstructure Gates
    let mut s1_quarantine = AntiChurnQuarantineGate::new();
    let amm_gate = AmmElasticityAndLiquidityGate;
    let sybil_gate = SybilAndEntropyGate;
    let mut s3_bayes = WalkForwardBayesCohortGate::from_config(&sandbox_cfg);
    let mut s5_quarantine = AntiChurnQuarantineGate::new();
    let mut s6_bayes = WalkForwardBayesCohortGate::from_config(&sandbox_cfg);

    // Build timeline of events strictly sorted by entry_epoch
    let mut entry_order: Vec<usize> = (0..trades.len()).collect();
    entry_order.sort_by_key(|&i| {
        let item = cache.get(&i.to_string()).expect("Cache missing entry");
        (item.entry_epoch, i)
    });

    // Market settlement timeline: (exit_epoch, idx, was_winner, is_toxic)
    let mut market_settlements: Vec<(u64, usize, bool, bool)> = trades.iter().enumerate().map(|(i, t)| {
        let item = cache.get(&i.to_string()).expect("Cache missing entry");
        let exit_epoch = item.entry_epoch.saturating_add(t.hold_secs);
        let was_winner = t.outcome == "WIN";
        let is_toxic = t.is_toxic || t.realized_pnl_pct < risk_cfg.stagnation_band_min_pct;
        (exit_epoch, i, was_winner, is_toxic)
    }).collect();
    market_settlements.sort_by_key(|&(exit_epoch, idx, _, _)| (exit_epoch, idx));

    let mut next_settlement_idx = 0;
    let mut csv_rows: Vec<TradeCsvRow> = vec![TradeCsvRow::default(); trades.len()];

    for &idx in &entry_order {
        let t = &trades[idx];
        let cache_item = cache.get(&idx.to_string()).expect("Cache missing entry").clone();
        let ctx: PreEntryContext = cache_item.into();
        let basis_sol = t.size_sol;
        let was_toxic = t.is_toxic;
        let was_winner = t.outcome == "WIN";

        // 1. ADVANCE WALK-FORWARD MARKET STATE: settle any trades where exit_epoch <= current.entry_epoch
        while next_settlement_idx < market_settlements.len() {
            let (settle_epoch, past_idx, past_win, past_toxic) = market_settlements[next_settlement_idx];
            if settle_epoch <= ctx.entry_epoch {
                let past_cache: PreEntryContext = cache.get(&past_idx.to_string()).unwrap().clone().into();
                // Update walk-forward gates with strictly historical settled outcomes
                s1_quarantine.record_settled_trade(&past_cache, settle_epoch, past_toxic);
                s3_bayes.record_settled_trade(&past_cache, settle_epoch, past_win, past_toxic);
                s5_quarantine.record_settled_trade(&past_cache, settle_epoch, past_toxic);
                s6_bayes.record_settled_trade(&past_cache, settle_epoch, past_win, past_toxic);
                next_settlement_idx += 1;
            } else {
                break;
            }
        }

        // --------------------------------------------------------------------
        // STRATEGY 0: Historical v0.7.0 Baseline
        // --------------------------------------------------------------------
        let s0_gross_sol = basis_sol * (t.realized_pnl_pct / 100.0);
        let s0_net_sol = compute_net_realized_sol(basis_sol, t.realized_pnl_pct, &risk_cfg);
        strat0.record_trade(s0_net_sol, s0_gross_sol, was_toxic, was_winner);

        // --------------------------------------------------------------------
        // STRATEGY 1: Entry Gates - Anti-Churn Quarantine Only
        // --------------------------------------------------------------------
        let s1_adm = s1_quarantine.validate_pre_entry(&ctx, &risk_cfg).is_ok();
        let mut s1_net_sol = 0.0;
        if s1_adm {
            let s1_gross_sol = basis_sol * (t.realized_pnl_pct / 100.0);
            s1_net_sol = compute_net_realized_sol(basis_sol, t.realized_pnl_pct, &risk_cfg);
            strat1.record_trade(s1_net_sol, s1_gross_sol, was_toxic, was_winner);
        } else {
            strat1.record_blocked(was_toxic);
        }

        // --------------------------------------------------------------------
        // STRATEGY 2: Entry Gates - AMM Reserve Elasticity & Liquidity Gate
        // --------------------------------------------------------------------
        let s2_adm = amm_gate.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg).is_ok();
        let mut s2_net_sol = 0.0;
        if s2_adm {
            let s2_gross_sol = basis_sol * (t.realized_pnl_pct / 100.0);
            s2_net_sol = compute_net_realized_sol(basis_sol, t.realized_pnl_pct, &risk_cfg);
            strat2.record_trade(s2_net_sol, s2_gross_sol, was_toxic, was_winner);
        } else {
            strat2.record_blocked(was_toxic);
        }

        // --------------------------------------------------------------------
        // STRATEGY 3: Entry Gates - Walk-Forward Bayes Cohort + Sybil Entropy
        // --------------------------------------------------------------------
        let s3_sybil_ok = sybil_gate.validate_pre_entry(&ctx, &sandbox_cfg).is_ok();
        let s3_bayes_ok = s3_bayes.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg).is_ok();
        let s3_adm = s3_sybil_ok && s3_bayes_ok;
        let mut s3_net_sol = 0.0;
        if s3_adm {
            let s3_gross_sol = basis_sol * (t.realized_pnl_pct / 100.0);
            s3_net_sol = compute_net_realized_sol(basis_sol, t.realized_pnl_pct, &risk_cfg);
            strat3.record_trade(s3_net_sol, s3_gross_sol, was_toxic, was_winner);
        } else {
            strat3.record_blocked(was_toxic);
        }

        // --------------------------------------------------------------------
        // STRATEGY 4: Exit Upgrade Only - All 195 Trades via simulate_v071_exit
        // --------------------------------------------------------------------
        let (_s4_net_pnl_pct, s4_net_sol, _s4_rule) = simulate_v071_exit(
            t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs, basis_sol, &risk_cfg
        );
        let (s4_gross_pnl_pct, _) = compute_v071_gross_pnl_pct(
            t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs, &risk_cfg
        );
        let s4_gross_sol = basis_sol * (s4_gross_pnl_pct / 100.0);
        let s4_was_win = s4_net_sol > 0.0;
        strat4.record_trade(s4_net_sol, s4_gross_sol, was_toxic, s4_was_win);

        // --------------------------------------------------------------------
        // STRATEGY 5: v0.7.1 Entry Gates (Anti-Churn + AMM Elasticity) + Historical Exits
        // --------------------------------------------------------------------
        let s5_quarantine_ok = s5_quarantine.validate_pre_entry(&ctx, &risk_cfg).is_ok();
        let s5_amm_ok = amm_gate.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg).is_ok();
        let s5_adm = s5_quarantine_ok && s5_amm_ok;
        let mut s5_net_sol = 0.0;
        if s5_adm {
            let s5_gross_sol = basis_sol * (t.realized_pnl_pct / 100.0);
            s5_net_sol = compute_net_realized_sol(basis_sol, t.realized_pnl_pct, &risk_cfg);
            strat5.record_trade(s5_net_sol, s5_gross_sol, was_toxic, was_winner);
        } else {
            strat5.record_blocked(was_toxic);
        }

        // --------------------------------------------------------------------
        // STRATEGY 6: Full Architecture (Combined Entry Gates + v0.7.1 Exit Ladder)
        // --------------------------------------------------------------------
        let mut s6_reject_reason = String::new();
        if let Err(err) = s6_bayes.quarantine.validate_pre_entry(&ctx, &risk_cfg) {
            s6_reject_reason = err.to_string();
        } else if let Err(err) = amm_gate.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg) {
            s6_reject_reason = err.to_string();
        } else if let Err(err) = sybil_gate.validate_pre_entry(&ctx, &sandbox_cfg) {
            s6_reject_reason = err.to_string();
        } else if let Err(err) = s6_bayes.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg) {
            s6_reject_reason = err.to_string();
        }

        let s6_adm = s6_reject_reason.is_empty();
        let mut s6_net_sol = 0.0;
        if s6_adm {
            let (_s6_net_pnl_pct, net_sol, _s6_rule) = simulate_v071_exit(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs, basis_sol, &risk_cfg
            );
            s6_net_sol = net_sol;
            let (s6_gross_pnl_pct, _) = compute_v071_gross_pnl_pct(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs, &risk_cfg
            );
            let s6_gross_sol = basis_sol * (s6_gross_pnl_pct / 100.0);
            let s6_was_win = s6_net_sol > 0.0;
            strat6.record_trade(s6_net_sol, s6_gross_sol, was_toxic, s6_was_win);
        } else {
            strat6.record_blocked(was_toxic);
        }

        // Save row for sequential CSV writing
        csv_rows[idx] = TradeCsvRow {
            idx,
            symbol: t.symbol.clone(),
            mint: t.mint.clone(),
            historical_pnl_pct: t.realized_pnl_pct,
            historical_net_sol: s0_net_sol,
            is_toxic: was_toxic,
            strat0_net_sol: s0_net_sol,
            strat1_adm: s1_adm,
            strat1_net_sol: s1_net_sol,
            strat2_adm: s2_adm,
            strat2_net_sol: s2_net_sol,
            strat3_adm: s3_adm,
            strat3_net_sol: s3_net_sol,
            strat4_adm: true,
            strat4_net_sol: s4_net_sol,
            strat5_adm: s5_adm,
            strat5_net_sol: s5_net_sol,
            strat6_adm: s6_adm,
            strat6_reject_reason: s6_reject_reason,
            strat6_net_sol: s6_net_sol,
        };
    }

    // Write sequential ablation report CSV
    let mut report_file = File::create(report_path).expect("Failed to create ablation report CSV");
    writeln!(report_file, "idx,symbol,mint,historical_pnl_pct,historical_net_sol,is_toxic,strat0_net_sol,strat1_adm,strat1_net_sol,strat2_adm,strat2_net_sol,strat3_adm,strat3_net_sol,strat4_adm,strat4_net_sol,strat5_adm,strat5_net_sol,strat6_adm,strat6_reject_reason,strat6_net_sol").unwrap();

    for row in &csv_rows {
        writeln!(report_file, "{},{},{},{:.2},{:.6},{},{:.6},{},{:.6},{},{:.6},{},{:.6},{},{:.6},{},{:.6},{},\"{}\",{:.6}",
            row.idx, row.symbol, row.mint, row.historical_pnl_pct, row.historical_net_sol, row.is_toxic,
            row.strat0_net_sol,
            row.strat1_adm, row.strat1_net_sol,
            row.strat2_adm, row.strat2_net_sol,
            row.strat3_adm, row.strat3_net_sol,
            row.strat4_adm, row.strat4_net_sol,
            row.strat5_adm, row.strat5_net_sol,
            row.strat6_adm, row.strat6_reject_reason, row.strat6_net_sol
        ).unwrap();
    }

    // Print Institutional Comparison Table
    let strategies = vec![strat0, strat1, strat2, strat3, strat4, strat5, strat6];

    println!("\n");
    println!("+----------------------------------------------------+--------+--------+--------+--------+-----------+-----------+--------+--------+------------+");
    println!("| Strategy Configuration                             | Trades | Toxic  | Win    | Win    | Gross     | Net       | Profit | Max DD | Expectancy |");
    println!("|                                                    | Taken  | Block  | Keep   | Rate % | PnL (SOL) | PnL (SOL) | Factor | (SOL)  | /Trade SOL |");
    println!("+----------------------------------------------------+--------+--------+--------+--------+-----------+-----------+--------+--------+------------+");

    for s in &strategies {
        println!("| {:<50} | {:>6} | {:>6} | {:>6} | {:>5.1}% | {:>+9.4} | {:>+9.4} | {:>6.2} | {:>6.4} | {:>+10.6} |",
            s.name, s.trades_taken, s.toxic_blocked, s.winners_preserved,
            s.win_rate_pct(), s.gross_sol, s.net_sol, s.profit_factor(),
            s.max_drawdown_sol, s.expectancy()
        );
    }
    println!("+----------------------------------------------------+--------+--------+--------+--------+-----------+-----------+--------+--------+------------+");
    println!("\n[OK] Detailed trade-by-trade ablation report written to: {}", report_path);
}
