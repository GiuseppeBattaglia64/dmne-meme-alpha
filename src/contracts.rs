use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};

use crate::config::RiskConfig;
use crate::risk;

pub const CONTRACT_SCHEMA_VERSION: &str = "2.0.0";
pub const DEFAULT_SOL_EUR_RATE: f64 = 86.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrancheStage {
    TP1,
    TP2,
    Runner,
}

impl TrancheStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrancheStage::TP1 => "TP1",
            TrancheStage::TP2 => "TP2",
            TrancheStage::Runner => "Runner",
        }
    }

    pub fn from_exit_rule(rule: &str, tranche_1_taken: bool, tranche_2_taken: bool) -> Self {
        if rule.contains("TP1") || (!tranche_1_taken && rule.contains("45%")) {
            TrancheStage::TP1
        } else if rule.contains("TP2") || (!tranche_2_taken && rule.contains("90%")) {
            TrancheStage::TP2
        } else {
            TrancheStage::Runner
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettledTradeRecord {
    pub schema_version: String,
    pub timestamp_exit_utc: String,
    pub timestamp_entry_utc: String,
    pub hold_duration_secs: u64,
    pub token_symbol: String,
    pub token_mint: String,
    pub pair_address: String,
    pub dex_url: String,
    pub size_sol: f64,
    pub entry_price_sol: f64,
    pub peak_price_sol: f64,
    pub trough_price_sol: f64,
    pub exit_price_sol: f64,
    pub mfe_pnl_pct: f64,
    pub mae_pnl_pct: f64,
    pub realized_pnl_pct: f64,
    pub realized_pnl_sol: f64,
    pub realized_pnl_eur: f64,
    pub liquidity_usd: f64,
    pub fdv_usd: f64,
    pub exit_rule: String,
    pub outcome: String,
    pub actual_fee_sol: f64,
    pub bankroll_before_sol: f64,
    pub bankroll_after_sol: f64,
    pub is_toxic: bool,
    pub exit_tip_lamports: u64,
    pub tranche_stage: TrancheStage,
}

impl SettledTradeRecord {
    pub const CSV_HEADER: &'static str = "timestamp_exit_utc,timestamp_entry_utc,hold_duration_secs,token_symbol,token_mint,pair_address,dex_url,size_sol,entry_price_sol,peak_price_sol,trough_price_sol,exit_price_sol,mfe_pnl_pct,mae_pnl_pct,realized_pnl_pct,realized_pnl_sol,realized_pnl_eur,liquidity_usd,fdv_usd,exit_rule,outcome,actual_fee_sol,bankroll_before_sol,bankroll_after_sol,is_toxic,exit_tip_lamports\n";

    pub fn to_csv_row(&self) -> String {
        let safe_symbol = self.token_symbol.replace(',', " ");
        let safe_rule = self.exit_rule.replace('"', "'");
        format!(
            "{},{},{},{},{},{},{},{:.4},{:.10e},{:.10e},{:.10e},{:.10e},{:.2},{:.2},{:.2},{:.6},{:.2},{:.2},{:.2},\"{}\",{},{:.6},{:.4},{:.4},{},{}\n",
            self.timestamp_exit_utc,
            self.timestamp_entry_utc,
            self.hold_duration_secs,
            safe_symbol,
            self.token_mint,
            self.pair_address,
            self.dex_url,
            self.size_sol,
            self.entry_price_sol,
            self.peak_price_sol,
            self.trough_price_sol,
            self.exit_price_sol,
            self.mfe_pnl_pct,
            self.mae_pnl_pct,
            self.realized_pnl_pct,
            self.realized_pnl_sol,
            self.realized_pnl_eur,
            self.liquidity_usd,
            self.fdv_usd,
            safe_rule,
            self.outcome,
            self.actual_fee_sol,
            self.bankroll_before_sol,
            self.bankroll_after_sol,
            self.is_toxic,
            self.exit_tip_lamports
        )
    }

    pub fn from_csv_line(line: &str) -> Option<Self> {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 20 {
            return None;
        }
        let exit_rule = cols[19].trim_matches('"').to_string();
        let pnl_pct = cols[14].parse::<f64>().unwrap_or(0.0);
        let pnl_sol = cols[15].parse::<f64>().unwrap_or(0.0);
        let is_toxic = if cols.len() > 24 {
            cols[24]
                .trim()
                .parse::<bool>()
                .unwrap_or_else(|_| risk::is_historical_exit_toxic(pnl_pct, &RiskConfig::default()))
        } else {
            risk::is_historical_exit_toxic(pnl_pct, &RiskConfig::default())
        };
        let exit_tip_lamports = if cols.len() > 25 {
            cols[25]
                .trim()
                .parse::<u64>()
                .unwrap_or_else(|_| risk::resolve_exit_tip(&exit_rule, pnl_sol, 1.0, &RiskConfig::default()))
        } else {
            risk::resolve_exit_tip(&exit_rule, pnl_sol, 1.0, &RiskConfig::default())
        };
        let tranche_stage = TrancheStage::from_exit_rule(&exit_rule, false, false);

        Some(Self {
            schema_version: CONTRACT_SCHEMA_VERSION.to_string(),
            timestamp_exit_utc: cols[0].to_string(),
            timestamp_entry_utc: cols[1].to_string(),
            hold_duration_secs: cols[2].parse::<u64>().unwrap_or(0),
            token_symbol: cols[3].to_string(),
            token_mint: cols[4].to_string(),
            pair_address: cols[5].to_string(),
            dex_url: cols[6].to_string(),
            size_sol: cols[7].parse::<f64>().unwrap_or(0.0),
            entry_price_sol: cols[8].parse::<f64>().unwrap_or(0.0),
            peak_price_sol: cols[9].parse::<f64>().unwrap_or(0.0),
            trough_price_sol: cols[10].parse::<f64>().unwrap_or(0.0),
            exit_price_sol: cols[11].parse::<f64>().unwrap_or(0.0),
            mfe_pnl_pct: cols[12].parse::<f64>().unwrap_or(0.0),
            mae_pnl_pct: cols[13].parse::<f64>().unwrap_or(0.0),
            realized_pnl_pct: pnl_pct,
            realized_pnl_sol: pnl_sol,
            realized_pnl_eur: cols[16].parse::<f64>().unwrap_or(pnl_sol * DEFAULT_SOL_EUR_RATE),
            liquidity_usd: cols[17].parse::<f64>().unwrap_or(0.0),
            fdv_usd: cols[18].parse::<f64>().unwrap_or(0.0),
            exit_rule,
            outcome: cols.get(20).map(|s| s.to_string()).unwrap_or_else(|| "SCRATCH".to_string()),
            actual_fee_sol: cols.get(21).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
            bankroll_before_sol: cols.get(22).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
            bankroll_after_sol: cols.get(23).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
            is_toxic,
            exit_tip_lamports,
            tranche_stage,
        })
    }

    pub fn parse_csv_file(path: &str) -> Result<Vec<Self>, std::io::Error> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for (i, line) in reader.lines().enumerate() {
            let line = line?;
            if i == 0 || line.trim().is_empty() {
                continue;
            }
            if let Some(rec) = Self::from_csv_line(&line) {
                out.push(rec);
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSnapshot {
    pub schema_version: String,
    pub engine_version: String,
    pub system_version: String,
    pub live_onchain_sol: f64,
    pub is_scanner_paused: bool,
    pub panic_mode: bool,
    pub wallet_pubkey: String,
    pub initial_bankroll_sol: f64,
    pub current_bankroll_sol: f64,
    pub floating_pnl_sol: f64,
    pub live_nav_sol: f64,
    pub total_realized_pnl_sol: f64,
    pub total_trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub scratches: usize,
    pub effective_win_rate_pct: f64,
    pub rejections_count: u64,
    pub persistence_error_count: u64,
    pub active_positions: serde_json::Value,
    pub radar_candidates: serde_json::Value,
    pub closed_trades: serde_json::Value,
    pub recent_onchain_events: Vec<String>,
    pub risk_config: serde_json::Value,
    pub config: serde_json::Value,
    pub rejection_gauge: serde_json::Value,
    pub stage2_audit_log: serde_json::Value,
}
