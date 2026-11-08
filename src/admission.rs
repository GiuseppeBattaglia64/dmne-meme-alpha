//! DMNE QUANT // CANONICAL CANDIDATE ADMISSION SYSTEM
//! Full-suite semantic loop enforcing zero disconnected modules (Article 31.28).
//!
//! Evaluates candidate tokens across all 5 institutional pre-trade risk gates:
//! 1. Liquidity Floor: `liquidity_usd >= min_entry_liquidity_usd`
//! 2. Genesis Guard: Anti-sniper bundle audit & shared tip account detection
//! 3. Hawkes Point Process: Supercritical wash-trading & robotic clocking filter
//! 4. Orderflow CVD: Rolling taker pressure ratio & bearish absorption divergence
//! 5. Dev Cluster / Funder Graph: Sybil cluster ceiling & root funder HHI concentration
//!
//! Hot-path call graph invariant: Every filter actively governs candidate admission.
//! Zero phantom logic. Rejection telemetry strictly coupled to execution flow.

use serde::{Deserialize, Serialize};
use crate::config::EngineConfig;
use crate::funder_graph::{FunderGraphEvaluator, FunderGraphMetrics, TokenHolderRecord};
use crate::genesis_guard::{GenesisBundleProfile, GenesisGuardEvaluator, GenesisVerdict};
use crate::hawkes::HawkesEvaluator;
use crate::orderflow::{CvdDivergence, RollingCvdTracker};
use crate::radar::RejectionCategory;

/// Comprehensive candidate telemetry passed to the canonical admission engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateTelemetry {
    pub symbol: String,
    pub token_mint: String,
    pub pair_address: String,
    pub dex_id: String,
    pub price_sol: f64,
    pub liquidity_usd: f64,
    pub fdv_usd: f64,
    pub m5_price_change_pct: f64,
    pub h1_price_change_pct: f64,
    pub buy_sell_ratio: f64,
    pub m5_volume_usd: f64,
    pub m5_buys: u64,
    pub m5_sells: u64,
    pub arrival_timestamps: Vec<f64>,
    pub genesis_profile: Option<GenesisBundleProfile>,
    pub holder_records: Option<Vec<TokenHolderRecord>>,
}

/// Typed rejection reason returned when a candidate fails any admission gate
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RejectionReason {
    LowLiquidity {
        current_usd: f64,
        required_usd: f64,
    },
    GenesisGuard {
        reason: String,
        metric_str: String,
    },
    HawkesWash {
        branching_ratio: f64,
        detail: String,
    },
    OrderflowCvd {
        reason: String,
        metric_label: String,
    },
    DevCluster {
        reason: String,
        metric_label: String,
    },
}

impl RejectionReason {
    /// Maps the rejection reason to the corresponding dashboard gauge category
    pub fn category(&self) -> RejectionCategory {
        match self {
            RejectionReason::LowLiquidity { .. } => RejectionCategory::LowLiquidity,
            RejectionReason::GenesisGuard { .. } => RejectionCategory::DevCluster,
            RejectionReason::HawkesWash { .. } => RejectionCategory::HawkesWash,
            RejectionReason::OrderflowCvd { .. } => RejectionCategory::LowConviction,
            RejectionReason::DevCluster { .. } => RejectionCategory::DevCluster,
        }
    }

    /// Formats the primary human-readable rejection reason string
    pub fn reason_str(&self) -> String {
        match self {
            RejectionReason::LowLiquidity { current_usd, required_usd } => {
                format!("Liquidity ${:.0} below threshold ${:.0}", current_usd, required_usd)
            }
            RejectionReason::GenesisGuard { reason, .. } => reason.clone(),
            RejectionReason::HawkesWash { detail, .. } => detail.clone(),
            RejectionReason::OrderflowCvd { reason, .. } => reason.clone(),
            RejectionReason::DevCluster { reason, .. } => reason.clone(),
        }
    }

