#![allow(dead_code)]
// DMNE QUANT // ISOLATED HISTORICAL REPLAY & ABLATION ENGINE
// Evaluates 7 Microstructure Counterfactual Strategies across Historical Trades
// 100% Zero-Lookahead / Zero Target Leakage via PreEntryContext Firewall

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::collections::HashMap;
use serde::Deserialize;

use dmne_meme_alpha::config::EngineConfig;
use dmne_meme_alpha::risk::compute_net_realized_sol;
use dmne_meme_alpha::sandbox::{
    PreEntryContext, HawkesEstimator, SybilBundleAuditor, VpinCalculator,
    EmpiricalBayesMatrix, CurveElasticityModel
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
    pub pre_entry_recent_rejections: usize,
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

fn main() {
    println!("================================================================================");
    println!(" DMNE QUANT // ADVANCED MICROSTRUCTURE HISTORICAL REPLAY ENGINE (v0.8.0 ABLATION)");
    println!(" Testing 5 Microstructure Methods Across Historical Trades (100% Zero Lookahead)");
    println!("================================================================================");

    let engine_cfg = EngineConfig::load();
    let risk_cfg = engine_cfg.risk;
    let sandbox_cfg = engine_cfg.sandbox;

    println!("[CONFIG] Hawkes: mu={:.2}, alpha={:.2}, beta={:.2}, branching=[{:.2}, {:.2}]",
        sandbox_cfg.hawkes_mu_baseline, sandbox_cfg.hawkes_alpha_jump, sandbox_cfg.hawkes_beta_decay,
        sandbox_cfg.hawkes_min_branching_ratio, sandbox_cfg.hawkes_max_branching_ratio);
    println!("[CONFIG] Sybil: max_slot0={:.2}, min_entropy={:.2}, max_funder={:.2}",
        sandbox_cfg.max_slot0_bundle_supply_ratio, sandbox_cfg.min_holder_shannon_entropy, sandbox_cfg.max_shared_funder_cluster_ratio);
    println!("[CONFIG] VPIN: bucket={:.1} SOL, max_entry={:.2}, emergency_exit={:.2}",
        sandbox_cfg.vpin_bucket_size_sol, sandbox_cfg.vpin_max_entry_toxicity, sandbox_cfg.vpin_emergency_exit_toxicity);
    println!("[CONFIG] Bayes: alpha_0={:.1}, beta_0={:.1}, min_score={:.2}",
        sandbox_cfg.bayes_prior_alpha, sandbox_cfg.bayes_prior_beta, sandbox_cfg.bayes_min_wallet_score);
    println!("[CONFIG] Elasticity: target_fdv=${:.0}, danger_zone=[{:.1}%, {:.1}%], max_impact={:.1}%",
        sandbox_cfg.graduation_target_fdv_usd, sandbox_cfg.graduation_danger_zone_min_pct,
        sandbox_cfg.graduation_danger_zone_max_pct, sandbox_cfg.max_amm_price_impact_pct);

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
    let mut strat1 = StrategyPerformance::new("Strategy 1: + Hawkes Cascade & Apex Exit");
    let mut strat2 = StrategyPerformance::new("Strategy 2: + Sybil & Outside Supply Filter");
    let mut strat3 = StrategyPerformance::new("Strategy 3: + VPIN Pre-Entry Toxicity Gate");
    let mut strat4 = StrategyPerformance::new("Strategy 4: + Empirical Bayes Cohort Scoring");
    let mut strat5 = StrategyPerformance::new("Strategy 5: + Bonding Curve & AMM Elasticity");
    let mut strat6 = StrategyPerformance::new("Strategy 6: Full v0.8.0 Ensemble + Tranches");

    // Initialize microstructure models
    let hawkes = HawkesEstimator::from_config(&sandbox_cfg);
    let sybil_auditor = SybilBundleAuditor;
    let vpin_calc = VpinCalculator::from_config(&sandbox_cfg);
    let mut bayes_matrix_strat4 = EmpiricalBayesMatrix::from_config(&sandbox_cfg);
    let mut bayes_matrix_strat6 = EmpiricalBayesMatrix::from_config(&sandbox_cfg);
    let elasticity_model = CurveElasticityModel;

    let mut report_file = File::create(report_path).expect("Failed to create ablation report CSV");
    writeln!(report_file, "idx,symbol,mint,historical_pnl_pct,historical_net_sol,is_toxic,strat0_net_sol,strat1_adm,strat1_net_sol,strat2_adm,strat2_net_sol,strat3_adm,strat3_net_sol,strat4_adm,strat4_net_sol,strat5_adm,strat5_net_sol,strat6_adm,strat6_reject_reason,strat6_net_sol").unwrap();

    for (i, t) in trades.iter().enumerate() {
        let cache_item = cache.get(&i.to_string()).expect("Cache missing entry").clone();
        // Convert to strictly pre-entry context
        let ctx: PreEntryContext = cache_item.into();

        let was_toxic = t.is_toxic;
        let was_winner = t.outcome == "WIN";
        let basis_sol = t.size_sol;
        let exit_epoch = ctx.entry_epoch.saturating_add(t.hold_secs);

        // --------------------------------------------------------------------
        // STRATEGY 0: Actual Historical Baseline
        // --------------------------------------------------------------------
        let s0_gross_sol = basis_sol * (t.realized_pnl_pct / 100.0);
        let s0_net_sol = compute_net_realized_sol(basis_sol, t.realized_pnl_pct, &risk_cfg);
        strat0.record_trade(s0_net_sol, s0_gross_sol, was_toxic, was_winner);

        // --------------------------------------------------------------------
        // STRATEGY 1: Method 1 (Hawkes Cascade Filter & Conservative Apex Exit)
        // --------------------------------------------------------------------
        let s1_cascade_ok = hawkes.validate_pre_entry(&ctx, &sandbox_cfg).is_ok();
        let mut s1_adm = false;
        let mut s1_net_sol = 0.0;
        if s1_cascade_ok {
            s1_adm = true;
            // Conservative Apex Exit: if token reached >= +25% peak gain, trail 8% from peak
            let s1_exit_pnl_pct = if t.mfe_pnl_pct >= 25.0 {
                t.mfe_pnl_pct - 8.0
            } else {
                t.realized_pnl_pct
            };
            let s1_gross_sol = basis_sol * (s1_exit_pnl_pct / 100.0);
            s1_net_sol = compute_net_realized_sol(basis_sol, s1_exit_pnl_pct, &risk_cfg);
            strat1.record_trade(s1_net_sol, s1_gross_sol, was_toxic, was_winner);
        } else {
            strat1.record_blocked(was_toxic);
        }

        // --------------------------------------------------------------------
        // STRATEGY 2: Method 2 (Sybil & Non-LP Supply Concentration Filter)
        // --------------------------------------------------------------------
        let s2_sybil_ok = sybil_auditor.validate_pre_entry(&ctx, &sandbox_cfg).is_ok();
        let mut s2_adm = false;
        let mut s2_net_sol = 0.0;
        if s2_sybil_ok {
            s2_adm = true;
            let s2_gross_sol = basis_sol * (t.realized_pnl_pct / 100.0);
            s2_net_sol = compute_net_realized_sol(basis_sol, t.realized_pnl_pct, &risk_cfg);
            strat2.record_trade(s2_net_sol, s2_gross_sol, was_toxic, was_winner);
        } else {
            strat2.record_blocked(was_toxic);
        }

        // --------------------------------------------------------------------
        // STRATEGY 3: Method 3 (VPIN Pre-Entry Toxicity Gate)
        // --------------------------------------------------------------------
        let s3_vpin_ok = vpin_calc.validate_pre_entry(&ctx, &sandbox_cfg).is_ok();
        let mut s3_adm = false;
        let mut s3_net_sol = 0.0;
        if s3_vpin_ok {
            s3_adm = true;
            let s3_gross_sol = basis_sol * (t.realized_pnl_pct / 100.0);
            s3_net_sol = compute_net_realized_sol(basis_sol, t.realized_pnl_pct, &risk_cfg);
            strat3.record_trade(s3_net_sol, s3_gross_sol, was_toxic, was_winner);
        } else {
            strat3.record_blocked(was_toxic);
        }

        // --------------------------------------------------------------------
        // STRATEGY 4: Method 4 (Empirical Bayes Cohort Scoring & Anti-Reentry)
        // --------------------------------------------------------------------
        let s4_bayes_ok = bayes_matrix_strat4.validate_pre_entry(&ctx, &sandbox_cfg).is_ok();
        let mut s4_adm = false;
        let mut s4_net_sol = 0.0;
        if s4_bayes_ok {
            s4_adm = true;
            let s4_gross_sol = basis_sol * (t.realized_pnl_pct / 100.0);
            s4_net_sol = compute_net_realized_sol(basis_sol, t.realized_pnl_pct, &risk_cfg);
            strat4.record_trade(s4_net_sol, s4_gross_sol, was_toxic, was_winner);
        } else {
            strat4.record_blocked(was_toxic);
        }
        // Walk-forward Bayesian update strictly after decision
        bayes_matrix_strat4.record_settled_trade(&ctx, exit_epoch, was_winner);

        // --------------------------------------------------------------------
        // STRATEGY 5: Method 5 (Bonding-Curve Saturation & AMM Elasticity)
        // --------------------------------------------------------------------
        let s5_elasticity_ok = elasticity_model.validate_pre_entry(&ctx, &sandbox_cfg).is_ok();
        let mut s5_adm = false;
        let mut s5_net_sol = 0.0;
        if s5_elasticity_ok {
            s5_adm = true;
            let s5_gross_sol = basis_sol * (t.realized_pnl_pct / 100.0);
            s5_net_sol = compute_net_realized_sol(basis_sol, t.realized_pnl_pct, &risk_cfg);
            strat5.record_trade(s5_net_sol, s5_gross_sol, was_toxic, was_winner);
        } else {
            strat5.record_blocked(was_toxic);
        }

        // --------------------------------------------------------------------
        // STRATEGY 6: Full v0.8.0 Ensemble (All 5 Gates + Multi-Tranche Ladder)
        // --------------------------------------------------------------------
        let s6_bayes_ok = bayes_matrix_strat6.validate_pre_entry(&ctx, &sandbox_cfg).is_ok();

        let mut s6_reject_reason = "";
        if !s1_cascade_ok {
            s6_reject_reason = "HAWKES_CASCADE_REJECT";
        } else if !s2_sybil_ok {
            s6_reject_reason = "SYBIL_OUTSIDE_SUPPLY_REJECT";
        } else if !s3_vpin_ok {
            s6_reject_reason = "VPIN_TOXICITY_REJECT";
        } else if !s6_bayes_ok {
            s6_reject_reason = "BAYES_COHORT_OR_REENTRY_REJECT";
        } else if !s5_elasticity_ok {
            s6_reject_reason = "BONDING_DANGER_OR_IMPACT_REJECT";
        }

        let mut s6_adm = false;
        let mut s6_net_sol = 0.0;

        if s6_reject_reason.is_empty() {
            s6_adm = true;
            // Execute v0.7.1 Multi-Tranche Convex Exit Ladder
            let s6_exit_pnl_pct = if t.mfe_pnl_pct >= 90.0 {
                // TP1: 40% @ +45%, TP2: 35% @ +90%, Runner: 25% @ (Peak - 18%)
                let runner_pnl = (t.mfe_pnl_pct - 18.0).max(12.0);
                (0.40 * 45.0) + (0.35 * 90.0) + (0.25 * runner_pnl)
            } else if t.mfe_pnl_pct >= 45.0 {
                // TP1: 40% @ +45%, Runner: 60% @ Breakeven Armed (+12% Floor or Trailing 18%)
                let runner_pnl = (t.mfe_pnl_pct - 18.0).max(12.0);
                (0.40 * 45.0) + (0.60 * runner_pnl)
            } else {
                t.realized_pnl_pct
            };

            let s6_gross_sol = basis_sol * (s6_exit_pnl_pct / 100.0);
            s6_net_sol = compute_net_realized_sol(basis_sol, s6_exit_pnl_pct, &risk_cfg);
            strat6.record_trade(s6_net_sol, s6_gross_sol, was_toxic, was_winner);
        } else {
            strat6.record_blocked(was_toxic);
        }
        // Walk-forward Bayesian update strictly after decision
        bayes_matrix_strat6.record_settled_trade(&ctx, exit_epoch, was_winner);

        // Write row to ablation report CSV
        writeln!(report_file, "{},{},{},{:.2},{:.6},{},{:.6},{},{:.6},{},{:.6},{},{:.6},{},{:.6},{},{:.6},{},\"{}\",{:.6}",
            i, t.symbol, t.mint, t.realized_pnl_pct, s0_net_sol, was_toxic,
            s0_net_sol,
            s1_adm, s1_net_sol,
            s2_adm, s2_net_sol,
            s3_adm, s3_net_sol,
            s4_adm, s4_net_sol,
            s5_adm, s5_net_sol,
            s6_adm, s6_reject_reason, s6_net_sol
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
