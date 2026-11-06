#![allow(dead_code)]
// DMNE QUANT // ISOLATED HISTORICAL REPLAY & ABLATION ENGINE
// Evaluates Microstructure Counterfactual Strategies across Historical Trades
// 100% Zero-Lookahead / Zero Target Leakage via PreEntryContext Firewall
// Dynamic 7.5% Kelly Sizing on Strategy Running Bankroll (Normalized Baseline 0.5556 SOL)
// Self-Contained Walk-Forward States: Each Strategy Learns from its OWN Simulated Outcomes

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

pub const INITIAL_BANKROLL_SOL: f64 = 0.5555555555555556;

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
    pub initial_bankroll_sol: f64,
    pub current_bankroll_sol: f64,
    pub trades_taken: usize,
    pub toxic_blocked: usize,
    pub winners_preserved: usize,
    pub wins: usize,
    pub losses: usize,
    pub gross_sol: f64,
    pub net_sol: f64,
    pub total_gain_sol: f64,
    pub total_loss_sol: f64,
    pub peak_bankroll_sol: f64,
    pub max_drawdown_sol: f64,
}

impl StrategyPerformance {
    pub fn new(name: &str, initial_bankroll: f64) -> Self {
        Self {
            name: name.to_string(),
            initial_bankroll_sol: initial_bankroll,
            current_bankroll_sol: initial_bankroll,
            peak_bankroll_sol: initial_bankroll,
            ..Default::default()
        }
    }

