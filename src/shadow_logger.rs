// DMNE QUANT // ISOLATED NON-BLOCKING SHADOW-MODE MICROSTRUCTURE LOGGER
// Zero-disruption on-chain Hawkes sequences, true VPIN volume bars, and RPC holder entropy

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, RwLock};

pub const DEFAULT_SHADOW_LOG_PATH: &str = "logs/shadow_microstructure.jsonl";
pub const DEFAULT_RPC_TIMEOUT_SECS: u64 = 4;
pub const MAX_TOP_HOLDERS_COUNT: usize = 10;
pub const DEFAULT_HAWKES_WINDOW_SIZE: usize = 50;
pub const DEFAULT_SOL_PRICE_USD: f64 = 180.0;
pub const JSONRPC_VERSION: &str = "2.0";
pub const RPC_COMMITMENT_CONFIRMED: &str = "confirmed";

/// Strict, zero-lookahead shadow microstructure snapshot evaluated at pre-entry
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShadowMicrostructureRecord {
    pub timestamp_epoch: f64,
    pub token_mint: String,
    pub symbol: String,
    pub spot_price_sol: f64,
    pub liquidity_usd: f64,
    pub fdv_usd: f64,
    pub swap_interval_deltas_ms: Vec<u64>,     // Raw Hawkes arrival deltas
    pub buy_volume_sol_window: f64,            // Real VPIN buy slice
    pub sell_volume_sol_window: f64,           // Real VPIN sell slice
    pub top_10_holder_shares: Vec<f64>,        // Real RPC holder supply fractions
    pub holder_shannon_entropy: f64,           // -sum(p * ln(p))
    pub is_admitted: bool,
    pub rejection_reason: Option<String>,
}

/// Computes holder Shannon entropy: H = -sum(p * ln(p))
/// Shares are normalized so that sum(p) == 1.0 over non-zero accounts.
pub fn compute_holder_shannon_entropy(shares: &[f64]) -> f64 {
    let valid_sum: f64 = shares.iter().copied().filter(|&s| s > 0.0).sum();
    if valid_sum <= 0.0 {
        return 0.0;
    }
    shares
        .iter()
        .copied()
        .filter(|&s| s > 0.0)
        .fold(0.0, |acc, s| {
            let p = s / valid_sum;
            if p > 0.0 {
                acc - (p * p.ln())
            } else {
                acc
            }
        })
}

/// Computes raw Hawkes arrival interval deltas (ms) between consecutive sorted timestamps
pub fn compute_arrival_deltas_ms(timestamps_ms: &[u64]) -> Vec<u64> {
    if timestamps_ms.len() < 2 {
        return Vec::new();
    }
    timestamps_ms
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]))
        .collect()
}

/// Computes Volume-Synchronized Probability of Toxicity (VPIN) imbalance: |V_B - V_S| / (V_B + V_S)
pub fn compute_vpin_imbalance(buy_volume_sol: f64, sell_volume_sol: f64) -> f64 {
    let total = buy_volume_sol + sell_volume_sol;
    if total <= 0.0 {
        0.0
    } else {
        (buy_volume_sol - sell_volume_sol).abs() / total
    }
}

/// Computes estimated buy and sell volume slices in SOL from window volume and order-flow counts
pub fn compute_vpin_volume_slices(
    volume_usd: f64,
    buy_txns: u64,
    sell_txns: u64,
    sol_price_usd: f64,
) -> (f64, f64) {
    if sol_price_usd <= 0.0 || volume_usd <= 0.0 {
        return (0.0, 0.0);
    }
    let total_sol = volume_usd / sol_price_usd;
    let total_txns = buy_txns + sell_txns;
    if total_txns == 0 {
        let half = total_sol / 2.0;
        return (half, half);
    }
    let buy_fraction = buy_txns as f64 / total_txns as f64;
    let sell_fraction = sell_txns as f64 / total_txns as f64;
    (total_sol * buy_fraction, total_sol * sell_fraction)
}

/// Thread-safe in-memory tracker for live Hawkes swap arrival sequences
#[derive(Debug, Clone)]
pub struct HawkesArrivalTracker {
    arrivals: Arc<RwLock<HashMap<String, Vec<u64>>>>,
    max_history_len: usize,
}

impl Default for HawkesArrivalTracker {
    fn default() -> Self {
        Self::new(DEFAULT_HAWKES_WINDOW_SIZE)
    }
}

impl HawkesArrivalTracker {
    pub fn new(max_history_len: usize) -> Self {
        Self {
            arrivals: Arc::new(RwLock::new(HashMap::new())),
            max_history_len,
        }
    }

    /// Records an arrival timestamp (in ms) for a token mint
    pub async fn record_arrival(&self, token_mint: &str, timestamp_ms: u64) {
        let mut map = self.arrivals.write().await;
        let list = map.entry(token_mint.to_string()).or_default();
        list.push(timestamp_ms);
        if list.len() > self.max_history_len {
            let drop_count = list.len().saturating_sub(self.max_history_len);
            list.drain(0..drop_count);
        }
    }

