// DMNE QUANT // POST-TRADE FORENSIC AUTOPSY AGENT & GATE CALIBRATOR CLI
// Offline Cold-Path Ingestion, Root Cause Diagnostic & Counterfactual Evaluation

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use serde_json::Value;

use dmne_meme_alpha::cold_path::{
    AutopsyConfig, ColdPathAuditor, SettledTradeRecord, TradeAutopsyReport
};
use dmne_meme_alpha::db;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("================================================================================");
    println!("  DMNE MEME ALPHA // POST-TRADE FORENSIC AUTOPSY & GATE CALIBRATION AGENT       ");
    println!("  Cold-Path Diagnostics & Counterfactual Optimization Engine                    ");
    println!("================================================================================\n");

    // 1. Establish PostgreSQL Connection & Initialize Autopsy Table
    let client_opt = match db::establish_connection(db::DEFAULT_DB_URL).await {
        Ok(c) => {
            if let Err(e) = db::init_autopsy_tables(&c).await {
                eprintln!("[DB WARNING] Failed to initialize autopsy table: {}", e);
            }
            Some(c)
        }
        Err(e) => {
            eprintln!("[DB WARNING] Could not connect to PostgreSQL (offline mode): {}", e);
            None
        }
    };

    let audited_ids: HashSet<i32> = if let Some(ref client) = client_opt {
        db::fetch_audited_trade_ids(client).await.unwrap_or_default()
    } else {
        HashSet::new()
    };
    println!("[DB] Loaded {} previously audited trade records from PostgreSQL", audited_ids.len());

    // 2. Index Shadow Microstructure Logs (CVD & Hawkes Telemetry)
    let shadow_path = "logs/shadow_microstructure.jsonl";
    let shadow_cache = load_shadow_microstructure_telemetry(shadow_path);
    println!("[TELEMETRY] Indexed entry orderflow context for {} unique token mints from {}", shadow_cache.len(), shadow_path);

    // 3. Load Settled Historical Trades from Forensic Ledger
    let ledger_path = "logs/trades_forensic.csv";
    let records = load_settled_trades_from_csv(ledger_path, &shadow_cache)?;
    println!("[DATA] Ingested {} total settled trades from {}\n", records.len(), ledger_path);

    // 4. Filter for Target Un-audited or Loss Trades
    // Filter criteria: realized_pnl_pct <= -0.10 OR Max Slippage exits
    let qualifying_trades: Vec<&SettledTradeRecord> = records.iter().filter(|t| {
        t.realized_pnl_pct <= -0.10 
            || t.exit_rule.contains("Max Slippage") 
            || t.realized_pnl_sol < 0.0
    }).collect();

    println!("[AUDIT SCAN] Identified {} loss / high-slippage trades requiring forensic examination.", qualifying_trades.len());

    let auditor = ColdPathAuditor::new(AutopsyConfig::default());
    let mut autopsies: Vec<TradeAutopsyReport> = Vec::new();
    let mut new_audits_count = 0;

    for trade in qualifying_trades {
        if let Some(report) = auditor.audit_settled_trade(trade) {
            if let Some(ref client) = client_opt {
                let rec_json = serde_json::to_value(&report.counterfactual).unwrap_or(Value::Null);
                let rec_str = rec_json.to_string();
                match db::insert_trade_autopsy(
                    client,
                    report.trade_id,
                    &report.symbol,
                    &report.mint,
                    report.realized_pnl_pct,
                    report.realized_pnl_sol,
                    &report.exit_rule,
                    report.hold_duration_secs,
                    report.peak_mfe_pct,
                    report.max_mae_pct,
                    report.slippage_drift_pct,
                    report.entry_cvd_ratio,
                    report.entry_hawkes_eta,
                    report.root_cause_category.as_str(),
                    &rec_str,
                ).await {
                    Ok(_) => {
                        if !audited_ids.contains(&report.trade_id) {
                            new_audits_count += 1;
                        }
                    }
                    Err(e) => {
                        eprintln!("[DB INSERT ERROR] Trade #{}: {}", report.trade_id, e);
                    }
                }
            }
            autopsies.push(report);
        }
    }

    if let Some(_) = client_opt {
        println!("[DB] Synchronized {} new / updated forensic autopsies into PostgreSQL\n", new_audits_count);
    }

    // 5. Display Forensic Autopsy Summary Table
    println!("----------------------------------------------------------------------------------------------------------------------------------");
    println!("  POST-MORTEM FORENSIC AUDIT TABLE (Loss & Slippage Settlements)");
    println!("----------------------------------------------------------------------------------------------------------------------------------");
    println!(
        "| {:<4} | {:<10} | {:<7} | {:<10} | {:<11} | {:<22} | {:<8} | {:<18} | {:<28} |",
        "ID", "Symbol", "Hold(s)", "PnL (%)", "PnL (SOL)", "Exit Rule", "Drift%", "Root Cause", "Counterfactual Avoidance"
    );
    println!("|------|------------|---------|------------|-------------|------------------------|----------|--------------------|------------------------------|");

    let mut total_loss_sol = 0.0;
    let mut avoided_loss_sol = 0.0;
    let mut rapid_rug_count = 0;
    let mut excessive_slippage_count = 0;
    let mut stagnation_decay_count = 0;
    let mut organic_stop_count = 0;

    for r in &autopsies {
        total_loss_sol += r.realized_pnl_sol.abs();
        if r.counterfactual.would_avoid_trade {
            avoided_loss_sol += r.counterfactual.avoided_loss_sol;
        }

        match r.root_cause_category.as_str() {
            "RAPID_DRAIN_RUG" => rapid_rug_count += 1,
            "EXCESSIVE_SLIPPAGE" => excessive_slippage_count += 1,
            "STAGNATION_DECAY" => stagnation_decay_count += 1,
            "ORGANIC_STOP" => organic_stop_count += 1,
            _ => {}
        }

        let cf_badge = if r.counterfactual.would_avoid_trade {
            format!("AVOIDED ({:.4} SOL)", r.counterfactual.avoided_loss_sol)
        } else {
            "UNAVOIDED".to_string()
        };

        println!(
            "| #{:<3} | {:<10} | {:>7.0} | {:>9.2}% | {:>10.5} | {:<22} | {:>7.1}% | {:<18} | {:<28} |",
            r.trade_id,
            truncate_str(&r.symbol, 10),
            r.hold_duration_secs,
            r.realized_pnl_pct,
            r.realized_pnl_sol,
            truncate_str(&r.exit_rule, 22),
            r.slippage_drift_pct,
            r.root_cause_category.as_str(),
            cf_badge
        );
    }
    println!("----------------------------------------------------------------------------------------------------------------------------------\n");

    // 6. Aggregate Calibrator Analytics
    let total_loss_trades = autopsies.len();
    let avoided_trades_count = autopsies.iter().filter(|r| r.counterfactual.would_avoid_trade).count();
    let avoidance_rate_pct = if total_loss_trades > 0 {
        (avoided_trades_count as f64 / total_loss_trades as f64) * 100.0
    } else {
        0.0
    };
    let capital_saved_pct = if total_loss_sol > 0.0 {
        (avoided_loss_sol / total_loss_sol) * 100.0
    } else {
        0.0
    };

    println!("================================================================================");
    println!("  FORENSIC ROOT CAUSE BREAKDOWN & COUNTERFACTUAL CALIBRATION METRICS             ");
    println!("================================================================================");
    println!("  Total Examined Loss Trades:     {}", total_loss_trades);
    println!("    - Rapid Drain Rugs (<120s):   {} ({:.1}%)", rapid_rug_count, pct(rapid_rug_count, total_loss_trades));
    println!("    - Excessive Slippage (>5%):   {} ({:.1}%)", excessive_slippage_count, pct(excessive_slippage_count, total_loss_trades));
    println!("    - Stagnation Decays (TTL):    {} ({:.1}%)", stagnation_decay_count, pct(stagnation_decay_count, total_loss_trades));
    println!("    - Organic Stops / Early Abort:{} ({:.1}%)", organic_stop_count, pct(organic_stop_count, total_loss_trades));
    println!("--------------------------------------------------------------------------------");
    println!("  Counterfactual Optimization Impact (Liq >= $35k | CVD >= +25% | Hawkes < 1.15):");
    println!("    - Avoidable Loss Trades:      {} / {} ({:.1}%)", avoided_trades_count, total_loss_trades, avoidance_rate_pct);
    println!("    - Gross Sol Saved / Rescued:  +{:.4} SOL / {:.4} SOL ({:.1}% of lost capital)", avoided_loss_sol, total_loss_sol, capital_saved_pct);
    println!("================================================================================\n");

    // 7. Bi-Directional Forensic Replay across Entire Historical Trade Sample (Winners & Losers)
    let replay = auditor.run_bidirectional_replay(&records);

    println!("================================================================================");
    println!("  BI-DIRECTIONAL FORENSIC REPLAY & UNBIASED ALPHA RECONCILIATION                ");
    println!("================================================================================");
    println!("  Total Settled Trades Replayed:  {} (Losses: {}, Wins: {})", replay.total_trades_evaluated, replay.total_losses_evaluated, replay.total_wins_evaluated);
    println!("  Gross Losses Rescued:          +{:>8.4} SOL  ({} losing trades averted)", replay.gross_losses_rescued_sol, replay.rescued_losses_count);
    println!("  Gross Wins Sacrificed:         -{:>8.4} SOL  ({} winning trades blocked)", replay.gross_wins_sacrificed_sol, replay.sacrificed_wins_count);
    println!("  ------------------------------------------------------------------------------");
    println!("  Net Alpha Delta:               {:>+8.4} SOL", replay.net_alpha_delta_sol);
    println!("  Staged PR Eligibility:         {} (Requires Net Delta > +0.0150 SOL & Sample >= 30)", 
        if replay.is_pr_eligible { "ELIGIBLE (Net Alpha Positive)" } else { "BLOCKED (Net Alpha Delta <= 0.0150 SOL or Sample < 30)" }
    );
    println!("================================================================================\n");

    Ok(())
}