    pub fn slot_size_sol(&self, kelly_pct: f64) -> f64 {
        (self.current_bankroll_sol * (kelly_pct / 100.0)).max(0.010)
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

        self.current_bankroll_sol += net_sol;
        if self.current_bankroll_sol > self.peak_bankroll_sol {
            self.peak_bankroll_sol = self.current_bankroll_sol;
        }
        let dd = self.peak_bankroll_sol - self.current_bankroll_sol;
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
    pub strat5a_adm: bool,
    pub strat5a_net_sol: f64,
    pub strat5b_adm: bool,
    pub strat5b_net_sol: f64,
    pub strat6_adm: bool,
    pub strat6_reject_reason: String,
    pub strat6_net_sol: f64,
}

fn main() {
    println!("================================================================================");
    println!(" DMNE QUANT // ADVANCED MICROSTRUCTURE HISTORICAL REPLAY ENGINE (v0.8.0 ABLATION)");
    println!(" Dynamic 7.5% Kelly Sizing (Normalized 0.5556 SOL) & Self-Contained Walk-Forward");
    println!("================================================================================");

    let engine_cfg = EngineConfig::load();
    let risk_cfg = engine_cfg.risk;
    let sandbox_cfg = engine_cfg.sandbox;

    println!("[CONFIG] Initial Bankroll: {:.4} SOL | Kelly Sizing: {:.1}% NAV",
        INITIAL_BANKROLL_SOL, risk_cfg.kelly_fraction_nav_pct);
    println!("[CONFIG] Sybil Entropy Floor: {:.2} bits | Liquidity Floor: ${:.0} | Max FDV/Liq: {:.1}x",
        sandbox_cfg.min_holder_shannon_entropy, risk_cfg.min_entry_liquidity_usd, risk_cfg.max_entry_fdv_to_liquidity_ratio);
    println!("[CONFIG] Bayes UCB Gate: alpha_0={:.1}, beta_0={:.1}, N_min=6, min_score={:.2}",
        WalkForwardBayesCohortGate::DEFAULT_PRIOR_ALPHA, WalkForwardBayesCohortGate::DEFAULT_PRIOR_BETA,
        sandbox_cfg.bayes_min_wallet_score);
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

    // Initialize 8 Evaluation Strategies
    let mut strat0 = StrategyPerformance::new("Strategy 0: Historical v0.7.0 Baseline", INITIAL_BANKROLL_SOL);
    let mut strat1 = StrategyPerformance::new("Strategy 1: Entry - Anti-Churn Quarantine Only", INITIAL_BANKROLL_SOL);
    let mut strat2 = StrategyPerformance::new("Strategy 2: Entry - AMM Elasticity & Liquidity", INITIAL_BANKROLL_SOL);
    let mut strat3 = StrategyPerformance::new("Strategy 3: Entry - Bayes UCB Cohort + Sybil", INITIAL_BANKROLL_SOL);
    let mut strat4 = StrategyPerformance::new("Strategy 4: Exit Upgrade - v0.7.1 Exit Engine Only", INITIAL_BANKROLL_SOL);
    let mut strat5a = StrategyPerformance::new("Strategy 5a: v0.7.1 Entry Gates + Historical Exits", INITIAL_BANKROLL_SOL);
    let mut strat5b = StrategyPerformance::new("Strategy 5b: v0.7.1 Entry Gates + v0.7.1 Exits", INITIAL_BANKROLL_SOL);
    let mut strat6 = StrategyPerformance::new("Strategy 6: Full v0.8.0 Ensemble + v0.7.1 Ladder", INITIAL_BANKROLL_SOL);

    // Initialize Walk-Forward Microstructure Gates
    let mut s1_quarantine = AntiChurnQuarantineGate::new();
    let amm_gate = AmmElasticityAndLiquidityGate;
    let sybil_gate = SybilAndEntropyGate;
    let mut s3_bayes = WalkForwardBayesCohortGate::from_config(&sandbox_cfg);
    let mut s5a_quarantine = AntiChurnQuarantineGate::new();
    let mut s5b_quarantine = AntiChurnQuarantineGate::new();
    let mut s6_bayes = WalkForwardBayesCohortGate::from_config(&sandbox_cfg);

    // Pending settlement queues for each walk-forward strategy
    // Format: (exit_epoch, PreEntryContext, was_win, is_toxic)
    let mut s1_pending: Vec<(u64, PreEntryContext, bool)> = Vec::new();
    let mut s3_pending: Vec<(u64, PreEntryContext, bool, bool)> = Vec::new();
    let mut s5a_pending: Vec<(u64, PreEntryContext, bool)> = Vec::new();
    let mut s5b_pending: Vec<(u64, PreEntryContext, bool)> = Vec::new();
    let mut s6_pending: Vec<(u64, PreEntryContext, bool, bool)> = Vec::new();

    // Sort trade evaluation strictly by entry_epoch
    let mut entry_order: Vec<usize> = (0..trades.len()).collect();
    entry_order.sort_by_key(|&i| {
        let item = cache.get(&i.to_string()).expect("Cache missing entry");
        (item.entry_epoch, i)
    });

    let mut csv_rows: Vec<TradeCsvRow> = vec![TradeCsvRow::default(); trades.len()];

    for &idx in &entry_order {
        let t = &trades[idx];
        let cache_item = cache.get(&idx.to_string()).expect("Cache missing entry").clone();
        let ctx: PreEntryContext = cache_item.into();
        let exit_epoch = ctx.entry_epoch.saturating_add(t.hold_secs);
        let was_toxic_baseline = t.is_toxic;
        let was_winner_baseline = t.outcome == "WIN";

        // ====================================================================
        // DRAIN WALK-FORWARD SETTLEMENTS: Settle trades where exit_epoch <= current.entry_epoch
        // ====================================================================
        // Strategy 1 settlements
        s1_pending.retain(|&(settle_epoch, ref past_ctx, past_toxic)| {
            if settle_epoch <= ctx.entry_epoch {
                s1_quarantine.record_settled_trade(past_ctx, settle_epoch, past_toxic);
                false
            } else {
                true
            }
        });

        // Strategy 3 settlements
        s3_pending.retain(|&(settle_epoch, ref past_ctx, past_win, past_toxic)| {
            if settle_epoch <= ctx.entry_epoch {
                s3_bayes.record_settled_trade(past_ctx, settle_epoch, past_win, past_toxic);
                false
            } else {
                true
            }
        });

        // Strategy 5a settlements
        s5a_pending.retain(|&(settle_epoch, ref past_ctx, past_toxic)| {
            if settle_epoch <= ctx.entry_epoch {
                s5a_quarantine.record_settled_trade(past_ctx, settle_epoch, past_toxic);
                false
            } else {
                true
            }
        });

        // Strategy 5b settlements
        s5b_pending.retain(|&(settle_epoch, ref past_ctx, past_toxic)| {
            if settle_epoch <= ctx.entry_epoch {
                s5b_quarantine.record_settled_trade(past_ctx, settle_epoch, past_toxic);
                false
            } else {
                true
            }
        });

        // Strategy 6 settlements
        s6_pending.retain(|&(settle_epoch, ref past_ctx, past_win, past_toxic)| {
            if settle_epoch <= ctx.entry_epoch {
                s6_bayes.record_settled_trade(past_ctx, settle_epoch, past_win, past_toxic);
                false
            } else {
                true
            }
        });

        // ====================================================================
        // STRATEGY 0: Historical v0.7.0 Baseline (Dynamic Sizing)
        // ====================================================================
        let s0_size = strat0.slot_size_sol(risk_cfg.kelly_fraction_nav_pct);
        let s0_gross_sol = s0_size * (t.realized_pnl_pct / 100.0);
        let s0_net_sol = compute_net_realized_sol(s0_size, t.realized_pnl_pct, &risk_cfg);
        strat0.record_trade(s0_net_sol, s0_gross_sol, was_toxic_baseline, was_winner_baseline);

        // ====================================================================
        // STRATEGY 1: Entry Gates - Anti-Churn Quarantine Only
        // ====================================================================
        let s1_adm = s1_quarantine.validate_pre_entry(&ctx, &risk_cfg).is_ok();
        let mut s1_net_sol = 0.0;
        if s1_adm {
            let s1_size = strat1.slot_size_sol(risk_cfg.kelly_fraction_nav_pct);
            let s1_gross_sol = s1_size * (t.realized_pnl_pct / 100.0);
            s1_net_sol = compute_net_realized_sol(s1_size, t.realized_pnl_pct, &risk_cfg);
            strat1.record_trade(s1_net_sol, s1_gross_sol, was_toxic_baseline, was_winner_baseline);
            let is_toxic = t.is_toxic || t.realized_pnl_pct < risk_cfg.stagnation_band_min_pct;
            s1_pending.push((exit_epoch, ctx.clone(), is_toxic));
        } else {
            strat1.record_blocked(was_toxic_baseline);
        }

        // ====================================================================
        // STRATEGY 2: Entry Gates - AMM Reserve Elasticity & Liquidity Gate
        // ====================================================================
        let s2_adm = amm_gate.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg).is_ok();
        let mut s2_net_sol = 0.0;
        if s2_adm {
            let s2_size = strat2.slot_size_sol(risk_cfg.kelly_fraction_nav_pct);
            let s2_gross_sol = s2_size * (t.realized_pnl_pct / 100.0);
            s2_net_sol = compute_net_realized_sol(s2_size, t.realized_pnl_pct, &risk_cfg);
            strat2.record_trade(s2_net_sol, s2_gross_sol, was_toxic_baseline, was_winner_baseline);
        } else {
            strat2.record_blocked(was_toxic_baseline);
        }

        // ====================================================================
        // STRATEGY 3: Entry Gates - Bayes UCB Cohort + Sybil Entropy
        // ====================================================================
        let s3_sybil_ok = sybil_gate.validate_pre_entry(&ctx, &sandbox_cfg).is_ok();
        let s3_bayes_ok = s3_bayes.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg).is_ok();
        let s3_adm = s3_sybil_ok && s3_bayes_ok;
        let mut s3_net_sol = 0.0;
        if s3_adm {
            let s3_size = strat3.slot_size_sol(risk_cfg.kelly_fraction_nav_pct);
            let s3_gross_sol = s3_size * (t.realized_pnl_pct / 100.0);
            s3_net_sol = compute_net_realized_sol(s3_size, t.realized_pnl_pct, &risk_cfg);
            strat3.record_trade(s3_net_sol, s3_gross_sol, was_toxic_baseline, was_winner_baseline);
            let is_toxic = t.is_toxic || t.realized_pnl_pct < risk_cfg.stagnation_band_min_pct;
            s3_pending.push((exit_epoch, ctx.clone(), was_winner_baseline, is_toxic));
        } else {
            strat3.record_blocked(was_toxic_baseline);
        }

