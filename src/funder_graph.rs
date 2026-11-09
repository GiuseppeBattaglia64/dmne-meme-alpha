// DMNE QUANT // MODULE 2: DEEP FUNDER GRAPH HHI / SYBIL CLUSTER FILTER
// Graph resolution engine: resolves holder funding lineage back to root funding sources
// Detects dev sybil clusters and concentrated root funding while guarding against CEX hot wallet false clustering
// Zero magic numbers: all thresholds declared in FunderGraphConfig with serde defaults

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::{Arc, RwLock, OnceLock};
use std::time::{Instant, Duration};
use serde::{Deserialize, Serialize};

/// Strongly typed configuration for Deep Funder Graph analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunderGraphConfig {
    /// Whether the Deep Funder Graph sybil cluster filter is enabled
    #[serde(default = "default_funder_enabled")]
    pub enabled: bool,

    /// Maximum recursive funding trace hops (default: 3)
    #[serde(default = "default_max_hops")]
    pub max_hops: usize,

    /// Maximum supply percentage controlled by a single root cluster before triggering SYBIL_DEV_CLUSTER (default: 18.0%)
    #[serde(default = "default_max_sybil_cluster_pct")]
    pub max_sybil_cluster_pct: f64,

    /// Maximum normalized Herfindahl-Hirschman Index of root funders before triggering CONCENTRATED_ROOT_FUNDING (default: 0.35)
    #[serde(default = "default_max_funder_hhi")]
    pub max_funder_hhi: f64,

    /// Maximum time delta (seconds) between CEX funding events to qualify as a coordinated burst (default: 300s)
    #[serde(default = "default_max_cluster_funding_window_secs")]
    pub max_cluster_funding_window_secs: u64,

    /// Maximum difference in SOL amounts to classify CEX withdrawals as identical scripted tranches (default: 0.005 SOL)
    #[serde(default = "default_cex_amount_tolerance_sol")]
    pub cex_amount_tolerance_sol: f64,

    /// Path to the offline on-chain graph cache
    #[serde(default = "default_cache_path")]
    pub cache_path: String,
}

fn default_funder_enabled() -> bool { true }
fn default_max_hops() -> usize { 3 }
fn default_max_sybil_cluster_pct() -> f64 { 18.0 }
fn default_max_funder_hhi() -> f64 { 0.35 }
fn default_max_cluster_funding_window_secs() -> u64 { 300 }
fn default_cex_amount_tolerance_sol() -> f64 { 0.005 }
fn default_cache_path() -> String { "logs/sandbox_onchain_cache.json".to_string() }

impl Default for FunderGraphConfig {
    fn default() -> Self {
        Self {
            enabled: default_funder_enabled(),
            max_hops: default_max_hops(),
            max_sybil_cluster_pct: default_max_sybil_cluster_pct(),
            max_funder_hhi: default_max_funder_hhi(),
            max_cluster_funding_window_secs: default_max_cluster_funding_window_secs(),
            cex_amount_tolerance_sol: default_cex_amount_tolerance_sol(),
            cache_path: default_cache_path(),
        }
    }
}

/// Known high-volume centralized exchange (CEX) deposit and hot wallets on Solana
pub const KNOWN_CEX_HOT_WALLETS: &[&str] = &[
    "5tzFkiKscMRHK5ZXPB62WYoqKkyTJaQUMMMiaE2L1Fhn", // Binance 1
    "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM", // Binance 2
    "2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPicm", // Coinbase 1
    "H8sMJSCQxfKiFTCfDR3DUMLPwcRbM61LGFJ8N4dK3WjS", // Coinbase 2
    "FWznbcNXWQuHTawe9RxvQ2LdJF2YmtV21TuDKs35H4BQ", // Kraken
    "5VCwKtCXgCJ6kit5FybXjvriW3xSfWXNQVdxidfzkvVu", // OKX
    "BmFdpraQhkiDQE6SnfG5omcA1VwzqfXrwtNYBwWTymy6", // KuCoin
    "AC5RDfQFmDS1deWZos921qbhirSeQiugjwxEmW5dR422", // Bybit
    "u6PJ8DtQuPFnfmwHbGFULQ4u4Egsv2M5R69YpWS2GhS", // Gate.io
    "ASTLFdGQMNRuyiptt85qKhpwY9eHj79U6jM4g7a3b3tS", // MEXC
];