fn pct(part: usize, whole: usize) -> f64 {
    if whole > 0 { (part as f64 / whole as f64) * 100.0 } else { 0.0 }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    } else {
        s.to_string()
    }
}

/// Loads shadow microstructure records and builds an entry telemetry lookup map (mint -> (cvd_ratio, hawkes_eta))
fn load_shadow_microstructure_telemetry(path: &str) -> HashMap<String, (f64, Option<f64>)> {
    let mut cache = HashMap::new();
    if let Ok(file) = File::open(path) {
        let reader = BufReader::new(file);
        for line in reader.lines().flatten() {
            if line.trim().is_empty() { continue; }
            if let Ok(val) = serde_json::from_str::<Value>(&line) {
                if let Some(mint) = val.get("token_mint").and_then(|m| m.as_str()) {
                    let buy_vol = val.get("buy_volume_sol_window").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let sell_vol = val.get("sell_volume_sol_window").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let total_vol = buy_vol + sell_vol;
                    let cvd_ratio = if total_vol > 0.0 {
                        (buy_vol - sell_vol) / total_vol
                    } else {
                        0.0
                    };

                    let hawkes_eta = val.get("swap_interval_deltas_ms")
                        .and_then(|arr| arr.as_array())
                        .and_then(|arr| {
                            if arr.len() >= 5 {
                                let mean_delta_ms: f64 = arr.iter().filter_map(|d| d.as_f64()).sum::<f64>() / arr.len() as f64;
                                if mean_delta_ms > 0.0 && mean_delta_ms < 1000.0 {
                                    Some(1.20) // Supercritical clustering proxy
                                } else {
                                    Some(0.85) // Organic subcritical proxy
                                }
                            } else {
                                None
                            }
                        });

                    cache.insert(mint.to_string(), (cvd_ratio, hawkes_eta));
                }
            }
        }
    }
    cache
}

