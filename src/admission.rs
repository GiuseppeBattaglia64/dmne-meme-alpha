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
#[allow(unused_imports)]
pub use crate::radar::Stage2Rejection;

/// Comprehensive candidate telemetry passed to the canonical admission engine
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenSecurityProfile {
    pub mint_authority_revoked: bool,
    pub freeze_authority_revoked: bool,
    pub has_transfer_hook: bool,
    pub has_permanent_delegate: bool,
    pub transfer_fee_bps: u16,
}

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
    #[serde(default)]
    pub holder_fetch_failed: bool,
    #[serde(default)]
    pub vpin_calculator: Option<crate::orderflow::VpinCalculator>,
    #[serde(default)]
    pub caller_msg_timestamp_utc: Option<i64>,
    #[serde(default)]
    pub pool_creation_timestamp_utc: Option<i64>,
    #[serde(default)]
    pub caller_delta_seconds: Option<i64>,
    #[serde(default)]
    pub quote_mint: Option<String>,
    #[serde(default)]
    pub bonding_curve_complete: Option<bool>,
    #[serde(default)]
    pub wash_volume_eta: Option<f64>,
    #[serde(default)]
    pub protocol_fee_ratio_phi: Option<f64>,
    #[serde(default)]
    pub peak_liquidity_usd: Option<f64>,
    #[serde(default)]
    pub sybil_ratio_rho: Option<f64>,
    #[serde(default)]
    pub transfer_edges: Option<Vec<crate::funder_graph::TransferEdge>>,
    #[serde(default)]
    pub deployer_address: Option<String>,
    #[serde(default)]
    pub token_security: Option<TokenSecurityProfile>,
    #[serde(default)]
    pub holder_usd_values: Option<Vec<f64>>,
}