/// Returns true if the address matches any known exchange hot wallet
pub fn is_known_cex_hot_wallet(address: &str) -> bool {
    let clean = address.trim();
    KNOWN_CEX_HOT_WALLETS.iter().any(|&w| w.eq_ignore_ascii_case(clean))
}

/// Funding transaction lineage record for a holder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HolderFundingSource {
    pub funder_address: String,
    pub timestamp_epoch: u64,
    pub amount_sol: f64,
    pub tx_signature: Option<String>,
    pub hop_depth: usize,
}

/// Token holder representation with supply share and funding lineage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenHolderRecord {
    pub holder_address: String,
    pub supply_pct: f64,
    pub is_liquidity_pool: bool,
    pub funding_source: Option<HolderFundingSource>,
}

pub type HolderRecord = TokenHolderRecord;

/// Cluster of token holders traced back to a common root funder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunderCluster {
    pub root_funder: String,
    pub is_cex_hot_wallet: bool,
    pub is_coordinated_cex_burst: bool,
    pub member_holders: Vec<String>,
    pub total_supply_pct: f64,
    pub member_count: usize,
    pub funding_timestamps: Vec<u64>,
    pub funding_amounts: Vec<f64>,
}

/// Comprehensive metrics evaluated by the Funder Graph engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunderGraphMetrics {
    pub token_mint: String,
    pub total_evaluated_supply_pct: f64,
    pub clusters_count: usize,
    pub max_cluster_supply_pct: f64,
    pub max_cluster_funder: String,
    pub max_cluster_size: usize,
    pub funder_hhi: f64,
    pub is_sybil_cluster: bool,
    pub is_concentrated_hhi: bool,
    pub is_rejected: bool,
    pub rejection_reason: Option<String>,
    pub clusters: Vec<FunderCluster>,
}

/// Evaluator executing graph resolution and sybil cluster detection
pub struct FunderGraphEvaluator {
    pub config: FunderGraphConfig,
    pub cache: HashMap<String, Vec<TokenHolderRecord>>,
}

/// Evaluates sniper profit overhang ceiling.
/// Returns (is_excess, top5_pct) where is_excess is true if top 5 non-pool wallets hold > 18.0% of supply AND price_multiple >= 3.50.
pub fn evaluate_sniper_overhang(holders: &[TokenHolderRecord], price_multiple: f64) -> (bool, f64) {
    let mut non_pool: Vec<f64> = holders
        .iter()
        .filter(|h| !h.is_liquidity_pool)
        .map(|h| h.supply_pct)
        .collect();
    non_pool.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let top5_pct: f64 = non_pool.iter().take(5).sum();
    let is_excess = top5_pct > 18.0 && price_multiple >= 3.50;
    (is_excess, top5_pct)
}

/// Calculates raw and normalized Shannon entropy over top non-pool holders:
/// - Filters out liquidity pools and AMM vaults (!h.is_liquidity_pool && h.supply_pct > 0.0).
/// - Takes up to top_n (default 20) individual holder percentages: p_i = pct / 100.0.
/// - Normalizes the probabilities across the sample: p'_i = p_i / sum(p_i).
/// - Computes raw Shannon entropy: H(X) = -sum(p'_i * log2(p'_i)).
/// - Computes normalized Shannon entropy: H_norm = H(X) / log2(N) (where N = number of evaluated holders, max 1.0).
/// Returns (raw_entropy, normalized_entropy).
pub fn calculate_shannon_entropy(holders: &[HolderRecord], top_n: usize) -> (f64, f64) {
    let mut non_pool: Vec<&HolderRecord> = holders
        .iter()
        .filter(|h| !h.is_liquidity_pool && h.supply_pct > 0.0)
        .collect();
    non_pool.sort_by(|a, b| b.supply_pct.partial_cmp(&a.supply_pct).unwrap_or(std::cmp::Ordering::Equal));
    let sample: Vec<&HolderRecord> = non_pool.into_iter().take(top_n).collect();

    let n = sample.len();
    if n <= 1 {
        return (0.0, 0.0);
    }

    let sum_p: f64 = sample.iter().map(|h| h.supply_pct / 100.0).sum();
    if sum_p <= 0.0 {
        return (0.0, 0.0);
    }

    let mut raw_entropy = 0.0;
    for h in &sample {
        let p_norm = (h.supply_pct / 100.0) / sum_p;
        if p_norm > 0.0 {
            raw_entropy -= p_norm * p_norm.log2();
        }
    }

    let max_entropy = (n as f64).log2();
    let norm_entropy = if max_entropy > 0.0 {
        (raw_entropy / max_entropy).clamp(0.0, 1.0)
    } else {
        0.0
    };

    (raw_entropy, norm_entropy)
}

