// DMNE QUANT // COLD-PATH POST-TRADE FORENSIC AUTOPSY AGENT
// Post-Mortem Root Cause Classification & Counterfactual Gate Calibration

use serde::{Deserialize, Serialize};

/// Canonical configuration constants for offline forensic analysis (Zero Magic Numbers)
pub const DEFAULT_RAPID_DRAIN_HOLD_SECS: f64 = 120.0;
pub const DEFAULT_EXCESSIVE_SLIPPAGE_DRIFT_PCT: f64 = 5.0;
pub const DEFAULT_MAX_SLIPPAGE_TRIGGER_PCT: f64 = -25.0;
pub const DEFAULT_HARD_STOP_TRIGGER_PCT: f64 = -20.0;
pub const DEFAULT_EARLY_ABORT_TRIGGER_PCT: f64 = -10.0;
pub const DEFAULT_COUNTERFACTUAL_MIN_CVD_RATIO: f64 = 0.25;
pub const DEFAULT_COUNTERFACTUAL_MAX_HAWKES_ETA: f64 = 1.15;
pub const DEFAULT_COUNTERFACTUAL_MIN_LIQUIDITY_USD: f64 = 35_000.0;

/// Root cause categorization for post-mortem analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RootCauseCategory {
    RapidDrainRug,
    ExcessiveSlippage,
    StagnationDecay,
    OrganicStop,
    NormalExit,
}

impl RootCauseCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RapidDrainRug => "RAPID_DRAIN_RUG",
            Self::ExcessiveSlippage => "EXCESSIVE_SLIPPAGE",
            Self::StagnationDecay => "STAGNATION_DECAY",
            Self::OrganicStop => "ORGANIC_STOP",
            Self::NormalExit => "NORMAL_EXIT",
        }
    }
}

/// Settled trade representation ingested from settled_trades / forensic ledger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettledTradeRecord {
    pub trade_id: i32,
    pub symbol: String,
    pub mint: String,
    pub hold_duration_secs: f64,
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
    pub is_toxic: bool,
    pub entry_cvd_ratio: Option<f64>,
    pub entry_hawkes_eta: Option<f64>,
}

/// Strongly-typed configuration envelope for the forensic autopsy auditor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutopsyConfig {
    pub rapid_drain_max_hold_secs: f64,
    pub excessive_slippage_drift_pct: f64,
    pub max_slippage_trigger_pct: f64,
    pub hard_stop_trigger_pct: f64,
    pub early_abort_trigger_pct: f64,
    pub counterfactual_min_cvd_ratio: f64,
    pub counterfactual_max_hawkes_eta: f64,
    pub counterfactual_min_liquidity_usd: f64,
}

impl Default for AutopsyConfig {
    fn default() -> Self {
        Self {
            rapid_drain_max_hold_secs: DEFAULT_RAPID_DRAIN_HOLD_SECS,
            excessive_slippage_drift_pct: DEFAULT_EXCESSIVE_SLIPPAGE_DRIFT_PCT,
            max_slippage_trigger_pct: DEFAULT_MAX_SLIPPAGE_TRIGGER_PCT,
            hard_stop_trigger_pct: DEFAULT_HARD_STOP_TRIGGER_PCT,
            early_abort_trigger_pct: DEFAULT_EARLY_ABORT_TRIGGER_PCT,
            counterfactual_min_cvd_ratio: DEFAULT_COUNTERFACTUAL_MIN_CVD_RATIO,
            counterfactual_max_hawkes_eta: DEFAULT_COUNTERFACTUAL_MAX_HAWKES_ETA,
            counterfactual_min_liquidity_usd: DEFAULT_COUNTERFACTUAL_MIN_LIQUIDITY_USD,
        }
    }
}

/// Result of counterfactual pre-trade gate simulation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CounterfactualResult {
    pub would_avoid_trade: bool,
    pub avoided_loss_sol: f64,
    pub avoided_loss_pct: f64,
    pub blocking_gates: Vec<String>,
    pub recommendation: String,
}

/// Comprehensive post-trade forensic autopsy report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeAutopsyReport {
    pub trade_id: i32,
    pub symbol: String,
    pub mint: String,
    pub realized_pnl_pct: f64,
    pub realized_pnl_sol: f64,
    pub exit_rule: String,
    pub hold_duration_secs: f64,
    pub peak_mfe_pct: f64,
    pub max_mae_pct: f64,
    pub slippage_drift_pct: f64,
    pub entry_cvd_ratio: Option<f64>,
    pub entry_hawkes_eta: Option<f64>,
    pub root_cause_category: RootCauseCategory,
    pub counterfactual: CounterfactualResult,
}

/// Offline post-mortem auditor and gate calibrator
pub struct ColdPathAuditor {
    pub config: AutopsyConfig,
}

impl Default for ColdPathAuditor {
    fn default() -> Self {
        Self::new(AutopsyConfig::default())
    }
}

impl ColdPathAuditor {
    pub fn new(config: AutopsyConfig) -> Self {
        Self { config }
    }

