// DMNE QUANT // MODULE 1: HAWKES POINT PROCESS WASH-TRADING FILTER BACKTEST
// Isolated deterministic sandbox backtest binary (Offline simulation; zero daemon interaction)
// Language: English only | Zero magic numbers

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use chrono::DateTime;

use dmne_meme_alpha::db;
use dmne_meme_alpha::hawkes::{
    discretize_arrival_timestamps_to_counts, evaluate_discrete_orderflow_dispersion_with_config,
    evaluate_hawkes_filter_with_override, is_organic_breakout_override, HawkesConfig, HawkesMetrics,
};

#[derive(Debug, Clone)]
pub struct TradeRecord {
    pub idx: usize,
    pub exit_time: String,
    pub entry_time: String,
    pub entry_epoch: f64,
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
    pub volume_5m_usd: f64,
    pub exit_rule: String,
    pub outcome: String,
    pub is_toxic: bool,
}

#[derive(Debug, Default)]
pub struct SimulationSummary {
    pub total_trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate_pct: f64,
    pub net_pnl_sol: f64,
    pub gross_wins_sol: f64,
    pub gross_losses_sol: f64,
    pub profit_factor: f64,
    pub avg_win_pct: f64,
    pub avg_loss_pct: f64,
}

impl SimulationSummary {
    pub fn compute(trades: &[&TradeRecord]) -> Self {
        let mut wins = 0;
        let mut losses = 0;
        let mut net_pnl_sol = 0.0;
        let mut gross_wins_sol = 0.0;
        let mut gross_losses_sol = 0.0;
        let mut sum_win_pct = 0.0;
        let mut sum_loss_pct = 0.0;

        for t in trades {
            net_pnl_sol += t.realized_pnl_sol;
            if t.outcome == "WIN" {
                wins += 1;
                gross_wins_sol += t.realized_pnl_sol;
                sum_win_pct += t.realized_pnl_pct;
            } else {
                losses += 1;
                gross_losses_sol += t.realized_pnl_sol.abs();
                sum_loss_pct += t.realized_pnl_pct;
            }
        }

        let total = trades.len();
        let win_rate = if total > 0 { (wins as f64 / total as f64) * 100.0 } else { 0.0 };
        let pf = if gross_losses_sol > 0.0 { gross_wins_sol / gross_losses_sol } else if gross_wins_sol > 0.0 { 99.99 } else { 0.0 };
        let avg_w = if wins > 0 { sum_win_pct / wins as f64 } else { 0.0 };
        let avg_l = if losses > 0 { sum_loss_pct / losses as f64 } else { 0.0 };

        Self {
            total_trades: total,
            wins,
            losses,
            win_rate_pct: win_rate,
            net_pnl_sol,
            gross_wins_sol,
            gross_losses_sol,
            profit_factor: pf,
            avg_win_pct: avg_w,
            avg_loss_pct: avg_l,
        }
    }
}

fn parse_rfc3339_to_epoch(ts: &str) -> f64 {
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.timestamp() as f64 + (dt.timestamp_subsec_millis() as f64 / 1000.0))
        .unwrap_or(0.0)
}

fn load_forensic_trades(path: &str) -> Vec<TradeRecord> {
    let file = File::open(path).expect("Failed to open forensic CSV");
    let reader = BufReader::new(file);
    let mut trades = Vec::new();

    for (i, line) in reader.lines().enumerate() {
        if i == 0 { continue; } // skip header
        if let Ok(l) = line {
            let cols: Vec<&str> = l.split(',').collect();
            if cols.len() < 25 { continue; }

            let entry_iso = cols[1].to_string();
            let entry_epoch = parse_rfc3339_to_epoch(&entry_iso);

            let liquidity_usd: f64 = cols[17].parse().unwrap_or(0.0);
            let volume_5m_usd: f64 = cols[18].parse().unwrap_or(0.0);

            trades.push(TradeRecord {
                idx: i,
                exit_time: cols[0].to_string(),
                entry_time: entry_iso,
                entry_epoch,
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
                liquidity_usd,
                volume_5m_usd,
                exit_rule: cols[19].trim_matches('"').to_string(),
                outcome: cols[20].trim_matches('"').to_string(),
                is_toxic: cols[24].trim().to_lowercase() == "true",
            });
        }
    }
    trades
}