    /// Retrieves interval deltas (ms) between consecutive arrivals for a token mint
    pub async fn get_interval_deltas_ms(&self, token_mint: &str) -> Vec<u64> {
        let map = self.arrivals.read().await;
        if let Some(list) = map.get(token_mint) {
            compute_arrival_deltas_ms(list)
        } else {
            Vec::new()
        }
    }
}

/// Queries Solana JSON-RPC for token largest accounts and total supply, returning top 10 shares and Shannon entropy
pub async fn fetch_rpc_holder_distribution(
    client: &reqwest::Client,
    rpc_url: &str,
    mint: &str,
    timeout_secs: u64,
) -> (Vec<f64>, f64) {
    let timeout = std::time::Duration::from_secs(timeout_secs);

    // 1. Fetch largest token accounts
    let accounts_payload = serde_json::json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 1,
        "method": "getTokenLargestAccounts",
        "params": [
            mint,
            { "commitment": RPC_COMMITMENT_CONFIRMED }
        ]
    });

    let accounts_res = client
        .post(rpc_url)
        .timeout(timeout)
        .json(&accounts_payload)
        .send()
        .await;

    let mut ui_amounts: Vec<f64> = Vec::new();

    if let Ok(res) = accounts_res {
        if let Ok(json) = res.json::<serde_json::Value>().await {
            if let Some(accounts) = json
                .get("result")
                .and_then(|r| r.get("value"))
                .and_then(|v| v.as_array())
            {
                for a in accounts.iter().take(MAX_TOP_HOLDERS_COUNT) {
                    if let Some(amt) = a.get("uiAmount").and_then(|v| v.as_f64()) {
                        if amt > 0.0 {
                            ui_amounts.push(amt);
                        }
                    }
                }
            }
        }
    }

    if ui_amounts.is_empty() {
        return (Vec::new(), 0.0);
    }

    // 2. Fetch total token supply
    let supply_payload = serde_json::json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 2,
        "method": "getTokenSupply",
        "params": [
            mint,
            { "commitment": RPC_COMMITMENT_CONFIRMED }
        ]
    });

    let supply_res = client
        .post(rpc_url)
        .timeout(timeout)
        .json(&supply_payload)
        .send()
        .await;

    let mut total_supply: f64 = 0.0;
    if let Ok(res) = supply_res {
        if let Ok(json) = res.json::<serde_json::Value>().await {
            if let Some(s) = json
                .get("result")
                .and_then(|r| r.get("value"))
                .and_then(|v| v.get("uiAmount"))
                .and_then(|v| v.as_f64())
            {
                if s > 0.0 {
                    total_supply = s;
                }
            }
        }
    }

    // If total supply is unobserved or 0, fallback to the sum of top accounts
    let base_supply = if total_supply > 0.0 {
        total_supply
    } else {
        ui_amounts.iter().sum()
    };

    let shares: Vec<f64> = if base_supply > 0.0 {
        ui_amounts.iter().map(|&amt| amt / base_supply).collect()
    } else {
        Vec::new()
    };

    let entropy = compute_holder_shannon_entropy(&shares);
    (shares, entropy)
}

/// Asynchronous, non-blocking handle for logging shadow microstructure telemetry
#[derive(Debug, Clone)]
pub struct ShadowLogger {
    tx: mpsc::UnboundedSender<ShadowMicrostructureRecord>,
}

impl ShadowLogger {
    /// Spawns a background logging worker that appends records to a JSON Lines file
    pub fn spawn(output_path: &str) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<ShadowMicrostructureRecord>();
        let path_str = output_path.to_string();

        tokio::spawn(async move {
            let path = Path::new(&path_str);
            if let Some(parent) = path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }

            let file_res = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await;

            let mut file = match file_res {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("[SHADOW LOGGER ERROR] Failed to open {}: {}", path_str, e);
                    return;
                }
            };

            while let Some(record) = rx.recv().await {
                if let Ok(serialized) = serde_json::to_string(&record) {
                    let mut line = serialized;
                    line.push('\n');
                    if let Err(e) = file.write_all(line.as_bytes()).await {
                        eprintln!("[SHADOW LOGGER ERROR] Failed to write log line: {}", e);
                    }
                    let _ = file.flush().await;
                }
            }
        });

        Self { tx }
    }

    /// Dispatches a record to the background logging worker without blocking the calling thread
    pub fn log_record(&self, record: ShadowMicrostructureRecord) -> bool {
        self.tx.send(record).is_ok()
    }
}

