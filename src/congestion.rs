//! Module 3: Solana Congestion & Network Regime Gate (AriSai Paradigm)
//!
//! Real-time network priority fee percentiles, compute unit price spikes,
//! and slot delta jitter estimator for pre-trade admission gating.
//!
//! Enforces zero magic numbers, strongly typed configuration, and zero lookahead.

use serde::{Deserialize, Serialize};

/// Network regime classification according to the AriSai Paradigm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CongestionRegime {
    /// Normal network state: standard priority fees, low block contention, clean fills.
    Normal,
    /// Elevated congestion: higher priority fees, moderate contention, trade allowed with caution.
    Elevated,
    /// High congestion / toxic spam surge: extreme fee spikes or severe block jitter.
    /// Pre-trade gate pauses admissions to protect capital from adverse selection and slippage.
    HighCongestion,
}

/// Strongly typed configuration for Solana Congestion & Network Regime Gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CongestionConfig {
    /// Maximum admissible swap interval jitter in milliseconds (default: 5200.0 ms)
    #[serde(default = "default_max_admissible_jitter_ms")]
    pub max_admissible_jitter_ms: f64,

    /// Maximum rolling audit density per minute (default: 200 audits/min)
    #[serde(default = "default_max_audit_density_per_min")]
    pub max_audit_density_per_min: usize,

    /// Maximum rolling rejection ratio before triggering spam surge pause (default: 0.985 / 98.5%)
    #[serde(default = "default_max_rejection_ratio")]
    pub max_rejection_ratio: f64,

    /// 50th percentile priority fee baseline in micro-lamports (default: 10,000)
    #[serde(default = "default_p50_fee_micro_lamports")]
    pub p50_fee_micro_lamports: u64,

    /// 75th percentile priority fee threshold in micro-lamports (default: 35,000)
    #[serde(default = "default_p75_fee_micro_lamports")]
    pub p75_fee_micro_lamports: u64,

    /// 90th percentile high-congestion fee cutoff in micro-lamports (default: 100,000)
    #[serde(default = "default_p90_fee_micro_lamports")]
    pub p90_fee_micro_lamports: u64,

    /// Rolling observation evaluation window in seconds (default: 120s)
    #[serde(default = "default_rolling_window_secs")]
    pub rolling_window_secs: u64,
}

fn default_max_admissible_jitter_ms() -> f64 { 5200.0 }
fn default_max_audit_density_per_min() -> usize { 200 }
fn default_max_rejection_ratio() -> f64 { 0.985 }
fn default_p50_fee_micro_lamports() -> u64 { 10_000 }
fn default_p75_fee_micro_lamports() -> u64 { 35_000 }
fn default_p90_fee_micro_lamports() -> u64 { 100_000 }
fn default_rolling_window_secs() -> u64 { 120 }

impl Default for CongestionConfig {
    fn default() -> Self {
        Self {
            max_admissible_jitter_ms: default_max_admissible_jitter_ms(),
            max_audit_density_per_min: default_max_audit_density_per_min(),
            max_rejection_ratio: default_max_rejection_ratio(),
            p50_fee_micro_lamports: default_p50_fee_micro_lamports(),
            p75_fee_micro_lamports: default_p75_fee_micro_lamports(),
            p90_fee_micro_lamports: default_p90_fee_micro_lamports(),
            rolling_window_secs: default_rolling_window_secs(),
        }
    }
}

/// Instantaneous or rolling network telemetry snapshot at timestamp t <= t_entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkTelemetrySnapshot {
    pub timestamp_epoch: f64,
    pub audit_density_per_min: usize,
    pub rejection_ratio: f64,
    pub swap_interval_jitter_ms: f64,
    pub priority_fee_micro_lamports: u64,
}

/// Gate decision returned by the Congestion Regime Evaluator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CongestionGateDecision {
    /// Admit candidate under Normal or Elevated regime
    Admit {
        regime: CongestionRegime,
        reason: String,
    },
    /// Pause admissions under HighCongestion regime
    PauseAdmission {
        regime: CongestionRegime,
        reason: String,
        observed_value: f64,
        threshold: f64,
    },
}

impl CongestionGateDecision {
    pub fn is_admitted(&self) -> bool {
        matches!(self, CongestionGateDecision::Admit { .. })
    }

    pub fn is_paused(&self) -> bool {
        matches!(self, CongestionGateDecision::PauseAdmission { .. })
    }
}

/// Evaluator that determines the network congestion regime strictly prior to candidate entry.
#[derive(Debug, Clone)]
pub struct CongestionRegimeEvaluator {
    pub config: CongestionConfig,
}

impl CongestionRegimeEvaluator {
    pub fn new(config: CongestionConfig) -> Self {
        Self { config }
    }