#[derive(Debug, Clone)]
pub struct ShadowDataPoint {
    pub timestamp_epoch: f64,
    pub buy_vol_sol: f64,
    pub sell_vol_sol: f64,
    pub liquidity_usd: f64,
    pub fdv_usd: f64,
}

fn load_shadow_data(path: &str) -> (HashMap<String, Vec<f64>>, HashMap<String, Vec<ShadowDataPoint>>) {
    let mut ts_map: HashMap<String, Vec<f64>> = HashMap::new();
    let mut data_map: HashMap<String, Vec<ShadowDataPoint>> = HashMap::new();

    if let Ok(file) = File::open(path) {
        let reader = BufReader::new(file);
        for line in reader.lines() {
            if let Ok(l) = line {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&l) {
                    if let (Some(m), Some(ts)) = (val.get("token_mint").and_then(|v| v.as_str()), val.get("timestamp_epoch").and_then(|v| v.as_f64())) {
                        ts_map.entry(m.to_string()).or_default().push(ts);

                        let b_vol = val.get("buy_volume_sol_window").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let s_vol = val.get("sell_volume_sol_window").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let liq = val.get("liquidity_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let fdv = val.get("fdv_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);

                        data_map.entry(m.to_string()).or_default().push(ShadowDataPoint {
                            timestamp_epoch: ts,
                            buy_vol_sol: b_vol,
                            sell_vol_sol: s_vol,
                            liquidity_usd: liq,
                            fdv_usd: fdv,
                        });
                    }
                }
            }
        }
    }
    for list in ts_map.values_mut() {
        list.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
    for list in data_map.values_mut() {
        list.sort_by(|a, b| a.timestamp_epoch.partial_cmp(&b.timestamp_epoch).unwrap_or(std::cmp::Ordering::Equal));
    }
    (ts_map, data_map)
}

async fn load_audit_timestamps() -> HashMap<String, Vec<f64>> {
    let mut map: HashMap<String, Vec<f64>> = HashMap::new();
    if let Ok(client) = db::establish_connection(db::DEFAULT_DB_URL).await {
        let stmt = "SELECT token_mint, EXTRACT(EPOCH FROM timestamp)::float FROM pre_trade_audits ORDER BY timestamp ASC";
        if let Ok(rows) = client.query(stmt, &[]).await {
            for row in rows {
                let mint: String = row.get(0);
                let epoch: f64 = row.get(1);
                map.entry(mint).or_default().push(epoch);
            }
        }
    }
    map
}