    /// Key metric string formatted for Stage 2 Deep Audit display
    pub fn metric_label(&self) -> String {
        match self {
            RejectionReason::LowLiquidity { current_usd, required_usd } => {
                format!("${:.1}k (<${:.0}k)", current_usd / 1000.0, required_usd / 1000.0)
            }
            RejectionReason::GenesisGuard { metric_str, .. } => metric_str.clone(),
            RejectionReason::HawkesWash { branching_ratio, .. } => {
                format!("eta: {:.2}", branching_ratio)
            }
            RejectionReason::OrderflowCvd { metric_label, .. } => metric_label.clone(),
            RejectionReason::DevCluster { metric_label, .. } => metric_label.clone(),
        }
    }

    /// Formatted verdict string for Stage 2 audit entry
    pub fn stage2_verdict(&self) -> String {
        match self {
            RejectionReason::LowLiquidity { .. } => "REJECT: LOW_LIQUIDITY".to_string(),
            RejectionReason::GenesisGuard { .. } => "REJECT: GENESIS_GUARD".to_string(),
            RejectionReason::HawkesWash { .. } => "REJECT: HAWKES_WASH".to_string(),
            RejectionReason::OrderflowCvd { reason, .. } => format!("REJECT: {}", reason),
            RejectionReason::DevCluster { .. } => "REJECT: DEV_CLUSTER".to_string(),
        }
    }
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reason_str())
    }
}

/// Canonical candidate evaluation predicate.
/// Strictly executes all 5 pre-trade gates in deterministic priority order.
/// Returns Ok(()) if the candidate clears all filters, or Err(RejectionReason) on first failure.
pub fn evaluate_candidate_admission(
    candidate: &CandidateTelemetry,
    cfg: &EngineConfig,
    funder_evaluator: Option<&FunderGraphEvaluator>,
) -> Result<(), RejectionReason> {
    // 1. Liquidity Floor Gate
    let min_liquidity = cfg.risk.min_entry_liquidity_usd;
    if candidate.liquidity_usd < min_liquidity {
        return Err(RejectionReason::LowLiquidity {
            current_usd: candidate.liquidity_usd,
            required_usd: min_liquidity,
        });
    }

    // 2. Genesis Bundle Protection & Anti-Sniper Cluster Detection Gate
    if cfg.genesis_guard.enabled {
        if let Some(ref profile) = candidate.genesis_profile {
            let genesis_evaluator = GenesisGuardEvaluator::new(cfg.genesis_guard.clone());
            if let GenesisVerdict::Rejected { reason, metric_str } = genesis_evaluator.evaluate(profile) {
                return Err(RejectionReason::GenesisGuard {
                    reason,
                    metric_str,
                });
            }
        }
    }

    // 3. Hawkes Point Process Wash-Trading Filter Gate
    if cfg.hawkes.enabled {
        let now_epoch_f64 = chrono::Utc::now().timestamp() as f64;
        let hawkes_metrics = HawkesEvaluator::evaluate(
            &candidate.arrival_timestamps,
            now_epoch_f64,
            candidate.m5_volume_usd,
            candidate.buy_sell_ratio,
            candidate.liquidity_usd,
            &cfg.hawkes,
        );
        if hawkes_metrics.is_wash_trading {
            return Err(RejectionReason::HawkesWash {
                branching_ratio: hawkes_metrics.branching_ratio,
                detail: hawkes_metrics.rejection_reason.unwrap_or_else(|| {
                    format!("HAWKES_WASH (eta: {:.2})", hawkes_metrics.branching_ratio)
                }),
            });
        }
    }

    // 4. Real-Time Orderflow CVD & Divergence Filter Gate
    if cfg.orderflow.enabled {
        let mut cvd_tracker = RollingCvdTracker::new(cfg.orderflow.cvd_window_secs);
        let now_epoch_f64 = chrono::Utc::now().timestamp() as f64;
        let sol_price_usd = cfg.sol_reference_price_usd.max(1.0);
        let volume_sol = (candidate.m5_volume_usd / sol_price_usd).max(0.0);
        cvd_tracker.record_snapshot_volume(
            now_epoch_f64,
            volume_sol,
            candidate.m5_buys,
            candidate.m5_sells,
        );
        let pressure = cvd_tracker.pressure_ratio();
        let divergence = cvd_tracker.evaluate_divergence(
            candidate.m5_price_change_pct,
            cfg.orderflow.bearish_absorption_threshold,
        );
        if divergence == CvdDivergence::BearishAbsorption {
            return Err(RejectionReason::OrderflowCvd {
                reason: "CVD_BEARISH_ABSORPTION".to_string(),
                metric_label: format!("CVD: {:.0}% (Abs)", pressure * 100.0),
            });
        } else if pressure < cfg.orderflow.min_entry_cvd_ratio {
            return Err(RejectionReason::OrderflowCvd {
                reason: "CVD_NEGATIVE_PRESSURE".to_string(),
                metric_label: format!(
                    "CVD: {:.0}% (<{:.0}%)",
                    pressure * 100.0,
                    cfg.orderflow.min_entry_cvd_ratio * 100.0
                ),
            });
        }
    }

    // 5. Dev Sybil Cluster & Deep Funder Graph HHI Gate
    if cfg.funder_graph.enabled {
        let funder_metrics: FunderGraphMetrics = match funder_evaluator {
            Some(fe) => {
                if let Some(ref holders) = candidate.holder_records {
                    fe.evaluate_holders(&candidate.token_mint, holders)
                } else {
                    fe.evaluate_token(&candidate.token_mint)
                }
            }
            None => {
                let default_evaluator = FunderGraphEvaluator::new(
                    cfg.funder_graph.clone(),
                    std::collections::HashMap::new(),
                );
                if let Some(ref holders) = candidate.holder_records {
                    default_evaluator.evaluate_holders(&candidate.token_mint, holders)
                } else {
                    default_evaluator.evaluate_token(&candidate.token_mint)
                }
            }
        };

        if funder_metrics.is_rejected {
            return Err(RejectionReason::DevCluster {
                reason: funder_metrics.rejection_reason.unwrap_or_else(|| "DEV_CLUSTER_REJECT".to_string()),
                metric_label: format!("Cluster: {:.1}%", funder_metrics.max_cluster_supply_pct),
            });
        }
    }

    Ok(())
}