/// Typed rejection reason returned when a candidate fails any admission gate
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RejectionReason {
    LateCallerDistribution {
        delta_seconds: i64,
        max_seconds: i64,
    },
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
            RejectionReason::LateCallerDistribution { .. } => RejectionCategory::LowConviction,
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
            RejectionReason::LateCallerDistribution { delta_seconds, max_seconds } => {
                format!("Late caller distribution ({}s > {}s)", delta_seconds, max_seconds)
            }
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
            RejectionReason::LateCallerDistribution { delta_seconds, max_seconds } => {
                format!("delta: {}s (>{}s)", delta_seconds, max_seconds)
            }
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
            RejectionReason::LateCallerDistribution { .. } => "REJECT: LATE_CALLER_DISTRIBUTION".to_string(),
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
    // 0. Gate 0: Late Caller Distribution Filter
    // Delta t = t_caller_msg - t_pool_creation
    // If the token originates from a social signal and Delta t > 60s, reject immediately with REJECT: LATE_CALLER_DISTRIBUTION
    let max_delta = cfg.risk.caller_max_delta_seconds;
    let delta_opt = if let (Some(t_caller), Some(t_pool)) = (candidate.caller_msg_timestamp_utc, candidate.pool_creation_timestamp_utc) {
        Some(t_caller - t_pool)
    } else {
        candidate.caller_delta_seconds
    };
    if let Some(delta) = delta_opt {
        if delta > max_delta {
            return Err(RejectionReason::LateCallerDistribution {
                delta_seconds: delta,
                max_seconds: max_delta,
            });
        }
    }

    // 1. Canonical Hard-Quote & Post-Migration Graduation Gate + Liquidity Floor
    if let Some(ref qm) = candidate.quote_mint {
        if !cfg.gates.canonical_quote_mints.is_empty() && !cfg.gates.canonical_quote_mints.iter().any(|c| c == qm) {
            return Err(RejectionReason::LowLiquidity {
                current_usd: candidate.liquidity_usd,
                required_usd: cfg.risk.min_entry_liquidity_usd,
            });
        }
    }
    if candidate.dex_id == "pumpfun" && candidate.bonding_curve_complete == Some(false) {
        return Err(RejectionReason::LowLiquidity {
            current_usd: candidate.liquidity_usd,
            required_usd: cfg.risk.min_entry_liquidity_usd,
        });
    }
    if let Some(peak_liq) = candidate.peak_liquidity_usd {
        if peak_liq > 0.0 && (candidate.liquidity_usd / peak_liq) < cfg.gates.min_liquidity_retention_ratio {
            return Err(RejectionReason::LowLiquidity {
                current_usd: candidate.liquidity_usd,
                required_usd: peak_liq * cfg.gates.min_liquidity_retention_ratio,
            });
        }
    }

    // Standard radar candidates: $32,000 floor (tokens between $25k-$32k show excessive slippage on sub-second dumps)
    // Social caller signals with early pool delta (<= 30s) may retain the $25,000 floor
    let is_early_social = match delta_opt {
        Some(delta) if delta <= 30 => true,
        _ => false,
    };
    let min_entry_liquidity = if is_early_social {
        25000.0f64.min(cfg.risk.min_entry_liquidity_usd)
    } else {
        cfg.risk.min_entry_liquidity_usd.max(32000.0)
    };
    if candidate.liquidity_usd < min_entry_liquidity {
        return Err(RejectionReason::LowLiquidity {
            current_usd: candidate.liquidity_usd,
            required_usd: min_entry_liquidity,
        });
    }

    // 1b. SPL Authority & Token-2022 Extension Safety Gate + Dynamic Deployer Blacklist
    if let Some(ref sec) = candidate.token_security {
        if !sec.mint_authority_revoked || !sec.freeze_authority_revoked {
            return Err(RejectionReason::DevCluster {
                reason: "SPL_AUTHORITY_NOT_REVOKED".to_string(),
                metric_label: "Auth: Active".to_string(),
            });
        }
        if sec.has_transfer_hook || sec.has_permanent_delegate || sec.transfer_fee_bps > 0 {
            return Err(RejectionReason::DevCluster {
                reason: "TOKEN2022_UNSAFE_EXTENSION".to_string(),
                metric_label: format!("FeeBps: {}", sec.transfer_fee_bps),
            });
        }
    }
    if let Some(ref deployer) = candidate.deployer_address {
        let store = crate::funder_graph::DeployerBlacklistStore::load_from_path(&cfg.gates.deployer_blacklist_path);
        if store.is_blacklisted(deployer) {
            return Err(RejectionReason::DevCluster {
                reason: "SERIAL_DEPLOYER_BLACKLISTED".to_string(),
                metric_label: "Deployer: Banned".to_string(),
            });
        }
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

    // 3. Hawkes Point Process Wash-Trading Filter Gate + Intra-Block Wash & Protocol-Fee Consistency
    if let Some(eta) = candidate.wash_volume_eta {
        if eta > cfg.gates.max_wash_volume_eta {
            return Err(RejectionReason::HawkesWash {
                branching_ratio: eta,
                detail: format!("WASH_VOLUME_ETA_EXCEEDED ({:.2} > {:.2})", eta, cfg.gates.max_wash_volume_eta),
            });
        }
    }
    if let Some(phi) = candidate.protocol_fee_ratio_phi {
        if phi < cfg.gates.min_protocol_fee_ratio_phi {
            return Err(RejectionReason::HawkesWash {
                branching_ratio: phi,
                detail: format!("PROTOCOL_FEE_INCONSISTENCY ({:.2} < {:.2})", phi, cfg.gates.min_protocol_fee_ratio_phi),
            });
        }
    }
    if cfg.hawkes.enabled {
        if candidate.arrival_timestamps.len() < 5 {
            return Err(RejectionReason::HawkesWash {
                branching_ratio: 0.0,
                detail: "BUFFER_COLD_UNDER_5_EVENTS".to_string(),
            });
        }
        let now_epoch_f64 = chrono::Utc::now().timestamp() as f64;
        let hawkes_metrics = HawkesEvaluator::evaluate(
            &candidate.arrival_timestamps,
            now_epoch_f64,
            candidate.m5_volume_usd,
            candidate.buy_sell_ratio,
            candidate.liquidity_usd,
            &cfg.hawkes,
        );
        if hawkes_metrics.is_wash_trading || hawkes_metrics.branching_ratio >= 1.15 {
            return Err(RejectionReason::HawkesWash {
                branching_ratio: hawkes_metrics.branching_ratio,
                detail: hawkes_metrics.rejection_reason.unwrap_or_else(|| {
                    format!("HAWKES_WASH (eta: {:.2} >= 1.15)", hawkes_metrics.branching_ratio)
                }),
            });
        }
    }

    // 4. Real-Time Orderflow CVD & Divergence Filter Gate
    if cfg.orderflow.enabled {
        // VPIN Volume-Synchronized Probability of Toxicity Gate
        if let Some(ref vpin_calc) = candidate.vpin_calculator {
            if vpin_calc.is_toxic_sell_dominated() {
                let vpin = vpin_calc.compute_vpin().unwrap_or(0.0);
                return Err(RejectionReason::OrderflowCvd {
                    reason: format!("HIGH_VPIN_TOXICITY_{:.2}", vpin),
                    metric_label: format!("VPIN: {:.2} (Toxic)", vpin),
                });
            }
        }

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
        let candidate_holders = candidate.holder_records.as_deref();
        let cached_holders = funder_evaluator.and_then(|fe| fe.cache.get(&candidate.token_mint).map(|v| v.as_slice()));
        let holders = match (candidate_holders, cached_holders) {
            (Some(h), _) if h.len() >= 5 => h,
            (_, Some(h)) if h.len() >= 5 => h,
            _ => {
                if candidate.holder_fetch_failed {
                    return Err(RejectionReason::DevCluster {
                        reason: "RPC_HOLDER_FETCH_FAILED".to_string(),
                        metric_label: "RPC: 429/Timeout".to_string(),
                    });
                }
                return Err(RejectionReason::DevCluster {
                    reason: "HOLDERS_UNDER_5".to_string(),
                    metric_label: "Holders: <5".to_string(),
                });
            }
        };

        // Sniper Profit Overhang Ceiling Check
        let price_multiple = (1.0 + (candidate.m5_price_change_pct / 100.0))
            .max(1.0 + (candidate.h1_price_change_pct / 100.0))
            .max(1.0);

        let (is_overhang, top5_pct) = crate::funder_graph::evaluate_sniper_overhang(holders, price_multiple);
        if is_overhang {
            return Err(RejectionReason::DevCluster {
                reason: "SNIPER_OVERHANG_UNREALIZED_PROFIT_EXCESS".to_string(),
                metric_label: format!("Overhang: {:.1}% (>18% @ {:.1}x)", top5_pct, price_multiple),
            });
        }

        // 2.0% Deployer Supply Overhang Ceiling Gate
        let top_dev_holding = holders
            .iter()
            .filter(|h| !h.is_liquidity_pool)
            .map(|h| h.supply_pct)
            .fold(0.0f64, f64::max);
        if top_dev_holding >= 2.0 && price_multiple >= 3.0 {
            return Err(RejectionReason::DevCluster {
                reason: "DEPLOYER_SUPPLY_OVERHANG_2PCT".to_string(),
                metric_label: format!("Dev: {:.1}% (>=2.0% @ {:.1}x)", top_dev_holding, price_multiple),
            });
        }

        // Shannon Entropy Dispersion Gate
        if let Err(reason) = crate::funder_graph::evaluate_holder_entropy(holders) {
            let (_, h_norm) = crate::funder_graph::calculate_shannon_entropy(holders, 20);
            return Err(RejectionReason::DevCluster {
                reason,
                metric_label: format!("H_norm: {:.2}", h_norm),
            });
        }

        let funder_metrics: FunderGraphMetrics = match funder_evaluator {
            Some(fe) => fe.evaluate_holders(&candidate.token_mint, holders),
            None => {
                let default_evaluator = FunderGraphEvaluator::new(
                    cfg.funder_graph.clone(),
                    std::collections::HashMap::new(),
                );
                default_evaluator.evaluate_holders(&candidate.token_mint, holders)
            }
        };

        if funder_metrics.is_rejected || funder_metrics.is_sybil_cluster || funder_metrics.is_concentrated_hhi {
            return Err(RejectionReason::DevCluster {
                reason: funder_metrics.rejection_reason.unwrap_or_else(|| "DEV_CLUSTER_REJECT".to_string()),
                metric_label: format!("Cluster: {:.1}%", funder_metrics.max_cluster_supply_pct),
            });
        }

        // Top-N Concentration HHI_top20 check over non-pool holders
        let non_pool_pcts: Vec<f64> = holders
            .iter()
            .filter(|h| !h.is_liquidity_pool && h.supply_pct > 0.0)
            .map(|h| h.supply_pct)
            .collect();
        let pct_sum: f64 = non_pool_pcts.iter().sum();
        if pct_sum > 0.0 {
            let hhi_top20: f64 = non_pool_pcts.iter().map(|&p| (p / pct_sum).powi(2)).sum();
            if hhi_top20 > cfg.gates.max_top20_hhi {
                return Err(RejectionReason::DevCluster {
                    reason: "TOP20_HHI_EXCEEDED".to_string(),
                    metric_label: format!("HHI_top20: {:.2}", hhi_top20),
                });
            }
        }

        // Sybil ratio rho <= 0.25
        if let Some(rho) = candidate.sybil_ratio_rho {
            if rho > cfg.gates.max_sybil_ratio_rho {
                return Err(RejectionReason::DevCluster {
                    reason: "SYBIL_RATIO_RHO_EXCEEDED".to_string(),
                    metric_label: format!("rho: {:.2}", rho),
                });
            }
        }

        // SPL Transfer-Graph Connected-Component Cap (C_max <= 0.15) & Zero-Swap Airdrop Filter
        if let Some(ref edges) = candidate.transfer_edges {
            let tg = crate::funder_graph::compute_transfer_connected_components(edges, holders);
            if tg.zero_swap_airdrop_cluster {
                return Err(RejectionReason::DevCluster {
                    reason: "ZERO_SWAP_AIRDROP_CLUSTER".to_string(),
                    metric_label: format!("C_max: {:.2}", tg.max_component_share),
                });
            }
            if tg.max_component_share > cfg.gates.max_transfer_component_cap {
                return Err(RejectionReason::DevCluster {
                    reason: "TRANSFER_COMPONENT_CAP_EXCEEDED".to_string(),
                    metric_label: format!("C_max: {:.2}", tg.max_component_share),
                });
            }
        }

        // Dust-adjusted active holder count (N_holders,eff >= $25)
        if let Some(ref usd_vals) = candidate.holder_usd_values {
            let eff_count = usd_vals
                .iter()
                .filter(|&&v| v >= cfg.gates.min_effective_holder_usd)
                .count();
            if eff_count < cfg.gates.min_effective_holders_count {
                return Err(RejectionReason::DevCluster {
                    reason: "DUST_ADJUSTED_HOLDERS_LOW".to_string(),
                    metric_label: format!("EffHolders: {}", eff_count),
                });
            }
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
        let now = chrono::Utc::now().timestamp() as f64;
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
            arrival_timestamps: vec![now - 70.0, now - 55.0, now - 40.0, now - 25.0, now - 10.0],
            genesis_profile: Some(GenesisBundleProfile {
                mint: "TestTokenMintAddress1111111111111111111111111".to_string(),
                pool_creation_slot: 1000,
                slot0_buy_count: 1,
                slot0_supply_bought_pct: 5.0,
                shared_tip_detected: false,
                common_funding_source: None,
            }),
            holder_records: Some(vec![
                TokenHolderRecord {
                    holder_address: "Holder1".to_string(),
                    supply_pct: 3.0,
                    is_liquidity_pool: false,
                    funding_source: None,
                },
                TokenHolderRecord {
                    holder_address: "Holder2".to_string(),
                    supply_pct: 2.5,
                    is_liquidity_pool: false,
                    funding_source: None,
                },
                TokenHolderRecord {
                    holder_address: "Holder3".to_string(),
                    supply_pct: 2.0,
                    is_liquidity_pool: false,
                    funding_source: None,
                },
                TokenHolderRecord {
                    holder_address: "Holder4".to_string(),
                    supply_pct: 1.5,
                    is_liquidity_pool: false,
                    funding_source: None,
                },
                TokenHolderRecord {
                    holder_address: "Holder5".to_string(),
                    supply_pct: 1.0,
                    is_liquidity_pool: false,
                    funding_source: None,
                },
            ]),
            holder_fetch_failed: false,
            vpin_calculator: None,
            caller_msg_timestamp_utc: None,
            pool_creation_timestamp_utc: None,
            caller_delta_seconds: None,
            quote_mint: None,
            bonding_curve_complete: None,
            wash_volume_eta: None,
            protocol_fee_ratio_phi: None,
            peak_liquidity_usd: None,
            sybil_ratio_rho: None,
            transfer_edges: None,
            deployer_address: None,
            token_security: None,
            holder_usd_values: None,
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
        candidate.liquidity_usd = 15000.0; // Below 32000.0 default
        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.category(), RejectionCategory::LowLiquidity);
        match err {
            RejectionReason::LowLiquidity { current_usd, required_usd } => {
                assert_eq!(current_usd, 15000.0);
                assert_eq!(required_usd, 32000.0);
            }
            _ => panic!("Expected LowLiquidity error"),
        }
        assert!(!can_admit_candidate(&candidate, &cfg, None));
    }

    #[test]
    fn test_gate1_standard_vs_social_early_delta_liquidity_buffer() {
        let cfg = EngineConfig::default();

        // 1. Standard radar candidate at $28,000 -> Rejected (< $32k floor)
        let mut standard_cand = make_test_candidate();
        standard_cand.liquidity_usd = 28000.0;
        let res_std = evaluate_candidate_admission(&standard_cand, &cfg, None);
        assert!(res_std.is_err());
        match res_std.unwrap_err() {
            RejectionReason::LowLiquidity { required_usd, .. } => assert_eq!(required_usd, 32000.0),
            _ => panic!("Expected LowLiquidity error"),
        }

        // 2. Early social caller signal (delta 20s <= 30s) at $28,000 -> Passed (retains $25k floor)
        let mut early_social = make_test_candidate();
        early_social.liquidity_usd = 28000.0;
        early_social.caller_delta_seconds = Some(20);
        let res_early = evaluate_candidate_admission(&early_social, &cfg, None);
        assert!(res_early.is_ok(), "Early social candidate at $28k should pass under $25k floor");

        // 3. Late social caller signal (delta 45s > 30s) at $28,000 -> Rejected (requires $32k floor)
        let mut late_social = make_test_candidate();
        late_social.liquidity_usd = 28000.0;
        late_social.caller_delta_seconds = Some(45);
        let res_late = evaluate_candidate_admission(&late_social, &cfg, None);
        assert!(res_late.is_err());
        match res_late.unwrap_err() {
            RejectionReason::LowLiquidity { required_usd, .. } => assert_eq!(required_usd, 32000.0),
            _ => panic!("Expected LowLiquidity error"),
        }
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
            TokenHolderRecord {
                holder_address: "Holder3".to_string(),
                supply_pct: 1.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "Holder4".to_string(),
                supply_pct: 1.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "Holder5".to_string(),
                supply_pct: 1.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
        ]);
        let cfg = EngineConfig::default();
        let funder_evaluator = FunderGraphEvaluator::new(cfg.funder_graph.clone(), HashMap::new());
        let res = evaluate_candidate_admission(&candidate, &cfg, Some(&funder_evaluator));
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.category(), RejectionCategory::DevCluster);
        match err {
            RejectionReason::DevCluster { reason, .. } => {
                assert!(reason.contains("SYBIL_DEV_CLUSTER"));
            }
            _ => panic!("Expected DevCluster error"),
        }
    }

    #[test]
    fn test_admission_hawkes_cold_buffer_rejection() {
        let mut candidate = make_test_candidate();
        candidate.arrival_timestamps = vec![1.0, 2.0, 3.0]; // < 5 events
        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        match err {
            RejectionReason::HawkesWash { detail, .. } => {
                assert_eq!(detail, "BUFFER_COLD_UNDER_5_EVENTS");
            }
            _ => panic!("Expected HawkesWash BUFFER_COLD_UNDER_5_EVENTS, got: {:?}", err),
        }
    }

    #[test]
    fn test_admission_rpc_holder_data_unavailable_rejection() {
        let mut candidate = make_test_candidate();
        candidate.holder_records = None; // RPC fetch failed (429 / timeout)
        candidate.holder_fetch_failed = true;
        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        match err {
            RejectionReason::DevCluster { reason, metric_label } => {
                assert_eq!(reason, "RPC_HOLDER_FETCH_FAILED");
                assert_eq!(metric_label, "RPC: 429/Timeout");
            }
            _ => panic!("Expected DevCluster RPC_HOLDER_FETCH_FAILED, got: {:?}", err),
        }

        // Also test genuinely < 5 non-pool holders
        candidate.holder_fetch_failed = false;
        candidate.holder_records = Some(vec![
            TokenHolderRecord {
                holder_address: "H1".to_string(),
                supply_pct: 1.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
        ]);
        let res2 = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res2.is_err());
        match res2.unwrap_err() {
            RejectionReason::DevCluster { reason, metric_label } => {
                assert_eq!(reason, "HOLDERS_UNDER_5");
                assert_eq!(metric_label, "Holders: <5");
            }
            _ => panic!("Expected HOLDERS_UNDER_5"),
        }
    }

    #[test]
    fn test_admission_dex_pool_vault_excluded_vs_non_dex_cluster() {
        let mut cfg = EngineConfig::default();
        cfg.genesis_guard.enabled = false;
        cfg.hawkes.enabled = false;
        cfg.orderflow.enabled = false;

        // (a) Graduated PumpSwap/Raydium pool with a 20.7% DEX-owned vault (is_liquidity_pool: true)
        // and dispersed human holders (<= 18% combined, each < 2.0%) passes Gate 5.
        let mut pool_candidate = make_test_candidate();
        pool_candidate.dex_id = "pumpswap".to_string();
        pool_candidate.holder_records = Some(vec![
            TokenHolderRecord {
                holder_address: "PoolVault111".to_string(),
                supply_pct: 20.7,
                is_liquidity_pool: true,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "Human1".to_string(),
                supply_pct: 1.8,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "Human2".to_string(),
                supply_pct: 1.6,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "Human3".to_string(),
                supply_pct: 1.5,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "Human4".to_string(),
                supply_pct: 1.4,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "Human5".to_string(),
                supply_pct: 1.2,
                is_liquidity_pool: false,
                funding_source: None,
            },
        ]);
        let res_pass = evaluate_candidate_admission(&pool_candidate, &cfg, None);
        assert!(
            res_pass.is_ok(),
            "Graduated pool with 20.7% DEX vault and dispersed human holders should pass Gate 5, got: {:?}",
            res_pass
        );

        // (b) A 20.7% non-DEX wallet cluster (is_liquidity_pool: false) MUST still be rejected by Gate 5.
        let mut cluster_candidate = make_test_candidate();
        cluster_candidate.holder_records = Some(vec![
            TokenHolderRecord {
                holder_address: "Sybil1".to_string(),
                supply_pct: 5.5,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: "SharedFunderXYZ".to_string(),
                    amount_sol: 1.0,
                    timestamp_epoch: 1000,
                    hop_depth: 1,
                    tx_signature: Some("sig1".to_string()),
                }),
            },
            TokenHolderRecord {
                holder_address: "Sybil2".to_string(),
                supply_pct: 5.2,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: "SharedFunderXYZ".to_string(),
                    amount_sol: 1.0,
                    timestamp_epoch: 1010,
                    hop_depth: 1,
                    tx_signature: Some("sig2".to_string()),
                }),
            },
            TokenHolderRecord {
                holder_address: "Sybil3".to_string(),
                supply_pct: 5.0,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: "SharedFunderXYZ".to_string(),
                    amount_sol: 1.0,
                    timestamp_epoch: 1020,
                    hop_depth: 1,
                    tx_signature: Some("sig3".to_string()),
                }),
            },
            TokenHolderRecord {
                holder_address: "Sybil4".to_string(),
                supply_pct: 5.0,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: "SharedFunderXYZ".to_string(),
                    amount_sol: 1.0,
                    timestamp_epoch: 1030,
                    hop_depth: 1,
                    tx_signature: Some("sig4".to_string()),
                }),
            },
            TokenHolderRecord {
                holder_address: "Human5".to_string(),
                supply_pct: 1.2,
                is_liquidity_pool: false,
                funding_source: None,
            },
        ]);
        let res_reject = evaluate_candidate_admission(&cluster_candidate, &cfg, None);
        assert!(
            matches!(res_reject, Err(RejectionReason::DevCluster { .. })),
            "20.7% non-DEX wallet cluster must be rejected by Gate 5, got: {:?}",
            res_reject
        );
    }

    #[test]
    fn test_admission_sniper_overhang_rejection() {
        let mut candidate = make_test_candidate();
        // Top 5 non-pool holders hold 20.0% (> 18.0%)
        candidate.holder_records = Some(vec![
            TokenHolderRecord {
                holder_address: "H1".to_string(),
                supply_pct: 5.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "H2".to_string(),
                supply_pct: 5.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "H3".to_string(),
                supply_pct: 4.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "H4".to_string(),
                supply_pct: 3.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "H5".to_string(),
                supply_pct: 3.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
        ]);
        // Price surge of +260% (price_multiple = 3.60 >= 3.50)
        candidate.m5_price_change_pct = 260.0;
        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        match err {
            RejectionReason::DevCluster { reason, metric_label } => {
                assert_eq!(reason, "SNIPER_OVERHANG_UNREALIZED_PROFIT_EXCESS");
                assert!(metric_label.contains("Overhang: 20.0%"));
            }
            _ => panic!("Expected DevCluster SNIPER_OVERHANG_UNREALIZED_PROFIT_EXCESS, got: {:?}", err),
        }
    }

    #[test]
    fn test_admission_shannon_entropy_rejection() {
        let mut candidate = make_test_candidate();
        // 1 whale with 90% and 10 sybils with 1%
        let mut holders = vec![
            TokenHolderRecord {
                holder_address: "LP".to_string(),
                supply_pct: 10.0,
                is_liquidity_pool: true,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "WHALE".to_string(),
                supply_pct: 90.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
        ];
        for i in 0..10 {
            holders.push(TokenHolderRecord {
                holder_address: format!("SYBIL_{}", i),
                supply_pct: 1.0,
                is_liquidity_pool: false,
                funding_source: None,
            });
        }
        candidate.holder_records = Some(holders);
        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        match err {
            RejectionReason::DevCluster { reason, metric_label } => {
                assert!(reason.contains("LOW_SHANNON_ENTROPY"));
                assert!(metric_label.contains("H_norm"));
            }
            _ => panic!("Expected DevCluster error for low Shannon entropy, got: {:?}", err),
        }
    }

    #[test]
    fn test_admission_deployer_supply_overhang_2pct_rejection() {
        let mut candidate = make_test_candidate();
        // Set price multiple >= 3.0 (e.g. +220% gain -> 3.2x)
        candidate.m5_price_change_pct = 220.0;
        // Deployer holds >= 2.0% (e.g. 2.5%)
        candidate.holder_records = Some(vec![
            TokenHolderRecord {
                holder_address: "AMM_POOL".to_string(),
                supply_pct: 80.0,
                is_liquidity_pool: true,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "DEV_WALLET".to_string(),
                supply_pct: 2.5,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "H2".to_string(),
                supply_pct: 1.5,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "H3".to_string(),
                supply_pct: 1.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "H4".to_string(),
                supply_pct: 0.8,
                is_liquidity_pool: false,
                funding_source: None,
            },
        ]);
        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.category(), RejectionCategory::DevCluster);
        match err {
            RejectionReason::DevCluster { reason, metric_label } => {
                assert_eq!(reason, "DEPLOYER_SUPPLY_OVERHANG_2PCT");
                assert!(metric_label.contains("Dev: 2.5%"));
                assert!(metric_label.contains(">=2.0%"));
            }
            _ => panic!("Expected DevCluster DEPLOYER_SUPPLY_OVERHANG_2PCT, got: {:?}", err),
        }
    }

    #[test]
    fn test_admission_vpin_toxicity_rejection() {
        let mut candidate = make_test_candidate();
        let mut vpin_calc = crate::orderflow::VpinCalculator::new(1.0, 10);
        // Fill 6 buckets of pure sell volume (1.0 SOL each)
        for _ in 0..6 {
            vpin_calc.add_trade(false, 1.0);
        }
        assert!(vpin_calc.is_toxic_sell_dominated());
        candidate.vpin_calculator = Some(vpin_calc);

        let cfg = EngineConfig::default();
        let res = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(err.category(), RejectionCategory::LowConviction);
        match err {
            RejectionReason::OrderflowCvd { reason, metric_label } => {
                assert!(reason.starts_with("HIGH_VPIN_TOXICITY"));
                assert!(metric_label.contains("Toxic"));
            }
            _ => panic!("Expected OrderflowCvd HIGH_VPIN_TOXICITY error, got: {:?}", err),
        }
    }

    #[test]
    fn test_admission_gate_0_late_caller_distribution() {
        let mut candidate = make_test_candidate();
        let cfg = EngineConfig::default();

        // 1. Social call within 60s window (delta = 25s <= 60s) -> passes Gate 0
        candidate.caller_msg_timestamp_utc = Some(1794290025);
        candidate.pool_creation_timestamp_utc = Some(1794290000);
        let res_pass = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res_pass.is_ok());

        // 2. Social call after 60s window (delta = 85s > 60s) -> rejected at Gate 0
        candidate.caller_msg_timestamp_utc = Some(1794290085);
        candidate.pool_creation_timestamp_utc = Some(1794290000);
        let res_fail = evaluate_candidate_admission(&candidate, &cfg, None);
        assert!(res_fail.is_err());
        let err = res_fail.unwrap_err();
        assert_eq!(err.stage2_verdict(), "REJECT: LATE_CALLER_DISTRIBUTION");
        match err {
            RejectionReason::LateCallerDistribution { delta_seconds, max_seconds } => {
                assert_eq!(delta_seconds, 85);
                assert_eq!(max_seconds, 60);
            }
            _ => panic!("Expected LateCallerDistribution, got {:?}", err),
        }
    }

    #[test]
    fn test_v100_fail_closed_microstructure_gates() {
        let mut cfg = EngineConfig::default();
        cfg.genesis_guard.enabled = false;
        cfg.hawkes.enabled = false;
        cfg.orderflow.enabled = false;

        // 1. Token-2022 unsafe extension (transfer hook / permanent delegate / fee > 0) -> rejected
        let mut c1 = make_test_candidate();
        c1.token_security = Some(TokenSecurityProfile {
            mint_authority_revoked: true,
            freeze_authority_revoked: true,
            has_transfer_hook: true,
            has_permanent_delegate: false,
            transfer_fee_bps: 0,
        });
        assert!(evaluate_candidate_admission(&c1, &cfg, None).is_err());

        // 2. SPL transfer-graph connected component cap (C_max > 0.15) & zero-swap airdrop cluster -> rejected
        let mut c2 = make_test_candidate();
        c2.holder_records = Some(vec![
            TokenHolderRecord { holder_address: "A".to_string(), supply_pct: 6.0, is_liquidity_pool: false, funding_source: None },
            TokenHolderRecord { holder_address: "B".to_string(), supply_pct: 6.0, is_liquidity_pool: false, funding_source: None },
            TokenHolderRecord { holder_address: "C".to_string(), supply_pct: 5.0, is_liquidity_pool: false, funding_source: None },
            TokenHolderRecord { holder_address: "D".to_string(), supply_pct: 1.0, is_liquidity_pool: false, funding_source: None },
            TokenHolderRecord { holder_address: "E".to_string(), supply_pct: 1.0, is_liquidity_pool: false, funding_source: None },
        ]);
        c2.transfer_edges = Some(vec![
            crate::funder_graph::TransferEdge { from: "A".to_string(), to: "B".to_string(), has_swap: false },
            crate::funder_graph::TransferEdge { from: "B".to_string(), to: "C".to_string(), has_swap: false },
        ]);
        assert!(evaluate_candidate_admission(&c2, &cfg, None).is_err());

        // 3. Liquidity retention < 50% -> rejected
        let mut c3 = make_test_candidate();
        c3.liquidity_usd = 40_000.0;
        c3.peak_liquidity_usd = Some(100_000.0);
        assert!(evaluate_candidate_admission(&c3, &cfg, None).is_err());

        // 4. Dust-adjusted active holders (< 5 holders >= $25) -> rejected
        let mut c4 = make_test_candidate();
        c4.holder_usd_values = Some(vec![100.0, 50.0, 10.0, 5.0, 2.0]);
        assert!(evaluate_candidate_admission(&c4, &cfg, None).is_err());

        // 5. Intra-block wash eta > 0.65 & protocol fee phi < 0.70 -> rejected
        let mut c5 = make_test_candidate();
        c5.wash_volume_eta = Some(0.75);
        assert!(evaluate_candidate_admission(&c5, &cfg, None).is_err());

        let mut c6 = make_test_candidate();
        c6.protocol_fee_ratio_phi = Some(0.50);
        assert!(evaluate_candidate_admission(&c6, &cfg, None).is_err());
    }
}