    /// Computes trigger-to-fill slippage drift in percentage points
    pub fn compute_slippage_drift(&self, trade: &SettledTradeRecord) -> f64 {
        let trigger_pct = if trade.exit_rule.contains("Max Slippage") {
            self.config.max_slippage_trigger_pct
        } else if trade.exit_rule.contains("Stop-Loss") {
            self.config.hard_stop_trigger_pct
        } else if trade.exit_rule.contains("Early Abort") {
            self.config.early_abort_trigger_pct
        } else {
            trade.realized_pnl_pct
        };

        // Drift represents how much worse actual realized loss was compared to the trigger
        if trade.realized_pnl_pct < trigger_pct {
            (trigger_pct - trade.realized_pnl_pct).abs()
        } else {
            0.0
        }
    }

    /// Categorizes the root cause of trade performance
    pub fn classify_root_cause(&self, trade: &SettledTradeRecord, slippage_drift: f64) -> RootCauseCategory {
        let is_max_slippage = trade.exit_rule.contains("Max Slippage") 
            || trade.realized_pnl_pct <= self.config.max_slippage_trigger_pct;

        if is_max_slippage && trade.hold_duration_secs <= self.config.rapid_drain_max_hold_secs {
            RootCauseCategory::RapidDrainRug
        } else if slippage_drift > self.config.excessive_slippage_drift_pct {
            RootCauseCategory::ExcessiveSlippage
        } else if trade.exit_rule.contains("Stagnation") {
            RootCauseCategory::StagnationDecay
        } else if trade.exit_rule.contains("Stop-Loss") || trade.exit_rule.contains("Early Abort") {
            RootCauseCategory::OrganicStop
        } else {
            RootCauseCategory::NormalExit
        }
    }

    /// Evaluates whether stricter counterfactual gate thresholds would have averted the loss
    pub fn run_counterfactual_simulation(&self, trade: &SettledTradeRecord) -> CounterfactualResult {
        if trade.realized_pnl_sol >= 0.0 {
            return CounterfactualResult {
                would_avoid_trade: false,
                avoided_loss_sol: 0.0,
                avoided_loss_pct: 0.0,
                blocking_gates: Vec::new(),
                recommendation: "Trade was profitable; no gate tightening required.".to_string(),
            };
        }

        let mut blocking_gates = Vec::new();

        // 1. Counterfactual Liquidity Gate ($35,000 floor)
        if trade.liquidity_usd > 0.0 && trade.liquidity_usd < self.config.counterfactual_min_liquidity_usd {
            blocking_gates.push(format!(
                "LIQUIDITY_GATE (${:.0} < ${:.0})",
                trade.liquidity_usd, self.config.counterfactual_min_liquidity_usd
            ));
        }

        // 2. Counterfactual CVD Taker Pressure (+25% floor)
        if let Some(cvd) = trade.entry_cvd_ratio {
            if cvd < self.config.counterfactual_min_cvd_ratio {
                blocking_gates.push(format!(
                    "CVD_GATE ({:+.1}% < {:+.1}%)",
                    cvd * 100.0, self.config.counterfactual_min_cvd_ratio * 100.0
                ));
            }
        }

        // 3. Counterfactual Hawkes Point Process Self-Excitation (eta < 1.15)
        if let Some(eta) = trade.entry_hawkes_eta {
            if eta >= self.config.counterfactual_max_hawkes_eta {
                blocking_gates.push(format!(
                    "HAWKES_GATE (eta {:.2} >= {:.2})",
                    eta, self.config.counterfactual_max_hawkes_eta
                ));
            }
        }

        if !blocking_gates.is_empty() {
            let avoided_sol = trade.realized_pnl_sol.abs();
            let avoided_pct = trade.realized_pnl_pct.abs();
            CounterfactualResult {
                would_avoid_trade: true,
                avoided_loss_sol: avoided_sol,
                avoided_loss_pct: avoided_pct,
                blocking_gates: blocking_gates.clone(),
                recommendation: format!(
                    "Tightening [{}] would have completely averted this {:.4} SOL loss ({:+.1}%)",
                    blocking_gates.join(" | "), avoided_sol, trade.realized_pnl_pct
                ),
            }
        } else {
            CounterfactualResult {
                would_avoid_trade: false,
                avoided_loss_sol: 0.0,
                avoided_loss_pct: 0.0,
                blocking_gates: Vec::new(),
                recommendation: "Loss occurred within current calibrated boundaries; execution or organic market movement.".to_string(),
            }
        }
    }