/// Convenience wrapper evaluating candidate admission with default evaluator
#[allow(dead_code)]
pub fn evaluate_candidate(
    candidate: &CandidateTelemetry,
    cfg: &EngineConfig,
) -> Result<(), RejectionReason> {
    evaluate_candidate_admission(candidate, cfg, None)
}

/// Boolean admission helper returning true if candidate clears all gates
#[allow(dead_code)]
pub fn can_admit_candidate(
    candidate: &CandidateTelemetry,
    cfg: &EngineConfig,
    funder_evaluator: Option<&FunderGraphEvaluator>,
) -> bool {
    evaluate_candidate_admission(candidate, cfg, funder_evaluator).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::funder_graph::HolderFundingSource;

    fn make_test_candidate() -> CandidateTelemetry {
        CandidateTelemetry {
            symbol: "TEST".to_string(),
            token_mint: "TestTokenMintAddress1111111111111111111111111".to_string(),
            pair_address: "PairAddress111111111111111111111111111111111".to_string(),
            dex_id: "pumpfun".to_string(),
            price_sol: 0.00001,
            liquidity_usd: 40000.0,
            fdv_usd: 50000.0,
            m5_price_change_pct: 5.0,
            h1_price_change_pct: 12.0,
            buy_sell_ratio: 2.5,
            m5_volume_usd: 15000.0,
            m5_buys: 40,
            m5_sells: 10,
            arrival_timestamps: vec![10.0, 22.0, 35.0, 48.0, 61.0, 75.0, 90.0, 105.0],
            genesis_profile: Some(GenesisBundleProfile {
                mint: "TestTokenMintAddress1111111111111111111111111".to_string(),
                pool_creation_slot: 1000,
                slot0_buy_count: 1,
                slot0_supply_bought_pct: 5.0,
                shared_tip_detected: false,
                common_funding_source: None,
            }),
            holder_records: None,
        }
    }

    #[test]
    fn test_admission_clean_candidate_passes() {
        let candidate = make_test_candidate();
        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_ok(), "Expected clean candidate to pass, got: {:?}", res);
        assert!(can_admit_candidate(&candidate, &cfg, None));
    }

    #[test]
    fn test_admission_low_liquidity_rejection() {
        let mut candidate = make_test_candidate();
        candidate.liquidity_usd = 15000.0; // Below 35000.0 default
        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.category(), RejectionCategory::LowLiquidity);
        match err {
            RejectionReason::LowLiquidity { current_usd, required_usd } => {
                assert_eq!(current_usd, 15000.0);
                assert_eq!(required_usd, cfg.risk.min_entry_liquidity_usd);
            }
            _ => panic!("Expected LowLiquidity error"),
        }
        assert!(!can_admit_candidate(&candidate, &cfg, None));
    }

    #[test]
    fn test_admission_genesis_guard_rejection() {
        let mut candidate = make_test_candidate();
        candidate.genesis_profile = Some(GenesisBundleProfile {
            mint: candidate.token_mint.clone(),
            pool_creation_slot: 1000,
            slot0_buy_count: 5, // Exceeds ceiling 3
            slot0_supply_bought_pct: 30.0, // Exceeds ceiling 25.0%
            shared_tip_detected: true,
            common_funding_source: None,
        });
        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.category(), RejectionCategory::DevCluster);
        match err {
            RejectionReason::GenesisGuard { .. } => {}
            _ => panic!("Expected GenesisGuard error"),
        }
    }

    #[test]
    fn test_admission_orderflow_cvd_negative_pressure_rejection() {
        let mut candidate = make_test_candidate();
        // Heavy sells: 5 buys vs 50 sells
        candidate.m5_buys = 5;
        candidate.m5_sells = 50;
        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.category(), RejectionCategory::LowConviction);
        match err {
            RejectionReason::OrderflowCvd { .. } => {}
            _ => panic!("Expected OrderflowCvd error"),
        }
    }

    #[test]
    fn test_admission_orderflow_cvd_bearish_absorption_rejection() {
        let mut candidate = make_test_candidate();
        // Price rose but huge sell absorption
        candidate.m5_price_change_pct = 10.0;
        candidate.m5_buys = 10;
        candidate.m5_sells = 90; // pressure ratio ~ -0.80 <= -0.30
        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        match err {
            RejectionReason::OrderflowCvd { reason, .. } => {
                assert_eq!(reason, "CVD_BEARISH_ABSORPTION");
            }
            _ => panic!("Expected OrderflowCvd error"),
        }
    }

    #[test]
    fn test_admission_dev_cluster_funder_rejection() {
        let mut candidate = make_test_candidate();
        candidate.holder_records = Some(vec![
            TokenHolderRecord {
                holder_address: "Holder1".to_string(),
                supply_pct: 12.0,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: "CommonDevFunderRoot".to_string(),
                    timestamp_epoch: 1000,
                    amount_sol: 5.0,
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
            TokenHolderRecord {
                holder_address: "Holder2".to_string(),
                supply_pct: 10.0,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: "CommonDevFunderRoot".to_string(),
                    timestamp_epoch: 1050,
                    amount_sol: 5.0,
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
        ]);
        let cfg = EngineConfig::default();
        let funder_evaluator = FunderGraphEvaluator::new(cfg.funder_graph.clone(), HashMap::new());
        let res = evaluate_candidate_admission(&candidate, &cfg, Some(&funder_evaluator));
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.category(), RejectionCategory::DevCluster);
        match err {
            RejectionReason::DevCluster { .. } => {}
            _ => panic!("Expected DevCluster error"),
        }
    }
}