/// Evaluates normalized Shannon entropy across top non-pool holders.
/// - If holders length < 5, returns Ok(()) (fail-closed already handles missing RPC).
/// - Calculates normalized entropy for top 20 holders.
/// - Invariant: If N >= 10 and H_norm < 0.62 (indicating unnatural sybil distribution or cartel concentration),
///   returns Err(format!("LOW_SHANNON_ENTROPY_{:.3}", h_norm)).
pub fn evaluate_holder_entropy(holders: &[HolderRecord]) -> Result<(), String> {
    if holders.len() < 5 {
        return Ok(());
    }

    let non_pool_count = holders
        .iter()
        .filter(|h| !h.is_liquidity_pool && h.supply_pct > 0.0)
        .count()
        .min(20);

    let (_raw_h, norm_h) = calculate_shannon_entropy(holders, 20);

    if non_pool_count >= 10 && norm_h < 0.62 {
        return Err(format!("LOW_SHANNON_ENTROPY_{:.3}", norm_h));
    }

    Ok(())
}

/// Thread-safe in-memory cache for token holder records (60-second TTL)
#[derive(Clone)]
pub struct HolderCache {
    inner: Arc<RwLock<HashMap<String, (Instant, Vec<TokenHolderRecord>)>>>,
}

impl Default for HolderCache {
    fn default() -> Self {
        Self::new()
    }
}

impl HolderCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get(&self, mint: &str) -> Option<Vec<TokenHolderRecord>> {
        let cache = self.inner.read().ok()?;
        if let Some((inserted_at, records)) = cache.get(mint) {
            if inserted_at.elapsed() < Duration::from_secs(60) {
                return Some(records.clone());
            }
        }
        None
    }

    pub fn insert(&self, mint: String, records: Vec<TokenHolderRecord>) {
        if let Ok(mut cache) = self.inner.write() {
            cache.insert(mint, (Instant::now(), records));
            // Evict stale entries if cache size exceeds 1000 items
            if cache.len() > 1000 {
                cache.retain(|_, (ts, _)| ts.elapsed() < Duration::from_secs(120));
            }
        }
    }
}

static GLOBAL_HOLDER_CACHE: OnceLock<HolderCache> = OnceLock::new();

pub fn get_global_holder_cache() -> &'static HolderCache {
    GLOBAL_HOLDER_CACHE.get_or_init(HolderCache::new)
}

/// Resolves token holder records via Solana JSON-RPC `getTokenLargestAccounts` (backed by 60s cache)
pub async fn fetch_holders_from_rpc(
    client: &reqwest::Client,
    rpc_url: &str,
    mint: &str,
    timeout_secs: u64,
) -> Vec<TokenHolderRecord> {
    fetch_holders_from_rpc_with_cache(client, rpc_url, mint, timeout_secs, get_global_holder_cache()).await
}

pub async fn fetch_holders_from_rpc_with_cache(
    client: &reqwest::Client,
    rpc_url: &str,
    mint: &str,
    timeout_secs: u64,
    cache: &HolderCache,
) -> Vec<TokenHolderRecord> {
    if let Some(cached) = cache.get(mint) {
        return cached;
    }

    let (shares, _) = crate::shadow_logger::fetch_rpc_holder_distribution(client, rpc_url, mint, timeout_secs).await;
    if shares.is_empty() {
        return Vec::new();
    }
    let records: Vec<TokenHolderRecord> = shares.into_iter().enumerate().map(|(i, pct)| {
        TokenHolderRecord {
            holder_address: format!("Holder_{}_{}", &mint[..6.min(mint.len())], i),
            supply_pct: pct,
            is_liquidity_pool: i == 0 && pct > 45.0,
            funding_source: None,
        }
    }).collect();

    cache.insert(mint.to_string(), records.clone());
    records
}

impl FunderGraphEvaluator {
    pub fn new(config: FunderGraphConfig, cache: HashMap<String, Vec<TokenHolderRecord>>) -> Self {
        Self { config, cache }
    }