    /// Performs full forensic autopsy on a settled trade record
    pub fn audit_settled_trade(&self, trade: &SettledTradeRecord) -> Option<TradeAutopsyReport> {
        let slippage_drift = self.compute_slippage_drift(trade);
        let root_cause = self.classify_root_cause(trade, slippage_drift);
        let counterfactual = self.run_counterfactual_simulation(trade);

        Some(TradeAutopsyReport {
            trade_id: trade.trade_id,
            symbol: trade.symbol.clone(),
            mint: trade.mint.clone(),
            realized_pnl_pct: trade.realized_pnl_pct,
            realized_pnl_sol: trade.realized_pnl_sol,
            exit_rule: trade.exit_rule.clone(),
            hold_duration_secs: trade.hold_duration_secs,
            peak_mfe_pct: trade.mfe_pnl_pct,
            max_mae_pct: trade.mae_pnl_pct,
            slippage_drift_pct: slippage_drift,
            entry_cvd_ratio: trade.entry_cvd_ratio,
            entry_hawkes_eta: trade.entry_hawkes_eta,
            root_cause_category: root_cause,
            counterfactual,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_record(
        trade_id: i32,
        symbol: &str,
        hold_secs: f64,
        pnl_pct: f64,
        pnl_sol: f64,
        exit_rule: &str,
        liquidity_usd: f64,
        cvd_ratio: Option<f64>,
        hawkes_eta: Option<f64>,
    ) -> SettledTradeRecord {
        SettledTradeRecord {
            trade_id,
            symbol: symbol.to_string(),
            mint: "TestMintAddress1111111111111111111111111111".to_string(),
            hold_duration_secs: hold_secs,
            size_sol: 0.045,
            entry_price_sol: 1.0e-6,
            peak_price_sol: 1.0e-6,
            trough_price_sol: 0.5e-6,
            exit_price_sol: 0.5e-6,
            mfe_pnl_pct: 0.0,
            mae_pnl_pct: pnl_pct,
            realized_pnl_pct: pnl_pct,
            realized_pnl_sol: pnl_sol,
            liquidity_usd,
            fdv_usd: 50_000.0,
            exit_rule: exit_rule.to_string(),
            is_toxic: true,
            entry_cvd_ratio: cvd_ratio,
            entry_hawkes_eta: hawkes_eta,
        }
    }

    #[test]
    fn test_classify_rapid_drain_rug() {
        let auditor = ColdPathAuditor::default();

        // 1. Rapid drain rug: Max Slippage within 102s (like Trade #116)
        let trade = create_test_record(
            116,
            "ㅤ",
            102.0,
            -61.73,
            -0.02778,
            "Max Slippage Loss (-25%)",
            13936.14,
            Some(-0.15),
            Some(1.20),
        );
        let autopsy = auditor.audit_settled_trade(&trade).expect("Audited report expected");
        assert_eq!(autopsy.root_cause_category, RootCauseCategory::RapidDrainRug);
        assert!((autopsy.slippage_drift_pct - 36.73).abs() < 1e-4);

        // 2. Excessive slippage: Hold > 120s with high drift (like Trade #118 BGATO)
        let trade_slow = create_test_record(
            118,
            "BGATO",
            562.0,
            -77.83,
            -0.02101,
            "Max Slippage Loss (-25%)",
            31443.82,
            Some(0.10),
            None,
        );
        let autopsy_slow = auditor.audit_settled_trade(&trade_slow).expect("Audited report expected");
        assert_eq!(autopsy_slow.root_cause_category, RootCauseCategory::ExcessiveSlippage);
        assert!((autopsy_slow.slippage_drift_pct - 52.83).abs() < 1e-4);

        // 3. Stagnation decay
        let trade_stagnant = create_test_record(
            99,
            "STAG",
            240.0,
            -3.5,
            -0.0015,
            "Stagnation TTL Exit",
            50000.0,
            Some(0.20),
            None,
        );
        let autopsy_stagnant = auditor.audit_settled_trade(&trade_stagnant).expect("Audited report expected");
        assert_eq!(autopsy_stagnant.root_cause_category, RootCauseCategory::StagnationDecay);
        assert_eq!(autopsy_stagnant.slippage_drift_pct, 0.0);
    }

    #[test]
    fn test_counterfactual_avoidance_logic() {
        let auditor = ColdPathAuditor::default();

        // Trade with low liquidity ($24,715 < $35,000) and weak CVD (0.125 < 0.25) like Puddgy #125
        let trade = create_test_record(
            125,
            "Puddgy",
            499.0,
            -29.76,
            -0.013394,
            "Max Slippage Loss (-25%)",
            24715.24,
            Some(0.125),
            Some(0.85),
        );

        let cf = auditor.run_counterfactual_simulation(&trade);
        assert!(cf.would_avoid_trade);
        assert_eq!(cf.avoided_loss_sol, 0.013394);
        assert_eq!(cf.avoided_loss_pct, 29.76);
        assert_eq!(cf.blocking_gates.len(), 2);
        assert!(cf.blocking_gates[0].contains("LIQUIDITY_GATE"));
        assert!(cf.blocking_gates[1].contains("CVD_GATE"));

        // Profitable trade should never be avoided
        let mut profitable = trade.clone();
        profitable.realized_pnl_sol = 0.015;
        profitable.realized_pnl_pct = 45.0;
        let cf_prof = auditor.run_counterfactual_simulation(&profitable);
        assert!(!cf_prof.would_avoid_trade);
    }
}

