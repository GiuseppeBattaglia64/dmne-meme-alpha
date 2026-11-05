// DMNE QUANT // TIER-1 INSTITUTIONAL MODULE
// Dynamic Jito Tip Discovery, Statistical Gateways, and Rolling Volatility Metrics

use reqwest::Client;
use serde_json::Value;
use crate::config::EngineConfig;

/// Dynamically discovers active Jito tip accounts from the Block Engine RPC
pub async fn fetch_dynamic_tip_accounts(client: &Client, jito_url: &str) -> Vec<String> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTipAccounts",
        "params": []
    });

    if let Ok(res) = client.post(jito_url).json(&payload).send().await {
        if let Ok(json) = res.json::<Value>().await {
            if let Some(arr) = json.get("result").and_then(|r| r.as_array()) {
                let accounts: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                if !accounts.is_empty() {
                    return accounts;
                }
            }
        }
    }
    // Dynamic fallback to system program if block engine discovery is unreachable
    vec!["96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5".to_string()]
}

/// Dynamic Sizing bounded by Half-Kelly and Pool Impact Ceiling
pub fn calculate_dynamic_size(bankroll_sol: f64, pool_liquidity_usd: f64, cfg: &EngineConfig) -> f64 {
    let pool_sol_reserve = if cfg.sol_reference_price_usd > 0.0 {
        (pool_liquidity_usd * 0.5) / cfg.sol_reference_price_usd
    } else {
        20.0
    };

    let impact_ceiling = cfg.max_price_impact_ratio * pool_sol_reserve;
    let kelly_allocation = cfg.kelly_nav_fraction * bankroll_sol;
    let computed = impact_ceiling.min(kelly_allocation).min(cfg.max_entry_sol);
    computed.max(cfg.min_entry_sol)
}

/// Evaluates holder concentration via Herfindahl-Hirschman Index (HHI)
pub async fn evaluate_holder_hhi(client: &Client, rpc_url: &str, mint: &str, ceiling: f64) -> bool {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTokenLargestAccounts",
        "params": [mint]
    });

    if let Ok(res) = client.post(rpc_url).json(&payload).send().await {
        if let Ok(json) = res.json::<Value>().await {
            if let Some(accounts) = json.get("result").and_then(|r| r.get("value")).and_then(|v| v.as_array()) {
                if !accounts.is_empty() {
                    let amounts: Vec<f64> = accounts.iter()
                        .filter_map(|a| a.get("uiAmount").and_then(|v| v.as_f64()))
                        .collect();
                    let sum: f64 = amounts.iter().sum();
                    if sum > 0.0 {
                        let hhi: f64 = amounts.iter().map(|&a| (a / sum).powi(2)).sum();
                        if hhi > ceiling {
                            println!("[GATE REJECT] Mint {}: HHI {:.3} exceeds threshold {:.3} (Concentrated dump risk)", mint, hhi, ceiling);
                            return false;
                        }
                    }
                }
            }
        }
    }
    true
}

/// Pump.fun-aware LP Burn & Lock Verifier
pub async fn verify_lp_burned_or_locked(
    client: &Client,
    rpc_url: &str,
    mint: &str,
    pair_addr: &str,
    dex_id: &str,
    min_dead_ratio: f64,
) -> bool {
    if dex_id == "pumpfun" || mint.ends_with("pump") {
        return true;
    }

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTokenSupply",
        "params": [pair_addr]
    });

    let supply: f64 = match client.post(rpc_url).json(&payload).send().await {
        Ok(res) => match res.json::<Value>().await {
            Ok(json) => json["result"]["value"]["uiAmount"].as_f64().unwrap_or(0.0),
            Err(_) => return false,
        },
        Err(_) => return false,
    };

    if supply == 0.0 {
        return true;
    }

    let largest_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "getTokenLargestAccounts",
        "params": [pair_addr]
    });

    if let Ok(res) = client.post(rpc_url).json(&largest_payload).send().await {
        if let Ok(json) = res.json::<Value>().await {
            if let Some(accounts) = json["result"]["value"].as_array() {
                for acc in accounts {
                    let addr = acc["address"].as_str().unwrap_or("");
                    let amount = acc["uiAmount"].as_f64().unwrap_or(0.0);
                    if (addr.starts_with("11111111111111111111111111111111") || addr.starts_with("Dead"))
                        && (amount / supply) >= min_dead_ratio
                    {
                        return true;
                    }
                }
            }
        }
    }

    println!("[GATE REJECT] AMM pair {}: LP tokens unverified on-chain", pair_addr);
    false
}