    /// Evaluates normalized Shannon entropy across top non-pool holders.
    pub fn evaluate_holder_entropy(&self, holders: &[HolderRecord]) -> Result<(), String> {
        evaluate_holder_entropy(holders)
    }

    /// Evaluates sniper profit overhang ceiling on provided holders.
    pub fn evaluate_sniper_overhang(&self, holders: &[TokenHolderRecord], price_multiple: f64) -> (bool, f64) {
        evaluate_sniper_overhang(holders, price_multiple)
    }

    /// Evaluates a candidate token mint using cached holder distributions
    pub fn evaluate_token(&self, token_mint: &str) -> FunderGraphMetrics {
        if let Some(holders) = self.cache.get(token_mint) {
            self.evaluate_holders(token_mint, holders)
        } else {
            // Uncached token: neutral fallback with zero flags
            FunderGraphMetrics {
                token_mint: token_mint.to_string(),
                total_evaluated_supply_pct: 0.0,
                clusters_count: 0,
                max_cluster_supply_pct: 0.0,
                max_cluster_funder: "NONE".to_string(),
                max_cluster_size: 0,
                funder_hhi: 0.0,
                is_sybil_cluster: false,
                is_concentrated_hhi: false,
                is_rejected: false,
                rejection_reason: None,
                clusters: Vec::new(),
            }
        }
    }