        // ====================================================================
        // STRATEGY 4: Exit Upgrade Only - All 195 Trades via simulate_v071_exit
        // ====================================================================
        let s4_size = strat4.slot_size_sol(risk_cfg.kelly_fraction_nav_pct);
        let (_s4_net_pnl, s4_net_sol, _s4_rule) = simulate_v071_exit(
            t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs, s4_size, &risk_cfg
        );
        let (s4_gross_pnl_pct, _) = compute_v071_gross_pnl_pct(
            t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs, &risk_cfg
        );
        let s4_gross_sol = s4_size * (s4_gross_pnl_pct / 100.0);
        let s4_was_win = s4_net_sol > 0.0;
        strat4.record_trade(s4_net_sol, s4_gross_sol, was_toxic_baseline, s4_was_win);

        // ====================================================================
        // STRATEGY 5a: v0.7.1 Entry Gates (Anti-Churn + AMM) + Historical Exits
        // ====================================================================
        let s5a_quarantine_ok = s5a_quarantine.validate_pre_entry(&ctx, &risk_cfg).is_ok();
        let s5a_amm_ok = amm_gate.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg).is_ok();
        let s5a_adm = s5a_quarantine_ok && s5a_amm_ok;
        let mut s5a_net_sol = 0.0;
        if s5a_adm {
            let s5a_size = strat5a.slot_size_sol(risk_cfg.kelly_fraction_nav_pct);
            let s5a_gross_sol = s5a_size * (t.realized_pnl_pct / 100.0);
            s5a_net_sol = compute_net_realized_sol(s5a_size, t.realized_pnl_pct, &risk_cfg);
            strat5a.record_trade(s5a_net_sol, s5a_gross_sol, was_toxic_baseline, was_winner_baseline);
            let is_toxic = t.is_toxic || t.realized_pnl_pct < risk_cfg.stagnation_band_min_pct;
            s5a_pending.push((exit_epoch, ctx.clone(), is_toxic));
        } else {
            strat5a.record_blocked(was_toxic_baseline);
        }

        // ====================================================================
        // STRATEGY 5b: v0.7.1 Entry Gates (Anti-Churn + AMM) + v0.7.1 Exits
        // ====================================================================
        let s5b_quarantine_ok = s5b_quarantine.validate_pre_entry(&ctx, &risk_cfg).is_ok();
        let s5b_amm_ok = amm_gate.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg).is_ok();
        let s5b_adm = s5b_quarantine_ok && s5b_amm_ok;
        let mut s5b_net_sol = 0.0;
        if s5b_adm {
            let s5b_size = strat5b.slot_size_sol(risk_cfg.kelly_fraction_nav_pct);
            let (s5b_net_pnl, net_sol, _s5b_rule) = simulate_v071_exit(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs, s5b_size, &risk_cfg
            );
            s5b_net_sol = net_sol;
            let (s5b_gross_pnl_pct, _) = compute_v071_gross_pnl_pct(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs, &risk_cfg
            );
            let s5b_gross_sol = s5b_size * (s5b_gross_pnl_pct / 100.0);
            let s5b_was_win = s5b_net_sol > 0.0;
            strat5b.record_trade(s5b_net_sol, s5b_gross_sol, was_toxic_baseline, s5b_was_win);
            let is_toxic = s5b_net_pnl < risk_cfg.stagnation_band_min_pct || s5b_net_pnl <= risk_cfg.stop_loss_pct;
            s5b_pending.push((exit_epoch, ctx.clone(), is_toxic));
        } else {
            strat5b.record_blocked(was_toxic_baseline);
        }

        // ====================================================================
        // STRATEGY 6: Full v0.8.0 Ensemble + Multi-Tranche Exit Ladder
        // ====================================================================
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
            let s6_size = strat6.slot_size_sol(risk_cfg.kelly_fraction_nav_pct);
            let (s6_net_pnl, net_sol, _s6_rule) = simulate_v071_exit(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs, s6_size, &risk_cfg
            );
            s6_net_sol = net_sol;
            let (s6_gross_pnl_pct, _) = compute_v071_gross_pnl_pct(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs, &risk_cfg
            );
            let s6_gross_sol = s6_size * (s6_gross_pnl_pct / 100.0);
            let s6_was_win = s6_net_sol > 0.0;
            strat6.record_trade(s6_net_sol, s6_gross_sol, was_toxic_baseline, s6_was_win);

            let is_toxic = s6_net_pnl < risk_cfg.stagnation_band_min_pct || s6_net_pnl <= risk_cfg.stop_loss_pct;
            s6_pending.push((exit_epoch, ctx.clone(), s6_was_win, is_toxic));
        } else {
            strat6.record_blocked(was_toxic_baseline);
        }

        // Save row for sequential CSV writing
        csv_rows[idx] = TradeCsvRow {
            idx,
            symbol: t.symbol.clone(),
            mint: t.mint.clone(),
            historical_pnl_pct: t.realized_pnl_pct,
            is_toxic: was_toxic_baseline,
            strat0_net_sol: s0_net_sol,
            strat1_adm: s1_adm,
            strat1_net_sol: s1_net_sol,
            strat2_adm: s2_adm,
            strat2_net_sol: s2_net_sol,
            strat3_adm: s3_adm,
            strat3_net_sol: s3_net_sol,
            strat4_adm: true,
            strat4_net_sol: s4_net_sol,
            strat5a_adm: s5a_adm,
            strat5a_net_sol: s5a_net_sol,
            strat5b_adm: s5b_adm,
            strat5b_net_sol: s5b_net_sol,
            strat6_adm: s6_adm,
            strat6_reject_reason: s6_reject_reason,
            strat6_net_sol: s6_net_sol,
        };
    }

    // Write sequential ablation report CSV
    let mut report_file = File::create(report_path).expect("Failed to create ablation report CSV");
    writeln!(report_file, "idx,symbol,mint,historical_pnl_pct,is_toxic,strat0_net_sol,strat1_adm,strat1_net_sol,strat2_adm,strat2_net_sol,strat3_adm,strat3_net_sol,strat4_adm,strat4_net_sol,strat5a_adm,strat5a_net_sol,strat5b_adm,strat5b_net_sol,strat6_adm,strat6_reject_reason,strat6_net_sol").unwrap();

    for row in &csv_rows {
        writeln!(report_file, "{},{},{},{:.2},{},{:.6},{},{:.6},{},{:.6},{},{:.6},{},{:.6},{},{:.6},{},{:.6},{},\"{}\",{:.6}",
            row.idx, row.symbol, row.mint, row.historical_pnl_pct, row.is_toxic,
            row.strat0_net_sol,
            row.strat1_adm, row.strat1_net_sol,
            row.strat2_adm, row.strat2_net_sol,
            row.strat3_adm, row.strat3_net_sol,
            row.strat4_adm, row.strat4_net_sol,
            row.strat5a_adm, row.strat5a_net_sol,
            row.strat5b_adm, row.strat5b_net_sol,
            row.strat6_adm, row.strat6_reject_reason, row.strat6_net_sol
        ).unwrap();
    }

    // Print Institutional Comparison Table
    let strategies = vec![strat0, strat1, strat2, strat3, strat4, strat5a, strat5b, strat6];

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