/// Spawns an isolated background task to fetch RPC holder metrics and log the candidate
pub fn spawn_shadow_candidate_logger(
    logger: ShadowLogger,
    client: reqwest::Client,
    rpc_url: String,
    token_mint: String,
    symbol: String,
    spot_price_sol: f64,
    liquidity_usd: f64,
    fdv_usd: f64,
    swap_interval_deltas_ms: Vec<u64>,
    buy_volume_sol_window: f64,
    sell_volume_sol_window: f64,
    is_admitted: bool,
    rejection_reason: Option<String>,
) {
    tokio::spawn(async move {
        let (top_10_holder_shares, holder_shannon_entropy) = fetch_rpc_holder_distribution(
            &client,
            &rpc_url,
            &token_mint,
            DEFAULT_RPC_TIMEOUT_SECS,
        )
        .await;

        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        let record = ShadowMicrostructureRecord {
            timestamp_epoch: now_epoch,
            token_mint,
            symbol,
            spot_price_sol,
            liquidity_usd,
            fdv_usd,
            swap_interval_deltas_ms,
            buy_volume_sol_window,
            sell_volume_sol_window,
            top_10_holder_shares,
            holder_shannon_entropy,
            is_admitted,
            rejection_reason,
        };

        logger.log_record(record);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_holder_shannon_entropy_single_holder() {
        let shares = vec![1.0];
        let entropy = compute_holder_shannon_entropy(&shares);
        assert_eq!(entropy, 0.0);
    }

    #[test]
    fn test_compute_holder_shannon_entropy_equal_distribution() {
        let count = 4usize;
        let shares = vec![0.25, 0.25, 0.25, 0.25];
        let entropy = compute_holder_shannon_entropy(&shares);
        let expected = (count as f64).ln();
        assert!((entropy - expected).abs() < 1e-9);
    }

    #[test]
    fn test_compute_holder_shannon_entropy_empty_or_zero() {
        assert_eq!(compute_holder_shannon_entropy(&[]), 0.0);
        assert_eq!(compute_holder_shannon_entropy(&[0.0, 0.0]), 0.0);
    }

    #[test]
    fn test_compute_arrival_deltas_ms() {
        let timestamps = vec![1000, 1250, 1600, 2000];
        let deltas = compute_arrival_deltas_ms(&timestamps);
        assert_eq!(deltas, vec![250, 350, 400]);

        assert!(compute_arrival_deltas_ms(&[]).is_empty());
        assert!(compute_arrival_deltas_ms(&[500]).is_empty());
    }

    #[test]
    fn test_compute_vpin_imbalance() {
        assert_eq!(compute_vpin_imbalance(10.0, 10.0), 0.0);
        assert_eq!(compute_vpin_imbalance(20.0, 0.0), 1.0);
        let imb = compute_vpin_imbalance(15.0, 5.0);
        assert!((imb - 0.50).abs() < 1e-9);
        assert_eq!(compute_vpin_imbalance(0.0, 0.0), 0.0);
    }

    #[test]
    fn test_compute_vpin_volume_slices() {
        let volume_usd = 18_000.0;
        let sol_price = 180.0; // 100 SOL total volume
        let (buy_sol, sell_sol) = compute_vpin_volume_slices(volume_usd, 75, 25, sol_price);
        assert!((buy_sol - 75.0).abs() < 1e-6);
        assert!((sell_sol - 25.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_hawkes_arrival_tracker() {
        let tracker = HawkesArrivalTracker::new(5);
        tracker.record_arrival("MINT_A", 1000).await;
        tracker.record_arrival("MINT_A", 1200).await;
        tracker.record_arrival("MINT_A", 1500).await;

        let deltas = tracker.get_interval_deltas_ms("MINT_A").await;
        assert_eq!(deltas, vec![200, 300]);

        let empty_deltas = tracker.get_interval_deltas_ms("UNKNOWN").await;
        assert!(empty_deltas.is_empty());
    }

    #[tokio::test]
    async fn test_shadow_logger_write_and_readback() {
        let temp_dir = std::env::temp_dir();
        let log_file = temp_dir.join(format!("test_shadow_{}.jsonl", std::process::id()));
        let log_file_str = log_file.to_str().unwrap();

        let logger = ShadowLogger::spawn(log_file_str);

        let record = ShadowMicrostructureRecord {
            timestamp_epoch: 1700000000.0,
            token_mint: "TestMint1111111111111111111111111111111111".to_string(),
            symbol: "TEST".to_string(),
            spot_price_sol: 0.00005,
            liquidity_usd: 25000.0,
            fdv_usd: 50000.0,
            swap_interval_deltas_ms: vec![120, 250, 310],
            buy_volume_sol_window: 12.5,
            sell_volume_sol_window: 4.2,
            top_10_holder_shares: vec![0.15, 0.10, 0.08, 0.05],
            holder_shannon_entropy: 1.32,
            is_admitted: true,
            rejection_reason: None,
        };

        let sent = logger.log_record(record.clone());
        assert!(sent);

        // Allow async flush
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let content = tokio::fs::read_to_string(&log_file).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        let read_record: ShadowMicrostructureRecord = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(read_record.symbol, "TEST");
        assert_eq!(read_record.swap_interval_deltas_ms, vec![120, 250, 310]);
        assert_eq!(read_record.top_10_holder_shares.len(), 4);
        assert!(read_record.is_admitted);

        let _ = tokio::fs::remove_file(&log_file).await;
    }
}