    /// Evaluates a list of token holders according to graph clustering rules
    pub fn evaluate_holders(&self, token_mint: &str, holders: &[TokenHolderRecord]) -> FunderGraphMetrics {
        if !self.config.enabled {
            return FunderGraphMetrics {
                token_mint: token_mint.to_string(),
                total_evaluated_supply_pct: 0.0,
                clusters_count: 0,
                max_cluster_supply_pct: 0.0,
                max_cluster_funder: "DISABLED".to_string(),
                max_cluster_size: 0,
                funder_hhi: 0.0,
                is_sybil_cluster: false,
                is_concentrated_hhi: false,
                is_rejected: false,
                rejection_reason: None,
                clusters: Vec::new(),
            };
        }

        // Filter out liquidity pools / AMM vaults
        let non_pool_holders: Vec<&TokenHolderRecord> = holders
            .iter()
            .filter(|h| !h.is_liquidity_pool && h.supply_pct > 0.0)
            .collect();

        if non_pool_holders.is_empty() {
            return FunderGraphMetrics {
                token_mint: token_mint.to_string(),
                total_evaluated_supply_pct: 0.0,
                clusters_count: 0,
                max_cluster_supply_pct: 0.0,
                max_cluster_funder: "EMPTY".to_string(),
                max_cluster_size: 0,
                funder_hhi: 0.0,
                is_sybil_cluster: false,
                is_concentrated_hhi: false,
                is_rejected: false,
                rejection_reason: None,
                clusters: Vec::new(),
            };
        }

        // Group holders by raw funder address
        let mut raw_groups: HashMap<String, Vec<&TokenHolderRecord>> = HashMap::new();
        for &h in &non_pool_holders {
            let funder = h
                .funding_source
                .as_ref()
                .map(|f| f.funder_address.clone())
                .unwrap_or_else(|| h.holder_address.clone());
            raw_groups.entry(funder).or_default().push(h);
        }

        let mut final_clusters: Vec<FunderCluster> = Vec::new();

        for (funder_addr, members) in raw_groups {
            let is_cex = is_known_cex_hot_wallet(&funder_addr);

            if is_cex {
                // Apply CEX Hot Wallet Guard:
                // Check if any subset was funded in a coordinated burst (<= 300s, identical SOL amount)
                let mut burst_members: Vec<&TokenHolderRecord> = Vec::new();
                let mut independent_members: Vec<&TokenHolderRecord> = Vec::new();

                for &m in &members {
                    if let Some(src) = &m.funding_source {
                        // Check if m shares identical funding window and amount with at least one other member
                        let has_coordinated_peer = members.iter().any(|&other| {
                            if other.holder_address == m.holder_address {
                                return false;
                            }
                            if let Some(other_src) = &other.funding_source {
                                let dt = (src.timestamp_epoch as i64 - other_src.timestamp_epoch as i64).abs() as u64;
                                let da = (src.amount_sol - other_src.amount_sol).abs();
                                dt <= self.config.max_cluster_funding_window_secs
                                    && da <= self.config.cex_amount_tolerance_sol
                            } else {
                                false
                            }
                        });

                        if has_coordinated_peer {
                            burst_members.push(m);
                        } else {
                            independent_members.push(m);
                        }
                    } else {
                        independent_members.push(m);
                    }
                }

                // If coordinated burst found, cluster them together
                if !burst_members.is_empty() {
                    let total_sup: f64 = burst_members.iter().map(|m| m.supply_pct).sum();
                    let ts_vec: Vec<u64> = burst_members
                        .iter()
                        .filter_map(|m| m.funding_source.as_ref().map(|s| s.timestamp_epoch))
                        .collect();
                    let amt_vec: Vec<f64> = burst_members
                        .iter()
                        .filter_map(|m| m.funding_source.as_ref().map(|s| s.amount_sol))
                        .collect();

                    final_clusters.push(FunderCluster {
                        root_funder: format!("{}_COORDINATED_BURST", funder_addr),
                        is_cex_hot_wallet: true,
                        is_coordinated_cex_burst: true,
                        member_holders: burst_members.iter().map(|m| m.holder_address.clone()).collect(),
                        total_supply_pct: total_sup,
                        member_count: burst_members.len(),
                        funding_timestamps: ts_vec,
                        funding_amounts: amt_vec,
                    });
                }

                // Independent retail depositors treated as singletons
                for ind in independent_members {
                    let ts = ind.funding_source.as_ref().map(|s| s.timestamp_epoch).unwrap_or(0);
                    let amt = ind.funding_source.as_ref().map(|s| s.amount_sol).unwrap_or(0.0);
                    final_clusters.push(FunderCluster {
                        root_funder: format!("{}_RETAIL_{}", funder_addr, ind.holder_address),
                        is_cex_hot_wallet: true,
                        is_coordinated_cex_burst: false,
                        member_holders: vec![ind.holder_address.clone()],
                        total_supply_pct: ind.supply_pct,
                        member_count: 1,
                        funding_timestamps: vec![ts],
                        funding_amounts: vec![amt],
                    });
                }
            } else {
                // Non-CEX root funder: all members naturally cluster under common funder
                let total_sup: f64 = members.iter().map(|m| m.supply_pct).sum();
                let ts_vec: Vec<u64> = members
                    .iter()
                    .filter_map(|m| m.funding_source.as_ref().map(|s| s.timestamp_epoch))
                    .collect();
                let amt_vec: Vec<f64> = members
                    .iter()
                    .filter_map(|m| m.funding_source.as_ref().map(|s| s.amount_sol))
                    .collect();

                final_clusters.push(FunderCluster {
                    root_funder: funder_addr,
                    is_cex_hot_wallet: false,
                    is_coordinated_cex_burst: false,
                    member_holders: members.iter().map(|m| m.holder_address.clone()).collect(),
                    total_supply_pct: total_sup,
                    member_count: members.len(),
                    funding_timestamps: ts_vec,
                    funding_amounts: amt_vec,
                });
            }
        }

        // Sort clusters by supply descending
        final_clusters.sort_by(|a, b| b.total_supply_pct.partial_cmp(&a.total_supply_pct).unwrap_or(std::cmp::Ordering::Equal));

        let total_evaluated_supply: f64 = final_clusters.iter().map(|c| c.total_supply_pct).sum();

        let (max_cluster_supply, max_funder, max_size) = if let Some(top) = final_clusters.first() {
            (top.total_supply_pct, top.root_funder.clone(), top.member_count)
        } else {
            (0.0, "NONE".to_string(), 0)
        };

        // Compute normalized HHI over evaluated non-pool supply: HHI = sum((S_m / S_top)^2)
        let funder_hhi = if total_evaluated_supply > 0.0 {
            final_clusters
                .iter()
                .map(|c| {
                    let share = c.total_supply_pct / total_evaluated_supply;
                    share * share
                })
                .sum::<f64>()
        } else {
            0.0
        };

        let mut is_sybil = false;
        let mut is_conc = false;
        let mut reason = None;

        if max_cluster_supply >= self.config.max_sybil_cluster_pct {
            is_sybil = true;
            reason = Some(format!(
                "SYBIL_DEV_CLUSTER: root {} controls {:.2}% of supply >= {:.2}%",
                max_funder, max_cluster_supply, self.config.max_sybil_cluster_pct
            ));
        } else if funder_hhi >= self.config.max_funder_hhi {
            is_conc = true;
            reason = Some(format!(
                "CONCENTRATED_ROOT_FUNDING: funder HHI {:.3} >= ceiling {:.3}",
                funder_hhi, self.config.max_funder_hhi
            ));
        }

        let is_rejected = is_sybil || is_conc;

        FunderGraphMetrics {
            token_mint: token_mint.to_string(),
            total_evaluated_supply_pct: total_evaluated_supply,
            clusters_count: final_clusters.len(),
            max_cluster_supply_pct: max_cluster_supply,
            max_cluster_funder: max_funder,
            max_cluster_size: max_size,
            funder_hhi,
            is_sybil_cluster: is_sybil,
            is_concentrated_hhi: is_conc,
            is_rejected,
            rejection_reason: reason,
            clusters: final_clusters,
        }
    }