/// Parses historical settled trades from CSV forensic ledger
fn load_settled_trades_from_csv(
    path: &str,
    shadow_cache: &HashMap<String, (f64, Option<f64>)>,
) -> Result<Vec<SettledTradeRecord>, std::io::Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for (line_idx, line_res) in reader.lines().enumerate() {
        if line_idx == 0 { continue; } // Skip header row
        let line = line_res?;
        if line.trim().is_empty() { continue; }

        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 24 { continue; }

        let trade_id = line_idx as i32; // Sequential trade index (1 to N)
        let hold_secs: f64 = cols[2].parse().unwrap_or(0.0);
        let symbol = cols[3].trim().to_string();
        let mint = cols[4].trim().to_string();
        let size_sol: f64 = cols[7].parse().unwrap_or(0.045);
        let entry_price_sol: f64 = cols[8].parse().unwrap_or(0.0);
        let peak_price_sol: f64 = cols[9].parse().unwrap_or(0.0);
        let trough_price_sol: f64 = cols[10].parse().unwrap_or(0.0);
        let exit_price_sol: f64 = cols[11].parse().unwrap_or(0.0);
        let mfe_pnl_pct: f64 = cols[12].parse().unwrap_or(0.0);
        let mae_pnl_pct: f64 = cols[13].parse().unwrap_or(0.0);
        let realized_pnl_pct: f64 = cols[14].parse().unwrap_or(0.0);
        let realized_pnl_sol: f64 = cols[15].parse().unwrap_or(0.0);
        let liquidity_usd: f64 = cols[17].parse().unwrap_or(0.0);
        let fdv_usd: f64 = cols[18].parse().unwrap_or(0.0);
        let exit_rule = cols[19].trim().trim_matches('"').to_string();
        let is_toxic: bool = cols[24].trim().parse().unwrap_or(false);

        let (cvd_ratio, hawkes_eta) = shadow_cache.get(&mint)
            .map(|(c, h)| (Some(*c), *h))
            .unwrap_or((None, None));

        records.push(SettledTradeRecord {
            trade_id,
            symbol,
            mint,
            hold_duration_secs: hold_secs,
            size_sol,
            entry_price_sol,
            peak_price_sol,
            trough_price_sol,
            exit_price_sol,
            mfe_pnl_pct,
            mae_pnl_pct,
            realized_pnl_pct,
            realized_pnl_sol,
            liquidity_usd,
            fdv_usd,
            exit_rule,
            is_toxic,
            entry_cvd_ratio: cvd_ratio,
            entry_hawkes_eta: hawkes_eta,
        });
    }

    Ok(records)
}