/// Pre-flight RPC transaction simulation
pub async fn simulate_swap_preflight(
    client: &Client,
    rpc_url: &str,
    encoded_tx_base64: &str,
) -> Result<u64, String> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "simulateTransaction",
        "params": [
            encoded_tx_base64,
            {
                "sigVerify": false,
                "replaceRecentBlockhash": true,
                "commitment": "processed",
                "encoding": "base64"
            }
        ]
    });

    let res = client.post(rpc_url).json(&payload).send().await
        .map_err(|e| format!("RPC error: {}", e))?;
    let json: Value = res.json().await.map_err(|e| format!("Parse error: {}", e))?;

    let err_val = &json["result"]["value"]["err"];
    if !err_val.is_null() {
        return Err(format!("Simulation reverted: {}", err_val));
    }

    let units = json["result"]["value"]["unitsConsumed"].as_u64().unwrap_or(200_000);
    Ok(units)
}

/// Dispatches atomic transaction bundle to Jito Block Engine
pub async fn broadcast_jito_bundle(
    client: &Client,
    jito_endpoint: &str,
    base58_txs: Vec<String>,
) -> Result<String, String> {
    if base58_txs.is_empty() || base58_txs.len() > 5 {
        return Err("Bundle must contain between 1 and 5 transactions".to_string());
    }

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendBundle",
        "params": [base58_txs]
    });

    let res = client.post(jito_endpoint).header("Content-Type", "application/json")
        .json(&payload).send().await.map_err(|e| format!("Jito transport error: {}", e))?;
    let json: Value = res.json().await.map_err(|e| format!("Jito parse error: {}", e))?;

    if let Some(id) = json["result"].as_str() {
        Ok(id.to_string())
    } else {
        Err(format!("Bundle rejected: {:?}", json.get("error")))
    }
}

/// Computes rolling standard deviation of returns (sigma)
pub fn calculate_rolling_volatility(prices: &[f64]) -> f64 {
    if prices.len() < 3 {
        return 0.05; // 5% baseline volatility fallback
    }
    let returns: Vec<f64> = prices.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    variance.sqrt().max(0.02)
}

/// Computes the Z-Score of the most recent price return
pub fn calculate_return_z_score(prices: &[f64]) -> f64 {
    if prices.len() < 4 {
        return 0.0;
    }
    let returns: Vec<f64> = prices.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
    let n = returns.len();
    let latest_return = returns[n - 1];
    let historical_returns = &returns[..n - 1];
    let mean = historical_returns.iter().sum::<f64>() / historical_returns.len() as f64;
    let variance = historical_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / historical_returns.len() as f64;
    let std_dev = variance.sqrt();
    if std_dev > 0.0001 {
        (latest_return - mean) / std_dev
    } else {
        0.0
    }
}


#[derive(Debug, Clone, serde::Deserialize)]
pub struct JitoTipFloorStats {
    pub landed_tips_25th_percentile: Option<f64>,
    pub landed_tips_50th_percentile: Option<f64>,
    pub landed_tips_75th_percentile: Option<f64>,
    pub landed_tips_95th_percentile: Option<f64>,
    pub landed_tips_99th_percentile: Option<f64>,
}

/// Dynamically queries Jito Tip Floor percentiles across recent bundles
pub async fn fetch_dynamic_tip_lamports(
    client: &Client,
    tip_floor_url: &str,
    target_percentile: &str,
) -> u64 {
    if let Ok(res) = client.get(tip_floor_url).send().await {
        if let Ok(arr) = res.json::<Vec<JitoTipFloorStats>>().await {
            if let Some(stats) = arr.first() {
                let tip_sol = match target_percentile {
                    "p75" => stats.landed_tips_75th_percentile.unwrap_or(0.001),
                    "p95" => stats.landed_tips_95th_percentile.unwrap_or(0.005),
                    "p25" => stats.landed_tips_25th_percentile.unwrap_or(0.0001),
                    _ => stats.landed_tips_50th_percentile.unwrap_or(0.0005),
                };
                let lamports = (tip_sol * 1_000_000_000.0) as u64;
                return lamports.max(10_000).min(10_000_000); // 0.00001 to 0.01 SOL bounds
            }
        }
    }
    100_000 // 0.0001 SOL fallback
}