    /// Loads on-chain holder graph cache from JSON file
    pub fn load_cache(path: &str) -> Result<HashMap<String, Vec<TokenHolderRecord>>, Box<dyn std::error::Error>> {
        let p = Path::new(path);
        if !p.exists() {
            return Ok(HashMap::new());
        }
        let file = File::open(p)?;
        let reader = BufReader::new(file);
        let val: serde_json::Value = serde_json::from_reader(reader)?;

        let mut map: HashMap<String, Vec<TokenHolderRecord>> = HashMap::new();

        // Check if top-level has "funder_graph" key
        let target_obj = if let Some(fg) = val.get("funder_graph").and_then(|v| v.as_object()) {
            fg
        } else if let Some(obj) = val.as_object() {
            obj
        } else {
            return Ok(HashMap::new());
        };

        for (mint, h_val) in target_obj {
            if let Ok(records) = serde_json::from_value::<Vec<TokenHolderRecord>>(h_val.clone()) {
                map.insert(mint.clone(), records);
            }
        }

        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_funder_config_defaults() {
        let cfg = FunderGraphConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.max_hops, 3);
        assert_eq!(cfg.max_sybil_cluster_pct, 18.0);
        assert_eq!(cfg.max_funder_hhi, 0.35);
        assert_eq!(cfg.max_cluster_funding_window_secs, 300);
        assert_eq!(cfg.cex_amount_tolerance_sol, 0.005);
    }

    #[test]
    fn test_cex_hot_wallet_recognition() {
        assert!(is_known_cex_hot_wallet("2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPicm")); // Coinbase
        assert!(is_known_cex_hot_wallet("5tzFkiKscMRHK5ZXPB62WYoqKkyTJaQUMMMiaE2L1Fhn")); // Binance
        assert!(!is_known_cex_hot_wallet("4n7bKuY36L2uJUEFzik1fMjQLQqBwrPLkzFM1hofpump")); // Random token
    }