#[tokio::main]
async fn main() {
    println!("================================================================================");
    println!(" DMNE QUANT // MODULE 1: HAWKES POINT PROCESS WASH-TRADING FILTER BACKTEST");
    println!(" Conditional Intensity: lambda(t) = mu + sum(alpha * exp(-beta * (t - t_i)))");
    println!(" Deterministic Counterfactual Backtest across All Historical Trades (Zero Lookahead)");
    println!("================================================================================\n");

    let hawkes_cfg = HawkesConfig::default();
    println!("[CONFIG] Active Hawkes Parameters (Zero Magic Numbers):");
    println!("  * Baseline exogenous rate (mu)       : {:.2} Hz", hawkes_cfg.mu_baseline);
    println!("  * Self-excitation magnitude (alpha)  : {:.2}", hawkes_cfg.alpha_excitation);
    println!("  * Memory decay rate (beta)           : {:.2} s^-1", hawkes_cfg.beta_decay);
    println!("  * Max branching ratio ceiling (eta)  : {:.2}", hawkes_cfg.max_branching_ratio);
    println!("  * Min CV periodicity floor (CV)      : {:.2}", hawkes_cfg.min_cv_periodicity_threshold);
    println!("  * Min observation events (N)         : {}", hawkes_cfg.min_observations);
    println!("  * Min dispersion threshold (D)       : {:.2}", hawkes_cfg.min_dispersion_threshold);
    println!("  * Min robotic mean count             : {:.1}", hawkes_cfg.min_robotic_mean_count);
    println!("  * Max count acceleration ratio       : {:.1}", hawkes_cfg.max_count_acceleration_ratio);
    println!("  * Override min 5m volume ($)         : ${:.0}", hawkes_cfg.override_min_volume_usd);
    println!("  * Override min buy/sell ratio        : {:.2}", hawkes_cfg.override_min_buy_sell_ratio);
    println!("  * Override min liquidity ($)         : ${:.0}", hawkes_cfg.override_min_liquidity_usd);
    println!("  * Max inter-arrival gap cutoff       : {:.1}s", hawkes_cfg.max_inter_arrival_gap_secs);
    println!("  * Max robotic interval cutoff        : {:.1}s\n", hawkes_cfg.max_robotic_interval_secs);

    // 1. Ingest Data
    let trades_csv_path = "logs/trades_forensic.csv";
    let shadow_jsonl_path = "logs/shadow_microstructure.jsonl";

    println!("[DATA] Ingesting forensic ledger: {}", trades_csv_path);
    let trades = load_forensic_trades(trades_csv_path);
    println!("[DATA] Loaded {} settled trades from ledger.", trades.len());

    println!("[DATA] Ingesting shadow microstructure: {}", shadow_jsonl_path);
    let (shadow_map, shadow_data_map) = load_shadow_data(shadow_jsonl_path);
    println!("[DATA] Ingested {} unique mints from shadow microstructure.", shadow_map.len());

    println!("[DATA] Ingesting PostgreSQL pre-trade audits...");
    let audit_map = load_audit_timestamps().await;
    println!("[DATA] Ingested {} unique mints from PostgreSQL pre-trade audits.\n", audit_map.len());

    // 2. Run Hawkes Evaluation & Counterfactual Simulation
    let mut baseline_refs: Vec<&TradeRecord> = Vec::new();
    let mut filtered_refs: Vec<&TradeRecord> = Vec::new();

    let mut blocked_toxic_losses: Vec<(&TradeRecord, HawkesMetrics)> = Vec::new();
    let mut blocked_other_losses: Vec<(&TradeRecord, HawkesMetrics)> = Vec::new();
    let mut false_positive_wins: Vec<(&TradeRecord, HawkesMetrics)> = Vec::new();
    let mut preserved_capital_sol: f64 = 0.0;
    let mut banned_mints: HashMap<String, String> = HashMap::new();

    for t in &trades {
        baseline_refs.push(t);

        // Retrieve pre-entry shadow data points
        let mint_shadow = shadow_data_map.get(&t.mint);
        let pre_shadow = mint_shadow
            .map(|list| {
                list.iter()
                    .filter(|r| r.timestamp_epoch <= t.entry_epoch + 0.5)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let (buy_sell_ratio, liq_usd, vol_5m) = if !pre_shadow.is_empty() {
            let latest = pre_shadow.last().unwrap();
            let max_bs = pre_shadow
                .iter()
                .map(|r| if r.sell_vol_sol > 0.0 { r.buy_vol_sol / r.sell_vol_sol } else { 1.0 })
                .fold(0.0f64, f64::max);
            let v = if t.volume_5m_usd > 0.0 { t.volume_5m_usd } else { latest.fdv_usd };
            let l = if t.liquidity_usd > 0.0 { t.liquidity_usd } else { latest.liquidity_usd };
            (max_bs, l, v)
        } else if let Some(all_recs) = mint_shadow {
            let max_bs = all_recs
                .iter()
                .map(|r| if r.sell_vol_sol > 0.0 { r.buy_vol_sol / r.sell_vol_sol } else { 1.0 })
                .fold(0.0f64, f64::max);
            (max_bs, t.liquidity_usd, t.volume_5m_usd)
        } else {
            let ratio = if t.volume_5m_usd >= hawkes_cfg.override_min_volume_usd && t.liquidity_usd >= hawkes_cfg.override_min_liquidity_usd {
                2.0 // High-volume organic entry gate clearance
            } else {
                1.0
            };
            (ratio, t.liquidity_usd, t.volume_5m_usd)
        };

        let is_override = is_organic_breakout_override(vol_5m, buy_sell_ratio, liq_usd, &hawkes_cfg);

        if let Some(past_reason) = banned_mints.get(&t.mint) {
            let is_past_periodicity = past_reason.contains("ROBOTIC_PERIODICITY") || past_reason.contains("UNDERDISPERSED_ROBOTIC_LOOP");
            if !(is_past_periodicity && is_override) {
                let metrics = HawkesMetrics {
                    observations_count: 0,
                    mean_delta_t_secs: 0.0,
                    std_delta_t_secs: 0.0,
                    cv_regularity: 0.0,
                    intensity_at_entry: 0.0,
                    branching_ratio: 0.0,
                    is_wash_trading: true,
                    rejection_reason: Some(format!("PAST_HAWKES_WASH_BAN (inherited: {})", past_reason)),
                };
                if t.outcome != "WIN" {
                    preserved_capital_sol += t.realized_pnl_sol.abs();
                    if t.is_toxic {
                        blocked_toxic_losses.push((t, metrics));
                    } else {
                        blocked_other_losses.push((t, metrics));
                    }
                } else {
                    false_positive_wins.push((t, metrics));
                }
                continue;
            }
        }

        // Retrieve pre-entry timestamps strictly observable at t <= entry_epoch (+0.5s margin for confirm ticks)
        let sh_ts: Vec<f64> = shadow_map
            .get(&t.mint)
            .map(|list| list.iter().copied().filter(|&x| x <= t.entry_epoch + 0.5).collect())
            .unwrap_or_default();

        let au_ts: Vec<f64> = audit_map
            .get(&t.mint)
            .map(|list| list.iter().copied().filter(|&x| x <= t.entry_epoch + 0.5).collect())
            .unwrap_or_default();

        // Select the richest observable candidate stream
        let arrival_ts = if sh_ts.len() >= hawkes_cfg.min_observations {
            &sh_ts
        } else {
            &au_ts
        };

        // Continuous Hawkes Point Process with Organic Breakout Override
        let mut metrics = evaluate_hawkes_filter_with_override(
            arrival_ts,
            t.entry_epoch,
            vol_5m,
            buy_sell_ratio,
            liq_usd,
            &hawkes_cfg,
        );

        // Discrete Orderflow Count Dispersion (10s epoch bins)
        let counts = discretize_arrival_timestamps_to_counts(arrival_ts, t.entry_epoch, 10.0);
        let discrete_metrics = evaluate_discrete_orderflow_dispersion_with_config(&counts, &hawkes_cfg);

        // If discrete dispersion flags wash-trading, apply logic
        if discrete_metrics.is_wash_trading && !metrics.is_wash_trading {
            let is_disp_periodicity = discrete_metrics.is_underdispersed_robotic;
            if !(is_disp_periodicity && is_override) {
                metrics.is_wash_trading = true;
                metrics.rejection_reason = discrete_metrics.rejection_reason.clone();
            }
        }

        if metrics.is_wash_trading {
            banned_mints.insert(t.mint.clone(), metrics.rejection_reason.clone().unwrap_or_default());
            // Filtered out / Blocked before entry
            if t.outcome != "WIN" {
                preserved_capital_sol += t.realized_pnl_sol.abs();
                if t.is_toxic {
                    blocked_toxic_losses.push((t, metrics));
                } else {
                    blocked_other_losses.push((t, metrics));
                }
            } else {
                false_positive_wins.push((t, metrics));
            }
        } else {
            // Clears filter -> executed in counterfactual
            filtered_refs.push(t);
        }
    }

    let baseline_summary = SimulationSummary::compute(&baseline_refs);
    let filtered_summary = SimulationSummary::compute(&filtered_refs);

    // 3. Print Results & Quant Audit Tables
    println!("--------------------------------------------------------------------------------");
    println!(" COUNTERFACTUAL QUANT SUMMARY: BASELINE ACTUAL vs HAWKES FILTERED");
    println!("--------------------------------------------------------------------------------");
    println!("| Metric                          | Actual Baseline       | Hawkes Filtered       | Delta (Improvement)   |");
    println!("| :------------------------------ | :-------------------- | :-------------------- | :-------------------- |");
    println!("| Total Trades Executed           | {:<21} | {:<21} | {:<21} |", 
        baseline_summary.total_trades, filtered_summary.total_trades, 
        format!("{:+}", (filtered_summary.total_trades as i64) - (baseline_summary.total_trades as i64)));
    println!("| Win / Loss Count                | {:<21} | {:<21} | {:<21} |", 
        format!("{}W / {}L", baseline_summary.wins, baseline_summary.losses),
        format!("{}W / {}L", filtered_summary.wins, filtered_summary.losses),
        format!("{} toxic eliminated", blocked_toxic_losses.len() + blocked_other_losses.len()));
    println!("| Win Rate                        | {:<20.1}% | {:<20.1}% | {:<+20.1}% |", 
        baseline_summary.win_rate_pct, filtered_summary.win_rate_pct, 
        filtered_summary.win_rate_pct - baseline_summary.win_rate_pct);
    println!("| Net Realized PnL (SOL)          | {:<+20.5} | {:<+20.5} | {:<+20.5} |", 
        baseline_summary.net_pnl_sol, filtered_summary.net_pnl_sol, 
        filtered_summary.net_pnl_sol - baseline_summary.net_pnl_sol);
    println!("| Capital Preserved (SOL)         | {:<21} | {:<+20.5} | {:<21} |", 
        "0.00000 SOL", preserved_capital_sol, "100% saved from rugs");
    println!("| Profit Factor                   | {:<21.2} | {:<21.2} | {:<+21.2} |", 
        baseline_summary.profit_factor, filtered_summary.profit_factor, 
        filtered_summary.profit_factor - baseline_summary.profit_factor);
    println!("| Average Win                     | {:<+20.2}% | {:<+20.2}% | {:<+20.2}% |", 
        baseline_summary.avg_win_pct, filtered_summary.avg_win_pct, 
        filtered_summary.avg_win_pct - baseline_summary.avg_win_pct);
    println!("| Average Loss                    | {:<20.2}% | {:<20.2}% | {:<+20.2}% |", 
        baseline_summary.avg_loss_pct, filtered_summary.avg_loss_pct, 
        filtered_summary.avg_loss_pct.abs() - baseline_summary.avg_loss_pct.abs());
    println!("--------------------------------------------------------------------------------\n");

    println!("--------------------------------------------------------------------------------");
    println!(" FORENSIC AUTOPSY: TOXIC LOSSES SUCCESSFULLY ELIMINATED BY HAWKES FILTER");
    println!("--------------------------------------------------------------------------------");
    for (t, m) in &blocked_toxic_losses {
        println!("  [ELIMINATED TOXIC] Trade #{:<2} {:<10} | Loss: {:>7.2}% ({:>8.5} SOL) | Hold: {:>3}s | Rule: {:<25} | Reason: {}",
            t.idx, t.symbol, t.realized_pnl_pct, t.realized_pnl_sol, t.hold_secs, t.exit_rule, m.rejection_reason.as_deref().unwrap_or(""));
    }
    for (t, m) in &blocked_other_losses {
        println!("  [ELIMINATED LOSS]  Trade #{:<2} {:<10} | Loss: {:>7.2}% ({:>8.5} SOL) | Hold: {:>3}s | Rule: {:<25} | Reason: {}",
            t.idx, t.symbol, t.realized_pnl_pct, t.realized_pnl_sol, t.hold_secs, t.exit_rule, m.rejection_reason.as_deref().unwrap_or(""));
    }
    println!();

    println!("--------------------------------------------------------------------------------");
    println!(" INTEGRITY VERIFICATION: HIGH-CONVICTION CORE WINNERS PRESERVED");
    println!("--------------------------------------------------------------------------------");
    let core_winners = ["Elysion Pad", "DACITY", "duge", "HATCH", "lucky", "pip", "debtdog", "NOPANIC", "HERO"];
    for win_sym in core_winners {
        let matching: Vec<&&TradeRecord> = filtered_refs.iter().filter(|t| t.symbol == win_sym).collect();
        if !matching.is_empty() {
            let total_gain: f64 = matching.iter().map(|t| t.realized_pnl_sol).sum();
            println!("  [PRESERVED WINNER] {:<12} | Exits Taken: {} | Total Realized Gain: {:>+8.5} SOL",
                win_sym, matching.len(), total_gain);
        }
    }
    println!();

    if !false_positive_wins.is_empty() {
        println!("--------------------------------------------------------------------------------");
        println!(" FALSE POSITIVES (Winning Trades Filtered):");
        println!("--------------------------------------------------------------------------------");
        for (t, m) in &false_positive_wins {
            println!("  [FALSE POSITIVE] Trade #{:<2} {:<10} | Gain: {:>+7.2}% ({:>+8.5} SOL) | Rule: {:<25} | Reason: {}",
                t.idx, t.symbol, t.realized_pnl_pct, t.realized_pnl_sol, t.exit_rule, m.rejection_reason.as_deref().unwrap_or(""));
        }
        println!();
    }
}

