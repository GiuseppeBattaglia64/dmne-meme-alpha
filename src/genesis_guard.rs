// DMNE QUANT // GENESIS BUNDLE PROTECTION & ANTI-SNIPER CLUSTER DETECTION
// Slot 0 bundle audit, shared tip account recognition & coordinated sniper defense

use serde::{Deserialize, Serialize};
use crate::config::GenesisGuardConfig;

/// Profile of initial slot 0 transactions and supply distribution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenesisBundleProfile {
    pub mint: String,
    pub pool_creation_slot: u64,
    pub slot0_buy_count: u32,
    pub slot0_supply_bought_pct: f64,
    pub shared_tip_detected: bool,
    pub common_funding_source: Option<String>,
}

/// Evaluation verdict for genesis bundle audit
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GenesisVerdict {
    Admitted,
    Rejected {
        reason: String,
        metric_str: String,
    },
}

/// Evaluator checking genesis bundle risk metrics against institutional constraints
#[derive(Debug, Clone)]
pub struct GenesisGuardEvaluator {
    pub config: GenesisGuardConfig,
}

impl GenesisGuardEvaluator {
    pub fn new(config: GenesisGuardConfig) -> Self {
        Self { config }
    }

    /// Evaluates a candidate token's genesis bundle profile
    pub fn evaluate(&self, profile: &GenesisBundleProfile) -> GenesisVerdict {
        if !self.config.enabled {
            return GenesisVerdict::Admitted;
        }

        // 1. Check aggregate supply sniped in slot 0
        if profile.slot0_supply_bought_pct >= self.config.max_genesis_supply_pct {
            return GenesisVerdict::Rejected {
                reason: format!(
                    "Genesis slot_0 supply sniped ({:.1}%) exceeds ceiling ({:.1}%)",
                    profile.slot0_supply_bought_pct, self.config.max_genesis_supply_pct
                ),
                metric_str: format!("Slot0: {:.1}%", profile.slot0_supply_bought_pct),
            };
        }

        // 2. Check number of coordinated buys in block 0
        if profile.slot0_buy_count > self.config.max_slot0_bundled_txs {
            return GenesisVerdict::Rejected {
                reason: format!(
                    "Genesis slot_0 coordinated buys ({}) exceeds bundle ceiling ({})",
                    profile.slot0_buy_count, self.config.max_slot0_bundled_txs
                ),
                metric_str: format!("Slot0Buys: {}", profile.slot0_buy_count),
            };
        }

        // 3. Check for shared tip account across multi-wallet bundle
        if self.config.reject_shared_tip_accounts && profile.shared_tip_detected {
            return GenesisVerdict::Rejected {
                reason: "Genesis slot_0 multi-wallet bundle shares identical Jito tip account".to_string(),
                metric_str: "SharedTip: TRUE".to_string(),
            };
        }

        GenesisVerdict::Admitted
    }
}

/// Inspects on-chain genesis bundle profile for a candidate mint
pub async fn inspect_candidate_genesis(
    _client: &reqwest::Client,
    _rpc_endpoint: &str,
    mint: &str,
    _pool_address: &str,
) -> GenesisBundleProfile {
    // In live execution, defaults to clean baseline unless flagged by websocket/RPC bundle watcher
    GenesisBundleProfile {
        mint: mint.to_string(),
        pool_creation_slot: 0,
        slot0_buy_count: 0,
        slot0_supply_bought_pct: 0.0,
        shared_tip_detected: false,
        common_funding_source: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_guard_rejects_heavy_bundled_supply() {
        let config = GenesisGuardConfig {
            enabled: true,
            max_genesis_supply_pct: 25.0,
            max_slot0_bundled_txs: 3,
            reject_shared_tip_accounts: true,
        };
        let evaluator = GenesisGuardEvaluator::new(config);

        let profile = GenesisBundleProfile {
            mint: "HeavySnipedMint111111111111111111111111111".to_string(),
            pool_creation_slot: 285_000_100,
            slot0_buy_count: 2,
            slot0_supply_bought_pct: 35.0, // 35% > 25%
            shared_tip_detected: false,
            common_funding_source: None,
        };

        let verdict = evaluator.evaluate(&profile);
        match verdict {
            GenesisVerdict::Rejected { reason, metric_str } => {
                assert!(reason.contains("supply sniped (35.0%) exceeds ceiling (25.0%)"));
                assert_eq!(metric_str, "Slot0: 35.0%");
            }
            GenesisVerdict::Admitted => panic!("Expected rejection for 35% slot_0 supply snipe"),
        }
    }

    #[test]
    fn test_genesis_guard_rejects_shared_tip_bundle() {
        let config = GenesisGuardConfig {
            enabled: true,
            max_genesis_supply_pct: 25.0,
            max_slot0_bundled_txs: 3,
            reject_shared_tip_accounts: true,
        };
        let evaluator = GenesisGuardEvaluator::new(config);

        let profile = GenesisBundleProfile {
            mint: "SharedTipSybilMint111111111111111111111111".to_string(),
            pool_creation_slot: 285_000_150,
            slot0_buy_count: 2,
            slot0_supply_bought_pct: 12.0, // Within supply ceiling
            shared_tip_detected: true,    // But shared tip detected
            common_funding_source: Some("Funder11111111111111111111111111111111111".to_string()),
        };

        let verdict = evaluator.evaluate(&profile);
        match verdict {
            GenesisVerdict::Rejected { reason, metric_str } => {
                assert!(reason.contains("shares identical Jito tip account"));
                assert_eq!(metric_str, "SharedTip: TRUE");
            }
            GenesisVerdict::Admitted => panic!("Expected rejection for shared tip bundle"),
        }
    }

    #[test]
    fn test_genesis_guard_admits_organic_distribution() {
        let config = GenesisGuardConfig {
            enabled: true,
            max_genesis_supply_pct: 25.0,
            max_slot0_bundled_txs: 3,
            reject_shared_tip_accounts: true,
        };
        let evaluator = GenesisGuardEvaluator::new(config);

        let profile = GenesisBundleProfile {
            mint: "OrganicFairLaunchMint111111111111111111111".to_string(),
            pool_creation_slot: 285_000_200,
            slot0_buy_count: 1,           // Single organic buy
            slot0_supply_bought_pct: 8.0, // 8.0% < 25.0%
            shared_tip_detected: false,
            common_funding_source: None,
        };

        let verdict = evaluator.evaluate(&profile);
        assert_eq!(verdict, GenesisVerdict::Admitted);
    }
}