    /// Evaluates the network regime at candidate entry timestamp t.
    /// Strictly zero lookahead: metrics must be calculated from t <= t_entry.
    pub fn evaluate_snapshot(&self, snapshot: &NetworkTelemetrySnapshot) -> CongestionGateDecision {
        // 1. Extreme swap interval jitter check
        if snapshot.swap_interval_jitter_ms >= self.config.max_admissible_jitter_ms {
            return CongestionGateDecision::PauseAdmission {
                regime: CongestionRegime::HighCongestion,
                reason: "BLOCK_JITTER_SPIKE: Swap interval delta exceeds maximum admissible latency".to_string(),
                observed_value: snapshot.swap_interval_jitter_ms,
                threshold: self.config.max_admissible_jitter_ms,
            };
        }

        // 2. High audit burst density combined with high rejection ratio (spam wave)
        if snapshot.audit_density_per_min >= self.config.max_audit_density_per_min
            && snapshot.rejection_ratio >= self.config.max_rejection_ratio
        {
            return CongestionGateDecision::PauseAdmission {
                regime: CongestionRegime::HighCongestion,
                reason: "MEMPOOL_SPAM_BURST: Toxic rejection wave flooding pre-trade scanner".to_string(),
                observed_value: snapshot.audit_density_per_min as f64,
                threshold: self.config.max_audit_density_per_min as f64,
            };
        }

        // 3. Priority fee spike check (P90 threshold)
        if snapshot.priority_fee_micro_lamports >= self.config.p90_fee_micro_lamports {
            return CongestionGateDecision::PauseAdmission {
                regime: CongestionRegime::HighCongestion,
                reason: "PRIORITY_FEE_SURGE: Network compute unit price in top 10th percentile".to_string(),
                observed_value: snapshot.priority_fee_micro_lamports as f64,
                threshold: self.config.p90_fee_micro_lamports as f64,
            };
        }

        // 4. Elevated vs Normal classification
        if snapshot.priority_fee_micro_lamports >= self.config.p75_fee_micro_lamports
            || snapshot.swap_interval_jitter_ms >= (self.config.max_admissible_jitter_ms * 0.90)
        {
            return CongestionGateDecision::Admit {
                regime: CongestionRegime::Elevated,
                reason: "ELEVATED_LOAD: Clearance granted with dynamic priority pricing".to_string(),
            };
        }

        CongestionGateDecision::Admit {
            regime: CongestionRegime::Normal,
            reason: "NORMAL_REGIME: Clean mempool execution conditions".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_congestion_config_defaults() {
        let cfg = CongestionConfig::default();
        assert_eq!(cfg.max_admissible_jitter_ms, 5200.0);
        assert_eq!(cfg.max_audit_density_per_min, 200);
        assert_eq!(cfg.max_rejection_ratio, 0.985);
        assert_eq!(cfg.p90_fee_micro_lamports, 100_000);
    }

    #[test]
    fn test_normal_regime_admittance() {
        let evaluator = CongestionRegimeEvaluator::new(CongestionConfig::default());
        let snapshot = NetworkTelemetrySnapshot {
            timestamp_epoch: 1794000000.0,
            audit_density_per_min: 50,
            rejection_ratio: 0.90,
            swap_interval_jitter_ms: 4500.0,
            priority_fee_micro_lamports: 10_000,
        };

        let decision = evaluator.evaluate_snapshot(&snapshot);
        assert!(decision.is_admitted());
        if let CongestionGateDecision::Admit { regime, .. } = decision {
            assert_eq!(regime, CongestionRegime::Normal);
        } else {
            panic!("Expected Admit(Normal)");
        }
    }

    #[test]
    fn test_jitter_spike_triggers_pause() {
        let evaluator = CongestionRegimeEvaluator::new(CongestionConfig::default());
        let snapshot = NetworkTelemetrySnapshot {
            timestamp_epoch: 1794000000.0,
            audit_density_per_min: 50,
            rejection_ratio: 0.90,
            swap_interval_jitter_ms: 6800.0, // > 5200ms
            priority_fee_micro_lamports: 15_000,
        };

        let decision = evaluator.evaluate_snapshot(&snapshot);
        assert!(decision.is_paused());
        if let CongestionGateDecision::PauseAdmission { regime, reason, .. } = decision {
            assert_eq!(regime, CongestionRegime::HighCongestion);
            assert!(reason.contains("BLOCK_JITTER_SPIKE"));
        } else {
            panic!("Expected PauseAdmission(HighCongestion)");
        }
    }

    #[test]
    fn test_spam_burst_triggers_pause() {
        let evaluator = CongestionRegimeEvaluator::new(CongestionConfig::default());
        let snapshot = NetworkTelemetrySnapshot {
            timestamp_epoch: 1794000000.0,
            audit_density_per_min: 240, // > 200
            rejection_ratio: 0.992,      // > 0.985
            swap_interval_jitter_ms: 4600.0,
            priority_fee_micro_lamports: 20_000,
        };

        let decision = evaluator.evaluate_snapshot(&snapshot);
        assert!(decision.is_paused());
        if let CongestionGateDecision::PauseAdmission { regime, reason, .. } = decision {
            assert_eq!(regime, CongestionRegime::HighCongestion);
            assert!(reason.contains("MEMPOOL_SPAM_BURST"));
        } else {
            panic!("Expected PauseAdmission(HighCongestion)");
        }
    }

    #[test]
    fn test_priority_fee_surge_triggers_pause() {
        let evaluator = CongestionRegimeEvaluator::new(CongestionConfig::default());
        let snapshot = NetworkTelemetrySnapshot {
            timestamp_epoch: 1794000000.0,
            audit_density_per_min: 80,
            rejection_ratio: 0.92,
            swap_interval_jitter_ms: 4700.0,
            priority_fee_micro_lamports: 125_000, // > 100_000 (P90)
        };

        let decision = evaluator.evaluate_snapshot(&snapshot);
        assert!(decision.is_paused());
        if let CongestionGateDecision::PauseAdmission { regime, reason, .. } = decision {
            assert_eq!(regime, CongestionRegime::HighCongestion);
            assert!(reason.contains("PRIORITY_FEE_SURGE"));
        } else {
            panic!("Expected PauseAdmission(HighCongestion)");
        }
    }
}

