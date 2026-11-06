#![allow(dead_code)]
// DMNE QUANT // ISOLATED HISTORICAL REPLAY & ABLATION ENGINE
// Definitive Backtest of EXACT Live v0.7.1 Metrics across All Past Trades (Archive + Live Ledger)
// Realistic Fast-Dump Gap Slippage Modeling [-30% SL] & Normalized Fixed vs Kelly Sizing

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::collections::HashMap;
use serde::Deserialize;

use dmne_meme_alpha::config::EngineConfig;
use dmne_meme_alpha::risk::compute_net_realized_sol;
use dmne_meme_alpha::sandbox::{
    PreEntryContext, AntiChurnQuarantineGate, AmmElasticityAndLiquidityGate,
    StopLossExecutionMode, compute_v071_gross_pnl_pct, simulate_v071_exit
};

pub const INITIAL_BANKROLL_SOL: f64 = 0.5555555555555556;
pub const FIXED_SLOT_SIZE_SOL: f64 = 0.041666666666666664; // Exactly 7.5% of 0.5556 SOL (~0.0417 SOL)

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

    pub fn slot_size_kelly(&self, kelly_pct: f64) -> f64 {
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

#[derive(Debug, Clone)]
struct TradeBreakdown {
    pub symbol: String,
    pub mint: String,
    pub entry_price: f64,
    pub peak_price: f64,
    pub mfe_pct: f64,
    pub actual_exit_price: f64,
    pub actual_pnl_pct: f64,
    pub simulated_net_pnl_pct: f64,
    pub net_pnl_sol: f64,
    pub exit_rule: String,
}

fn parse_rfc3339_to_epoch(ts: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.timestamp().max(0) as u64)
        .unwrap_or(0)
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

fn load_live_settled_trades(
    csv_path: &str,
    start_idx: usize,
) -> Vec<(ArchivedTrade, PreEntryContext)> {
    let file = match File::open(csv_path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = BufReader::new(file);
    let mut raw_rows: Vec<ArchivedTrade> = Vec::new();

    for (i, line) in reader.lines().enumerate() {
        if i == 0 { continue; }
        if let Ok(l) = line {
            let cols: Vec<&str> = l.split(',').collect();
            if cols.len() < 25 { continue; }
            raw_rows.push(ArchivedTrade {
                idx: start_idx + i - 1,
                exit_time: cols[0].to_string(),
                entry_time: cols[1].to_string(),
                hold_secs: cols[2].parse().unwrap_or(240),
                symbol: cols[3].to_string(),
                mint: cols[4].to_string(),
                pair_address: cols[5].to_string(),
                size_sol: cols[7].parse().unwrap_or(0.0417),
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

    // Group partial tranches of the same position by (mint, entry_time)
    let mut groups: HashMap<(String, String), Vec<ArchivedTrade>> = HashMap::new();
    for row in raw_rows {
        let key = (row.mint.clone(), row.entry_time.clone());
        groups.entry(key).or_default().push(row);
    }

    let mut consolidated = Vec::new();
    let mut group_keys: Vec<(String, String)> = groups.keys().cloned().collect();
    group_keys.sort_by_key(|(_, entry_time)| parse_rfc3339_to_epoch(entry_time));

    for (cur_idx, key) in group_keys.iter().enumerate() {
        let tranches = &groups[key];
        let first = &tranches[0];
        let total_size_sol: f64 = tranches.iter().map(|t| t.size_sol).sum();
        let total_pnl_sol: f64 = tranches.iter().map(|t| t.realized_pnl_sol).sum();
        let max_hold_secs = tranches.iter().map(|t| t.hold_secs).max().unwrap_or(first.hold_secs);
        let max_peak_price = tranches.iter().map(|t| t.peak_price_sol).fold(0.0_f64, |a, b| a.max(b));
        let max_mfe = tranches.iter().map(|t| t.mfe_pnl_pct).fold(0.0_f64, |a, b| a.max(b));
        let last_exit_price = tranches.last().map(|t| t.exit_price_sol).unwrap_or(first.exit_price_sol);
        let last_exit_time = tranches.last().map(|t| t.exit_time.clone()).unwrap_or_else(|| first.exit_time.clone());
        let any_toxic = tranches.iter().any(|t| t.is_toxic);
        let combined_rules: Vec<&str> = tranches.iter().map(|t| t.exit_rule.as_str()).collect();

        let realized_pnl_pct = if total_size_sol > 0.0 {
            (total_pnl_sol / total_size_sol) * 100.0
        } else {
            first.realized_pnl_pct
        };

        let outcome = if total_pnl_sol > 0.0 { "WIN".to_string() } else { "LOSS".to_string() };

        let trade = ArchivedTrade {
            idx: start_idx + cur_idx,
            exit_time: last_exit_time,
            entry_time: first.entry_time.clone(),
            hold_secs: max_hold_secs,
            symbol: first.symbol.clone(),
            mint: first.mint.clone(),
            pair_address: first.pair_address.clone(),
            size_sol: total_size_sol,
            entry_price_sol: first.entry_price_sol,
            peak_price_sol: max_peak_price,
            trough_price_sol: first.trough_price_sol,
            exit_price_sol: last_exit_price,
            mfe_pnl_pct: max_mfe,
            mae_pnl_pct: first.mae_pnl_pct,
            realized_pnl_pct,
            realized_pnl_sol: total_pnl_sol,
            liquidity_usd: first.liquidity_usd,
            fdv_usd: first.fdv_usd,
            exit_rule: combined_rules.join(" + "),
            outcome,
            is_toxic: any_toxic,
        };

        let entry_epoch = parse_rfc3339_to_epoch(&first.entry_time);
        let ctx = PreEntryContext {
            token_mint: first.mint.clone(),
            symbol: first.symbol.clone(),
            entry_epoch,
            entry_price_sol: first.entry_price_sol,
            size_sol: total_size_sol,
            liquidity_usd: first.liquidity_usd,
            fdv_usd: first.fdv_usd,
            bonding_curve_pct: 0.0,
            consumed_cu: 55774,
            jito_tip_lamports: 10000,
            pre_entry_arrival_timestamps: vec![entry_epoch as f64 - 10.0, entry_epoch as f64 - 5.0],
            pre_entry_recent_rejections: 0,
            pre_entry_total_observations: 10,
        };

        consolidated.push((trade, ctx));
    }

    consolidated
}

fn load_onchain_cache(cache_path: &str) -> HashMap<String, OnChainCacheItem> {
    let file = File::open(cache_path).expect("Failed to open onchain cache json");
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).expect("Failed to parse onchain cache json")
}

fn main() {
    println!("================================================================================");
    println!(" DMNE QUANT // DEFINITIVE BACKTEST OF LIVE v0.7.1 METRICS ACROSS ALL PAST TRADES");
    println!(" Evaluated on Combined Archive (195) + Live Ledger (5) with Realistic Gap SL");
    println!("================================================================================");

    let engine_cfg = EngineConfig::load();
    let risk_cfg = engine_cfg.risk;
    let sandbox_cfg = engine_cfg.sandbox;

    println!("[CONFIG] Live [risk] Envelope from config.toml:");
    println!("  - Liquidity Floor: ${:.0} | Max FDV/Liq: {:.1}x | Conviction Floor: {:.1}%",
        risk_cfg.min_entry_liquidity_usd, risk_cfg.max_entry_fdv_to_liquidity_ratio, risk_cfg.min_entry_conviction_pct);
    println!("  - Anti-Churn: Reentry Cooldown={}s | Toxic Ban Floor={:.1}% (Permanent={})",
        risk_cfg.reentry_cooldown_secs, risk_cfg.stagnation_band_min_pct, risk_cfg.toxic_exit_permanent_ban);
    println!("  - Exit Ladder: BE Armed=+{:.0}% (Floor=+{:.0}%) | TP1=+{:.0}% ({:.0}%) | TP2=+{:.0}% ({:.0}%)",
        risk_cfg.breakeven_arm_pct, risk_cfg.breakeven_floor_pct,
        risk_cfg.tp1_trigger_pct, risk_cfg.tp1_sell_fraction * 100.0,
        risk_cfg.tp2_trigger_pct, risk_cfg.tp2_sell_fraction * 100.0);
    println!("  - Trailing Drop: {:.0}% | Early Abort: {:.1}% at {}s | Hard SL: {:.0}% | Max Adverse Slippage: {:.0}%",
        risk_cfg.runner_trailing_drop_pct, risk_cfg.early_abort_loss_pct, risk_cfg.early_abort_window_secs,
        risk_cfg.stop_loss_pct, risk_cfg.max_slippage_loss_pct);

    let archive_path = "logs/trades_forensic_v070_archive.csv";
    let live_csv_path = "logs/trades_forensic.csv";
    let cache_path = &sandbox_cfg.cache_path;

    println!("\n[LOAD] Reading historical archive: {}", archive_path);
    let mut trades = load_archived_trades(archive_path);
    let archive_count = trades.len();
    println!("[LOAD] Loaded {} archived trades.", archive_count);

    println!("[LOAD] Reading cache for historical pre-entry profiles: {}", cache_path);
    let cache = load_onchain_cache(cache_path);

    let mut all_contexts: Vec<PreEntryContext> = Vec::new();
    for i in 0..archive_count {
        let item = cache.get(&i.to_string()).expect("Cache missing entry").clone();
        all_contexts.push(item.into());
    }

    println!("[LOAD] Reading newly settled live trades from: {}", live_csv_path);
    let live_pairs = load_live_settled_trades(live_csv_path, archive_count);
    let live_count = live_pairs.len();
    println!("[LOAD] Appended {} consolidated live positions.", live_count);

    for (live_trade, live_ctx) in live_pairs {
        trades.push(live_trade);
        all_contexts.push(live_ctx);
    }

    let total_trade_count = trades.len();
    println!("[LOAD] Total combined trade ledger: {} complete trade lifecycles.\n", total_trade_count);

    // Initialize 6 Specific Backtest Strategies
    let mut strat1 = StrategyPerformance::new("1. Actual Historical Ledger (Raw Recorded PnL)", INITIAL_BANKROLL_SOL);
    let mut strat2 = StrategyPerformance::new("2. Live v0.7.1 Entry Gates Only (Fixed 0.0417 SOL, Old Exits)", INITIAL_BANKROLL_SOL);
    let mut strat3 = StrategyPerformance::new("3. Live v0.7.1 Exits Only (Fixed 0.0417 SOL, Gap SL [-30%])", INITIAL_BANKROLL_SOL);
    let mut strat4 = StrategyPerformance::new("4. Live v0.7.1 Full Engine (Fixed 0.0417 SOL, Ideal -20% SL)", INITIAL_BANKROLL_SOL);
    let mut strat5 = StrategyPerformance::new("5. Live v0.7.1 Full Engine (Fixed 0.0417 SOL, Realistic Gap SL)", INITIAL_BANKROLL_SOL);
    let mut strat6 = StrategyPerformance::new("6. Live v0.7.1 Full Engine (Dynamic 7.5% Kelly, Realistic Gap SL)", INITIAL_BANKROLL_SOL);

    // Microstructure gates
    let amm_gate = AmmElasticityAndLiquidityGate;
    let mut s2_quarantine = AntiChurnQuarantineGate::new();
    let mut s4_quarantine = AntiChurnQuarantineGate::new();
    let mut s5_quarantine = AntiChurnQuarantineGate::new();
    let mut s6_quarantine = AntiChurnQuarantineGate::new();

    // Pending queues for walk-forward settlement
    let mut s2_pending: Vec<(u64, PreEntryContext, bool)> = Vec::new();
    let mut s4_pending: Vec<(u64, PreEntryContext, bool)> = Vec::new();
    let mut s5_pending: Vec<(u64, PreEntryContext, bool)> = Vec::new();
    let mut s6_pending: Vec<(u64, PreEntryContext, bool)> = Vec::new();

    let mut strat5_breakdown: Vec<TradeBreakdown> = Vec::new();

    // Process all trades in chronological entry order
    let mut entry_order: Vec<usize> = (0..total_trade_count).collect();
    entry_order.sort_by_key(|&i| (all_contexts[i].entry_epoch, i));

    for &idx in &entry_order {
        let t = &trades[idx];
        let ctx = &all_contexts[idx];
        let exit_epoch = ctx.entry_epoch.saturating_add(t.hold_secs);
        let was_toxic_baseline = t.is_toxic;
        let was_winner_baseline = t.outcome == "WIN";

        // Drain pending settlements
        s2_pending.retain(|&(settle_epoch, ref past_ctx, past_toxic)| {
            if settle_epoch <= ctx.entry_epoch {
                s2_quarantine.record_settled_trade(past_ctx, settle_epoch, past_toxic);
                false
            } else { true }
        });
        s4_pending.retain(|&(settle_epoch, ref past_ctx, past_toxic)| {
            if settle_epoch <= ctx.entry_epoch {
                s4_quarantine.record_settled_trade(past_ctx, settle_epoch, past_toxic);
                false
            } else { true }
        });
        s5_pending.retain(|&(settle_epoch, ref past_ctx, past_toxic)| {
            if settle_epoch <= ctx.entry_epoch {
                s5_quarantine.record_settled_trade(past_ctx, settle_epoch, past_toxic);
                false
            } else { true }
        });
        s6_pending.retain(|&(settle_epoch, ref past_ctx, past_toxic)| {
            if settle_epoch <= ctx.entry_epoch {
                s6_quarantine.record_settled_trade(past_ctx, settle_epoch, past_toxic);
                false
            } else { true }
        });

        // --------------------------------------------------------------------
        // STRATEGY 1: Actual Historical Ledger
        // --------------------------------------------------------------------
        let s1_net_sol = t.realized_pnl_sol;
        let s1_gross_sol = t.size_sol * (t.realized_pnl_pct / 100.0);
        strat1.record_trade(s1_net_sol, s1_gross_sol, was_toxic_baseline, was_winner_baseline);

        // --------------------------------------------------------------------
        // STRATEGY 2: Entry Gates Only + Actual Historical Exits (Fixed 0.0417 SOL)
        // --------------------------------------------------------------------
        let s2_quarantine_ok = s2_quarantine.validate_pre_entry(ctx, &risk_cfg).is_ok();
        let s2_amm_ok = amm_gate.validate_pre_entry(ctx, &risk_cfg, &sandbox_cfg).is_ok();
        let s2_adm = s2_quarantine_ok && s2_amm_ok;
        if s2_adm {
            let s2_gross_sol = FIXED_SLOT_SIZE_SOL * (t.realized_pnl_pct / 100.0);
            let s2_net_sol = compute_net_realized_sol(FIXED_SLOT_SIZE_SOL, t.realized_pnl_pct, &risk_cfg);
            strat2.record_trade(s2_net_sol, s2_gross_sol, was_toxic_baseline, was_winner_baseline);
            let is_toxic = t.is_toxic || t.realized_pnl_pct < risk_cfg.stagnation_band_min_pct;
            s2_pending.push((exit_epoch, ctx.clone(), is_toxic));
        } else {
            strat2.record_blocked(was_toxic_baseline);
        }

        // --------------------------------------------------------------------
        // STRATEGY 3: Exit Engine Only on All Trades (Realistic Gap SL, Fixed 0.0417 SOL)
        // --------------------------------------------------------------------
        let (s3_net_pnl, s3_net_sol, _s3_rule) = simulate_v071_exit(
            t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs,
            FIXED_SLOT_SIZE_SOL, &risk_cfg, StopLossExecutionMode::RealisticGapSlippage
        );
        let (s3_gross_pnl_pct, _) = compute_v071_gross_pnl_pct(
            t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs,
            &risk_cfg, StopLossExecutionMode::RealisticGapSlippage
        );
        let s3_gross_sol = FIXED_SLOT_SIZE_SOL * (s3_gross_pnl_pct / 100.0);
        let s3_was_win = s3_net_pnl > 0.0;
        strat3.record_trade(s3_net_sol, s3_gross_sol, was_toxic_baseline, s3_was_win);

        // --------------------------------------------------------------------
        // STRATEGY 4: Full Engine (Fixed 0.0417 SOL, Ideal -20% SL)
        // --------------------------------------------------------------------
        let s4_quarantine_ok = s4_quarantine.validate_pre_entry(ctx, &risk_cfg).is_ok();
        let s4_amm_ok = amm_gate.validate_pre_entry(ctx, &risk_cfg, &sandbox_cfg).is_ok();
        let s4_adm = s4_quarantine_ok && s4_amm_ok;
        if s4_adm {
            let (s4_net_pnl, s4_net_sol, _s4_rule) = simulate_v071_exit(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs,
                FIXED_SLOT_SIZE_SOL, &risk_cfg, StopLossExecutionMode::Ideal
            );
            let (s4_gross_pnl_pct, _) = compute_v071_gross_pnl_pct(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs,
                &risk_cfg, StopLossExecutionMode::Ideal
            );
            let s4_gross_sol = FIXED_SLOT_SIZE_SOL * (s4_gross_pnl_pct / 100.0);
            let s4_was_win = s4_net_pnl > 0.0;
            strat4.record_trade(s4_net_sol, s4_gross_sol, was_toxic_baseline, s4_was_win);
            let is_toxic = s4_net_pnl < risk_cfg.stagnation_band_min_pct || s4_net_pnl <= risk_cfg.stop_loss_pct;
            s4_pending.push((exit_epoch, ctx.clone(), is_toxic));
        } else {
            strat4.record_blocked(was_toxic_baseline);
        }

        // --------------------------------------------------------------------
        // STRATEGY 5: Full Engine (Fixed 0.0417 SOL, Realistic Gap SL [-30%])
        // --------------------------------------------------------------------
        let s5_quarantine_ok = s5_quarantine.validate_pre_entry(ctx, &risk_cfg).is_ok();
        let s5_amm_ok = amm_gate.validate_pre_entry(ctx, &risk_cfg, &sandbox_cfg).is_ok();
        let s5_adm = s5_quarantine_ok && s5_amm_ok;
        if s5_adm {
            let (s5_net_pnl, s5_net_sol, s5_rule) = simulate_v071_exit(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs,
                FIXED_SLOT_SIZE_SOL, &risk_cfg, StopLossExecutionMode::RealisticGapSlippage
            );
            let (s5_gross_pnl_pct, _) = compute_v071_gross_pnl_pct(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs,
                &risk_cfg, StopLossExecutionMode::RealisticGapSlippage
            );
            let s5_gross_sol = FIXED_SLOT_SIZE_SOL * (s5_gross_pnl_pct / 100.0);
            let s5_was_win = s5_net_pnl > 0.0;
            strat5.record_trade(s5_net_sol, s5_gross_sol, was_toxic_baseline, s5_was_win);

            strat5_breakdown.push(TradeBreakdown {
                symbol: t.symbol.clone(),
                mint: t.mint.clone(),
                entry_price: t.entry_price_sol,
                peak_price: t.peak_price_sol,
                mfe_pct: t.mfe_pnl_pct,
                actual_exit_price: t.exit_price_sol,
                actual_pnl_pct: t.realized_pnl_pct,
                simulated_net_pnl_pct: s5_net_pnl,
                net_pnl_sol: s5_net_sol,
                exit_rule: s5_rule,
            });

            let is_toxic = s5_net_pnl < risk_cfg.stagnation_band_min_pct || s5_net_pnl <= risk_cfg.stop_loss_pct;
            s5_pending.push((exit_epoch, ctx.clone(), is_toxic));
        } else {
            strat5.record_blocked(was_toxic_baseline);
        }

        // --------------------------------------------------------------------
        // STRATEGY 6: Full Engine (Dynamic 7.5% Kelly Compounding, Realistic Gap SL)
        // --------------------------------------------------------------------
        let s6_quarantine_ok = s6_quarantine.validate_pre_entry(ctx, &risk_cfg).is_ok();
        let s6_amm_ok = amm_gate.validate_pre_entry(ctx, &risk_cfg, &sandbox_cfg).is_ok();
        let s6_adm = s6_quarantine_ok && s6_amm_ok;
        if s6_adm {
            let s6_slot_size = strat6.slot_size_kelly(risk_cfg.kelly_fraction_nav_pct);
            let (s6_net_pnl, s6_net_sol, _s6_rule) = simulate_v071_exit(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs,
                s6_slot_size, &risk_cfg, StopLossExecutionMode::RealisticGapSlippage
            );
            let (s6_gross_pnl_pct, _) = compute_v071_gross_pnl_pct(
                t.entry_price_sol, t.peak_price_sol, t.exit_price_sol, t.hold_secs,
                &risk_cfg, StopLossExecutionMode::RealisticGapSlippage
            );
            let s6_gross_sol = s6_slot_size * (s6_gross_pnl_pct / 100.0);
            let s6_was_win = s6_net_pnl > 0.0;
            strat6.record_trade(s6_net_sol, s6_gross_sol, was_toxic_baseline, s6_was_win);
            let is_toxic = s6_net_pnl < risk_cfg.stagnation_band_min_pct || s6_net_pnl <= risk_cfg.stop_loss_pct;
            s6_pending.push((exit_epoch, ctx.clone(), is_toxic));
        } else {
            strat6.record_blocked(was_toxic_baseline);
        }
    }

    // Print Institutional Comparison Table
    let strategies = vec![strat1, strat2, strat3, strat4, strat5, strat6];

    println!("+----------------------------------------------------------------+--------+--------+--------+--------+-----------+-----------+--------+--------+------------+");
    println!("| Backtest Strategy (Exact v0.7.1 Metrics Across All 200 Trades) | Trades | Toxic  | Win    | Win    | Gross     | Net       | Profit | Max DD | Expectancy |");
    println!("|                                                                | Taken  | Block  | Keep   | Rate % | PnL (SOL) | PnL (SOL) | Factor | (SOL)  | /Trade SOL |");
    println!("+----------------------------------------------------------------+--------+--------+--------+--------+-----------+-----------+--------+--------+------------+");

    for s in &strategies {
        println!("| {:<62} | {:>6} | {:>6} | {:>6} | {:>5.1}% | {:>+9.4} | {:>+9.4} | {:>6.2} | {:>6.4} | {:>+10.6} |",
            s.name, s.trades_taken, s.toxic_blocked, s.winners_preserved,
            s.win_rate_pct(), s.gross_sol, s.net_sol, s.profit_factor(),
            s.max_drawdown_sol, s.expectancy()
        );
    }
    println!("+----------------------------------------------------------------+--------+--------+--------+--------+-----------+-----------+--------+--------+------------+");

    // Print Top 10 Winners Breakdown under Strategy 5 (Realistic Gap-Slippage SL)
    strat5_breakdown.sort_by(|a, b| b.net_pnl_sol.partial_cmp(&a.net_pnl_sol).unwrap());

    println!("\n==========================================================================================");
    println!(" TOP 10 WINNING TRADES UNDER CURRENT v0.7.1 FULL ENGINE (Realistic Gap-Slippage SL)");
    println!("==========================================================================================");
    println!("| {:<10} | {:<44} | {:>10} | {:>10} | {:>9} | {:>11} | {:<22} |",
        "Symbol", "Mint", "MFE Peak %", "Raw PnL %", "Net PnL %", "Net PnL SOL", "Triggered Rule");
    println!("|------------+----------------------------------------------+------------+------------+-----------+-------------+------------------------|");
    for t in strat5_breakdown.iter().take(10) {
        println!("| {:<10} | {:<44} | {:>+9.1}% | {:>+9.1}% | {:>+8.1}% | {:>+10.6}  | {:<22} |",
            t.symbol, t.mint, t.mfe_pct, t.actual_pnl_pct, t.simulated_net_pnl_pct, t.net_pnl_sol, t.exit_rule);
    }

    // Print Worst 5 Losers Breakdown under Strategy 5 (Realistic Gap-Slippage SL)
    strat5_breakdown.sort_by(|a, b| a.net_pnl_sol.partial_cmp(&b.net_pnl_sol).unwrap());

    println!("\n==========================================================================================");
    println!(" WORST 5 LOSING TRADES UNDER CURRENT v0.7.1 FULL ENGINE (Realistic Gap-Slippage SL)");
    println!("==========================================================================================");
    println!("| {:<10} | {:<44} | {:>10} | {:>10} | {:>9} | {:>11} | {:<22} |",
        "Symbol", "Mint", "MFE Peak %", "Raw PnL %", "Net PnL %", "Net PnL SOL", "Triggered Rule");
    println!("|------------+----------------------------------------------+------------+------------+-----------+-------------+------------------------|");
    for t in strat5_breakdown.iter().take(5) {
        println!("| {:<10} | {:<44} | {:>+9.1}% | {:>+9.1}% | {:>+8.1}% | {:>+10.6}  | {:<22} |",
            t.symbol, t.mint, t.mfe_pct, t.actual_pnl_pct, t.simulated_net_pnl_pct, t.net_pnl_sol, t.exit_rule);
    }
    println!("==========================================================================================\n");
}