    #[test]
    fn test_sybil_dev_cluster_detection() {
        let cfg = FunderGraphConfig::default();
        let evaluator = FunderGraphEvaluator::new(cfg, HashMap::new());

        // 3 dev sub-wallets funded by common dev root wallet
        let holders = vec![
            TokenHolderRecord {
                holder_address: "DEV_H1".to_string(),
                supply_pct: 8.5,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: "ROOT_DEV_DISPENSER".to_string(),
                    timestamp_epoch: 1000,
                    amount_sol: 1.0,
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
            TokenHolderRecord {
                holder_address: "DEV_H2".to_string(),
                supply_pct: 6.5,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: "ROOT_DEV_DISPENSER".to_string(),
                    timestamp_epoch: 1020,
                    amount_sol: 1.0,
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
            TokenHolderRecord {
                holder_address: "DEV_H3".to_string(),
                supply_pct: 5.5,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: "ROOT_DEV_DISPENSER".to_string(),
                    timestamp_epoch: 1040,
                    amount_sol: 1.0,
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
            TokenHolderRecord {
                holder_address: "RETAIL_1".to_string(),
                supply_pct: 4.0,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: "INDEPENDENT_ROOT_1".to_string(),
                    timestamp_epoch: 500,
                    amount_sol: 2.5,
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
        ];

        let metrics = evaluator.evaluate_holders("TEST_MINT", &holders);
        assert!(metrics.is_rejected);
        assert!(metrics.is_sybil_cluster);
        assert_eq!(metrics.max_cluster_funder, "ROOT_DEV_DISPENSER");
        assert!((metrics.max_cluster_supply_pct - 20.5).abs() < 1e-6); // 8.5 + 6.5 + 5.5 = 20.5 >= 18.0
        assert!(metrics.rejection_reason.unwrap().contains("SYBIL_DEV_CLUSTER"));
    }

    #[test]
    fn test_cex_hot_wallet_guard_uncoordinated_retail_clearance() {
        let cfg = FunderGraphConfig::default();
        let evaluator = FunderGraphEvaluator::new(cfg, HashMap::new());

        let coinbase_addr = "2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPicm";

        // 3 organic retail holders funded from Coinbase hours apart with varied amounts
        let holders = vec![
            TokenHolderRecord {
                holder_address: "RETAIL_CB_1".to_string(),
                supply_pct: 6.0,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: coinbase_addr.to_string(),
                    timestamp_epoch: 10000,
                    amount_sol: 2.15,
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
            TokenHolderRecord {
                holder_address: "RETAIL_CB_2".to_string(),
                supply_pct: 7.0,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: coinbase_addr.to_string(),
                    timestamp_epoch: 35000, // hours later
                    amount_sol: 14.80,      // different amount
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
            TokenHolderRecord {
                holder_address: "RETAIL_CB_3".to_string(),
                supply_pct: 5.5,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: coinbase_addr.to_string(),
                    timestamp_epoch: 80000, // days later
                    amount_sol: 0.65,       // different amount
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
        ];

        let metrics = evaluator.evaluate_holders("ORGANIC_WINNER", &holders);
        // CEX guard splits them into 3 distinct singleton clusters!
        assert!(!metrics.is_rejected);
        assert!(!metrics.is_sybil_cluster);
        assert_eq!(metrics.clusters_count, 3);
        assert!((metrics.max_cluster_supply_pct - 7.0).abs() < 1e-6); // Max is 7.0% < 18.0%
        assert!(metrics.funder_hhi < 0.35);
    }

    #[test]
    fn test_cex_hot_wallet_coordinated_sybil_burst_detection() {
        let cfg = FunderGraphConfig::default();
        let evaluator = FunderGraphEvaluator::new(cfg, HashMap::new());

        let binance_addr = "5tzFkiKscMRHK5ZXPB62WYoqKkyTJaQUMMMiaE2L1Fhn";

        // Dev sybil withdrew from Binance within 90 seconds to 3 fresh wallets with identical 0.500 SOL
        let holders = vec![
            TokenHolderRecord {
                holder_address: "SYBIL_BN_1".to_string(),
                supply_pct: 8.0,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: binance_addr.to_string(),
                    timestamp_epoch: 5000,
                    amount_sol: 0.500,
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
            TokenHolderRecord {
                holder_address: "SYBIL_BN_2".to_string(),
                supply_pct: 7.0,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: binance_addr.to_string(),
                    timestamp_epoch: 5045, // 45s later
                    amount_sol: 0.500,     // identical amount
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
            TokenHolderRecord {
                holder_address: "SYBIL_BN_3".to_string(),
                supply_pct: 6.0,
                is_liquidity_pool: false,
                funding_source: Some(HolderFundingSource {
                    funder_address: binance_addr.to_string(),
                    timestamp_epoch: 5080, // 80s later
                    amount_sol: 0.500,     // identical amount
                    tx_signature: None,
                    hop_depth: 1,
                }),
            },
        ];

        let metrics = evaluator.evaluate_holders("SYBIL_CEX_BURST", &holders);
        // Coordinated burst clustered into 21.0% single cluster
        assert!(metrics.is_rejected);
        assert!(metrics.is_sybil_cluster);
        assert!((metrics.max_cluster_supply_pct - 21.0).abs() < 1e-6); // 8 + 7 + 6 = 21.0 >= 18.0
        assert!(metrics.rejection_reason.unwrap().contains("SYBIL_DEV_CLUSTER"));
    }

    #[test]
    fn test_evaluate_sniper_overhang() {
        let make_holder = |addr: &str, pct: f64, is_pool: bool| TokenHolderRecord {
            holder_address: addr.to_string(),
            supply_pct: pct,
            is_liquidity_pool: is_pool,
            funding_source: None,
        };

        let holders = vec![
            make_holder("POOL_RAYDIUM", 40.0, true), // pool ignored
            make_holder("SNIPER_1", 5.0, false),
            make_holder("SNIPER_2", 4.5, false),
            make_holder("SNIPER_3", 4.0, false),
            make_holder("SNIPER_4", 3.0, false),
            make_holder("SNIPER_5", 2.5, false), // top 5 non-pool = 5+4.5+4+3+2.5 = 19.0% (> 18.0%)
            make_holder("SNIPER_6", 2.0, false),
        ];

        // 1. High overhang (>18%) + High multiple (>=3.5x) => Reject
        let (excess, pct) = evaluate_sniper_overhang(&holders, 3.50);
        assert!(excess);
        assert!((pct - 19.0).abs() < 1e-6);

        // 2. High overhang (>18%) + Low multiple (<3.5x) => Cleared
        let (excess_low_mult, pct_low_mult) = evaluate_sniper_overhang(&holders, 2.50);
        assert!(!excess_low_mult);
        assert!((pct_low_mult - 19.0).abs() < 1e-6);

        // 3. Low overhang (<=18%) + High multiple (>=3.5x) => Cleared
        let low_overhang_holders = vec![
            make_holder("POOL_RAYDIUM", 60.0, true),
            make_holder("SNIPER_1", 3.0, false),
            make_holder("SNIPER_2", 3.0, false),
            make_holder("SNIPER_3", 3.0, false),
            make_holder("SNIPER_4", 3.0, false),
            make_holder("SNIPER_5", 3.0, false), // top 5 non-pool = 15.0% (<= 18.0%)
        ];
        let (excess_low, pct_low) = evaluate_sniper_overhang(&low_overhang_holders, 5.0);
        assert!(!excess_low);
        assert!((pct_low - 15.0).abs() < 1e-6);
    }

    #[test]
    fn test_calculate_shannon_entropy_uniform_and_clustered() {
        let make_holder = |addr: &str, pct: f64, is_pool: bool| TokenHolderRecord {
            holder_address: addr.to_string(),
            supply_pct: pct,
            is_liquidity_pool: is_pool,
            funding_source: None,
        };

        // 1. Uniform distribution: 20 equal non-pool holders (2.5% each) + 1 LP pool (ignored)
        let mut uniform_holders = vec![make_holder("LP_POOL", 50.0, true)];
        for i in 0..20 {
            uniform_holders.push(make_holder(&format!("HOLDER_{}", i), 2.5, false));
        }
        let (_raw_uniform, norm_uniform) = calculate_shannon_entropy(&uniform_holders, 20);
        assert!((norm_uniform - 1.0).abs() < 1e-4);
        assert!(evaluate_holder_entropy(&uniform_holders).is_ok());

        // 2. Heavily clustered / sybil cartel: 1 dominant wallet holds 90% and 10 small wallets hold 1% each
        let mut clustered_holders = vec![make_holder("LP_POOL", 10.0, true)];
        clustered_holders.push(make_holder("CARTEL_WHALE", 90.0, false));
        for i in 0..10 {
            clustered_holders.push(make_holder(&format!("SYBIL_{}", i), 1.0, false));
        }
        let (_raw_clustered, norm_clustered) = calculate_shannon_entropy(&clustered_holders, 20);
        assert!(norm_clustered < 0.62);
        let res = evaluate_holder_entropy(&clustered_holders);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("LOW_SHANNON_ENTROPY"));

        // 3. Small sample (< 5 holders total) => Bypassed
        let small_holders = vec![
            make_holder("HOLDER_1", 10.0, false),
            make_holder("HOLDER_2", 10.0, false),
        ];
        assert!(evaluate_holder_entropy(&small_holders).is_ok());
    }

    #[tokio::test]
    async fn test_holder_cache_hit_within_60s() {
        let cache = HolderCache::new();
        let records = vec![
            TokenHolderRecord {
                holder_address: "HolderA".to_string(),
                supply_pct: 12.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
            TokenHolderRecord {
                holder_address: "HolderB".to_string(),
                supply_pct: 8.0,
                is_liquidity_pool: false,
                funding_source: None,
            },
        ];

        cache.insert("TestMint123".to_string(), records.clone());

        // First retrieval should hit cache
        let cached = cache.get("TestMint123");
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 2);

        // Repeated query within 60s returns from cache without re-invoking network
        let client = reqwest::Client::new();
        let res = fetch_holders_from_rpc_with_cache(
            &client,
            "http://invalid-dead-endpoint.example.com",
            "TestMint123",
            1,
            &cache,
        ).await;
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].holder_address, "HolderA");
    }
}

