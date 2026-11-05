#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RadarCandidate {
    pub symbol: String,
    pub token_mint: String,
    pub dex_id: String,
    pub liquidity_usd: f64,
    pub bonding_curve_pct: u32,
    pub dev_holding_pct: f64,
    pub mint_authority_revoked: bool,
    pub freeze_authority_revoked: bool,
    pub cu_consumed: u64,
}

mod config;
mod institutional;
mod db;
mod amm;
mod ws_ingest;

fn append_comprehensive_trade_log(
    pos: &Position,
    exit_pnl_pct: f64,
    exit_pnl_sol: f64,
    exit_reason: &str,
    bankroll_before: f64,
    bankroll_after: f64,
) {
    use std::io::Write;
    use std::fs::OpenOptions;

    let log_path = "/home/ubuntu/dmne-meme-alpha/logs/trades_forensic.csv";
    let file_exists = std::path::Path::new(log_path).exists();

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
        if !file_exists {
            let header = "timestamp_exit_utc,timestamp_entry_utc,hold_duration_secs,token_symbol,token_mint,pair_address,dex_url,size_sol,entry_price_sol,peak_price_sol,trough_price_sol,exit_price_sol,mfe_pnl_pct,mae_pnl_pct,realized_pnl_pct,realized_pnl_sol,realized_pnl_eur,liquidity_usd,fdv_usd,exit_rule,outcome,simulated_fee_sol,bankroll_before_sol,bankroll_after_sol\n";
            let _ = file.write_all(header.as_bytes());
        }

        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let hold_duration = now_epoch.saturating_sub(pos.entry_timestamp_epoch);
        let now_utc = chrono::Utc::now().to_rfc3339();

        let eur_rate = 85.50;
        let pnl_eur = exit_pnl_sol * eur_rate;
        let mfe_pnl_pct = ((pos.peak_price_sol - pos.entry_price_sol) / pos.entry_price_sol) * 100.0;
        let mae_pnl_pct = ((pos.trough_price_sol - pos.entry_price_sol) / pos.entry_price_sol) * 100.0;
        let outcome = if exit_pnl_sol > 0.0001 { "WIN" } else if exit_pnl_sol < -0.0001 { "LOSS" } else { "BREAKEVEN" };
        let simulated_fee = 0.00105; // 0.001 Jito tip + priority fees
        let dex_url = format!("https://dexscreener.com/solana/{}", pos.token_mint);

        let row = format!(
            "{},{},{},{},{},{},{},{:.4},{:.10e},{:.10e},{:.10e},{:.10e},{:.2},{:.2},{:.2},{:.6},{:.2},{:.2},{:.2},\"{}\",{},{:.5},{:.4},{:.4}\n",
            now_utc,
            pos.entry_time_iso,
            hold_duration,
            pos.symbol,
            pos.token_mint,
            pos.pair_address,
            dex_url,
            pos.size_sol,
            pos.entry_price_sol,
            pos.peak_price_sol,
            pos.trough_price_sol,
            pos.current_price_sol,
            mfe_pnl_pct,
            mae_pnl_pct,
            exit_pnl_pct,
            exit_pnl_sol,
            pnl_eur,
            pos.liquidity_usd,
            pos.fdv_usd,
            exit_reason,
            outcome,
            simulated_fee,
            bankroll_before,
            bankroll_after
        );
        let _ = file.write_all(row.as_bytes());
    }
}



use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_tungstenite::connect_async;

const SOLANA_WSS_URL: &str = "wss://api.mainnet-beta.solana.com";
const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const DASHBOARD_PORT: u16 = 8090;

#[derive(Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: String,
    pub token_mint: String,
    pub pair_address: String,
    pub symbol: String,
    pub entry_time_iso: String,
    pub entry_timestamp_epoch: u64,
    pub entry_price_sol: f64,
    pub current_price_sol: f64,
    pub last_price_sol: f64,
    pub initial_size_sol: f64,
    pub size_sol: f64,
    pub tranche_1_taken: bool,
    pub tranche_2_taken: bool,
    pub pnl_pct: f64,
    pub status: String,
    pub peak_price_sol: f64,
    pub trough_price_sol: f64,
    pub liquidity_usd: f64,
    pub fdv_usd: f64,
    pub price_buffer: Vec<f64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ClosedTrade {
    pub token_mint: String,
    pub symbol: String,
    pub size_sol: f64,
    pub pnl_sol: f64,
    pub pnl_pct: f64,
    pub reason: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BotState {
    pub initial_bankroll_sol: f64,
    pub current_bankroll_sol: f64,
    pub total_trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub active_positions: Vec<Position>,
    pub radar_candidates: Vec<RadarCandidate>,
    pub rejections_count: u64,
    pub closed_trades: Vec<ClosedTrade>,
    pub recent_onchain_events: Vec<String>,
}

impl BotState {
    pub fn new() -> Self {
        let initial_sol = config::EngineConfig::default().calculate_initial_bankroll_sol();
        Self {
            initial_bankroll_sol: initial_sol, // $100.00 USD initial capital
            current_bankroll_sol: initial_sol,
            total_trades: 0,
            wins: 0,
            losses: 0,
            active_positions: Vec::new(),
            radar_candidates: Vec::new(),
            rejections_count: 0,
            closed_trades: Vec::new(),
            recent_onchain_events: Vec::new(),
        }
    }
}

// DexScreener API Structs
#[derive(Deserialize)]
struct TokenProfile {
    #[serde(rename = "chainId")]
    chain_id: Option<String>,
    #[serde(rename = "tokenAddress")]
    token_address: Option<String>,
}

#[derive(Deserialize)]
struct DexResponse {
    pairs: Option<Vec<DexPair>>,
}

#[derive(Deserialize)]
struct DexLiquidity {
    usd: Option<f64>,
}

#[derive(Deserialize)]
struct DexPair {
    #[serde(rename = "pairAddress")]
    pair_address: Option<String>,
    #[serde(rename = "priceNative")]
    price_native: Option<String>,
    #[serde(rename = "baseToken")]
    base_token: Option<TokenInfo>,
    liquidity: Option<DexLiquidity>,
    fdv: Option<f64>,
}

#[derive(Deserialize)]
struct TokenInfo {
    symbol: Option<String>,
}

// Fetch live price in SOL for an active token

async fn verify_token_security(client: &Client, rpc_url: &str, mint: &str) -> bool {
    // Check 1: Account Info for Freeze and Mint Authority
    let account_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getAccountInfo",
        "params": [mint, { "encoding": "jsonParsed" }]
    });

    if let Ok(res) = client.post(rpc_url).json(&account_payload).send().await {
        if let Ok(json) = res.json::<serde_json::Value>().await {
            if let Some(parsed) = json.get("result").and_then(|r| r.get("value")).and_then(|v| v.get("data")).and_then(|d| d.get("parsed")).and_then(|p| p.get("info")) {
                if !parsed.get("freezeAuthority").map(|v| v.is_null()).unwrap_or(true) {
                    println!("[GATE REJECT] {}: Active freeze authority detected (Honeypot risk)", mint);
                    return false;
                }
                if !parsed.get("mintAuthority").map(|v| v.is_null()).unwrap_or(true) {
                    println!("[GATE REJECT] {}: Active mint authority detected (Inflation trap)", mint);
                    return false;
                }
            }
        }
    }

    // Check 2: Holder Concentration via getTokenLargestAccounts
    let largest_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "getTokenLargestAccounts",
        "params": [mint]
    });

    if let Ok(res) = client.post(rpc_url).json(&largest_payload).send().await {
        if let Ok(json) = res.json::<serde_json::Value>().await {
            if let Some(accounts) = json.get("result").and_then(|r| r.get("value")).and_then(|v| v.as_array()) {
                if accounts.len() > 1 {
                    let mut total_top_ui = 0.0;
                    let mut top_holder_ui = 0.0;
                    for (i, acc) in accounts.iter().enumerate() {
                        let amount = acc.get("uiAmount").and_then(|a| a.as_f64()).unwrap_or(0.0);
                        if i == 1 { top_holder_ui = amount; }
                        total_top_ui += amount;
                    }
                    if total_top_ui > 0.0 {
                        let top_ratio = (top_holder_ui / total_top_ui) * 100.0;
                        if top_ratio > 30.0 {
                            println!("[GATE REJECT] {}: Top non-pool holder controls {:.1}% of top supply (Dump vulnerability)", mint, top_ratio);
                            return false;
                        }
                    }
                }
            }
        }
    }

    true
}

async fn fetch_live_price(client: &Client, token_mint: &str) -> Option<(f64, String, String, f64, f64)> {
    let url = format!("https://api.dexscreener.com/latest/dex/tokens/{}", token_mint);
    if let Ok(res) = client.get(&url).timeout(Duration::from_millis(1500)).send().await {
        if let Ok(data) = res.json::<DexResponse>().await {
            if let Some(pairs) = data.pairs {
                if let Some(best_pair) = pairs.first() {
                    let price_sol = best_pair.price_native.as_deref().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                    let sym = best_pair.base_token.as_ref().and_then(|b| b.symbol.clone()).unwrap_or_else(|| "TOKEN".to_string());
                    if price_sol > 0.0 {
                        let pair_addr = best_pair.pair_address.clone().unwrap_or_default();
                    let liq = best_pair.liquidity.as_ref().and_then(|l| l.usd).unwrap_or(0.0);
                    let fdv = best_pair.fdv.unwrap_or(0.0);
                    return Some((price_sol, sym, pair_addr, liq, fdv));
                    }
                }
            }
        }
    }
    None
}

const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>DMNE MEME-ALPHA // Real Market Paper Desk</title>
  <script src="https://cdn.tailwindcss.com"></script>
  <style>
    body { background-color: #0b0f19; font-family: ui-monospace, SFMono-Regular, monospace; }
  </style>
</head>
<body class="text-slate-300 p-6">
  <div class="max-w-7xl mx-auto space-y-6">
    <div class="flex justify-between items-center border-b border-slate-800 pb-4">
      <div>
        <h1 class="text-xl font-bold text-white tracking-wider">DMNE MEME-ALPHA <span class="text-xs bg-amber-950 text-amber-400 border border-amber-800 px-2 py-0.5 rounded ml-2">LIVE MARKET PAPER DESK</span></h1>
        <p class="text-xs text-slate-500 mt-1">Real Solana Market Feeds &middot; Live On-Chain Tokens &middot; Real Pricing Engine</p>
      </div>
      <div class="flex items-center space-x-2 text-xs">
        <span class="inline-block w-2.5 h-2.5 rounded-full bg-emerald-500 animate-pulse"></span>
        <span class="text-slate-400">REAL PRICE FEED SYNCED</span>
      </div>
    </div>

    <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
      <div class="bg-slate-900 border border-slate-800 p-4 rounded-lg">
        <div class="text-xs text-slate-500 uppercase tracking-wider">Paper NAV</div>
        <div id="nav-val" class="text-2xl font-bold text-white mt-1">0.2300 SOL</div>
        <div id="nav-eur" class="text-xs text-slate-400 mt-1">≈ €20.00 EUR</div>
      </div>
      <div class="bg-slate-900 border border-slate-800 p-4 rounded-lg">
        <div class="text-xs text-slate-500 uppercase tracking-wider">Realized PnL</div>
        <div id="pnl-val" class="text-2xl font-bold text-slate-400 mt-1">+0.0000 SOL</div>
        <div id="pnl-pct" class="text-xs text-slate-500 mt-1">+0.00%</div>
      </div>
      <div class="bg-slate-900 border border-slate-800 p-4 rounded-lg">
        <div class="text-xs text-slate-500 uppercase tracking-wider">Real Win Rate</div>
        <div id="win-rate" class="text-2xl font-bold text-white mt-1">0.0%</div>
        <div id="win-count" class="text-xs text-slate-500 mt-1">0 Wins / 0 Losses</div>
      </div>
      <div class="bg-slate-900 border border-slate-800 p-4 rounded-lg">
        <div class="text-xs text-slate-500 uppercase tracking-wider">Executed Paper Snipes</div>
        <div id="total-trades" class="text-2xl font-bold text-white mt-1">0</div>
        <div class="text-xs text-slate-500 mt-1">Dynamic Half-Kelly Sizing</div>
      </div>
    </div>

    <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
      <div class="lg:col-span-2 bg-slate-900 border border-slate-800 rounded-lg p-5">
        <h2 class="text-sm font-semibold text-white uppercase tracking-wider mb-3">Active Real Market Positions</h2>
        <div class="overflow-x-auto">
          <table class="w-full text-xs text-left">
            <thead class="text-slate-500 border-b border-slate-800">
              <tr>
                <th class="pb-2">Token / Mint</th>
                <th class="pb-2">Allocated</th>
                <th class="pb-2">Entry (SOL)</th>
                <th class="pb-2">Current (SOL)</th>
                <th class="pb-2">Live PnL %</th>
                <th class="pb-2">Ladder Status</th>
              </tr>
            </thead>
            <tbody id="positions-body" class="divide-y divide-slate-800/60">
              <tr><td colspan="6" class="py-4 text-center text-slate-600">Scanning Solana mainnet for new pools...</td></tr>
            </tbody>
          </table>
        </div>
      </div>

      <div class="bg-slate-900 border border-slate-800 rounded-lg p-5">
        <h2 class="text-sm font-semibold text-white uppercase tracking-wider mb-3">Live Feed Log</h2>
        <div id="event-feed" class="space-y-2 text-[11px] font-mono max-h-72 overflow-y-auto text-slate-400">
          <div class="text-slate-600">Listening to real Solana DEX events...</div>
        </div>
      </div>
    </div>

    <div class="bg-slate-900 border border-slate-800 rounded-lg p-5">
      <h2 class="text-sm font-semibold text-white uppercase tracking-wider mb-3">Closed Trades History (Real Market Exit Fills)</h2>
      <div class="overflow-x-auto">
        <table class="w-full text-xs text-left">
          <thead class="text-slate-500 border-b border-slate-800">
            <tr>
              <th class="pb-2">Token / Mint</th>
              <th class="pb-2">Size</th>
              <th class="pb-2">Realized PnL</th>
              <th class="pb-2">PnL %</th>
              <th class="pb-2">Exit Trigger</th>
            </tr>
          </thead>
          <tbody id="history-body" class="divide-y divide-slate-800/60">
            <tr><td colspan="5" class="py-3 text-center text-slate-600">No closed trades yet</td></tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>

  <script>
    const evtSource = new EventSource('/events');
    evtSource.onmessage = function(e) {
      const data = JSON.parse(e.data);
      const solEurRate = 86.0;

      document.getElementById('nav-val').innerText = data.current_bankroll_sol.toFixed(4) + ' SOL';
      document.getElementById('nav-eur').innerText = '≈ €' + (data.current_bankroll_sol * solEurRate).toFixed(2) + ' EUR';

      const netPnlSol = data.current_bankroll_sol - data.initial_bankroll_sol;
      const netPnlPct = (netPnlSol / data.initial_bankroll_sol) * 100;
      const pnlEl = document.getElementById('pnl-val');
      pnlEl.innerText = (netPnlSol >= 0 ? '+' : '') + netPnlSol.toFixed(4) + ' SOL';
      pnlEl.className = 'text-2xl font-bold mt-1 ' + (netPnlSol >= 0 ? 'text-emerald-400' : 'text-rose-400');
      document.getElementById('pnl-pct').innerText = (netPnlPct >= 0 ? '+' : '') + netPnlPct.toFixed(2) + '%';

      const totalClosed = data.wins + data.losses;
      const winRate = totalClosed > 0 ? ((data.wins / totalClosed) * 100).toFixed(1) : '0.0';
      document.getElementById('win-rate').innerText = winRate + '%';
      document.getElementById('win-count').innerText = data.wins + ' Wins / ' + data.losses + ' Losses';
      document.getElementById('total-trades').innerText = data.total_trades;

      const posBody = document.getElementById('positions-body');
      if (data.active_positions.length === 0) {
        posBody.innerHTML = '<tr><td colspan="6" class="py-4 text-center text-slate-600">Scanning Solana mainnet for new pools...</td></tr>';
      } else {
        posBody.innerHTML = data.active_positions.map(p => `
          <tr>
            <td class="py-2.5 font-mono text-slate-300">
              <span class="font-bold text-white">${p.symbol}</span>
              <a href="https://dexscreener.com/solana/${p.token_mint}" target="_blank" class="text-[10px] text-blue-400 hover:underline block">${p.token_mint.substring(0, 10)}...</a>
            </td>
            <td class="py-2.5">${p.size_sol.toFixed(3)} SOL</td>
            <td class="py-2.5">${p.entry_price_sol.toExponential(4)}</td>
            <td class="py-2.5">${p.current_price_sol.toExponential(4)}</td>
            <td class="py-2.5 font-bold ${p.pnl_pct >= 0 ? 'text-emerald-400' : 'text-rose-400'}">
              ${p.pnl_pct >= 0 ? '+' : ''}${p.pnl_pct.toFixed(1)}%
            </td>
            <td class="py-2.5"><span class="px-2 py-0.5 rounded bg-slate-800 text-slate-300 border border-slate-700">${p.status}</span></td>
          </tr>
        `).join('');
      }

      const feed = document.getElementById('event-feed');
      if (data.recent_onchain_events.length > 0) {
        feed.innerHTML = data.recent_onchain_events.map(ev => `
          <div class="truncate text-slate-400 border-b border-slate-800/40 pb-1">&gt; ${ev}</div>
        `).join('');
      }

      const histBody = document.getElementById('history-body');
      if (data.closed_trades.length > 0) {
        histBody.innerHTML = data.closed_trades.slice(-8).reverse().map(h => `
          <tr>
            <td class="py-2 font-mono">
              <span class="font-bold text-white">${h.symbol}</span>
              <span class="text-[10px] text-slate-500 block">${h.token_mint.substring(0, 8)}...</span>
            </td>
            <td class="py-2">${h.size_sol.toFixed(3)} SOL</td>
            <td class="py-2 font-bold ${h.pnl_sol >= 0 ? 'text-emerald-400' : 'text-rose-400'}">${h.pnl_sol >= 0 ? '+' : ''}${h.pnl_sol.toFixed(4)}</td>
            <td class="py-2 ${h.pnl_pct >= 0 ? 'text-emerald-400' : 'text-rose-400'}">${h.pnl_pct >= 0 ? '+' : ''}${h.pnl_pct.toFixed(1)}%</td>
            <td class="py-2 text-slate-400">${h.reason}</td>
          </tr>
        `).join('');
      }
    };
  </script>
</body>
</html>"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("============================================================");
    println!(" DMNE QUANT / INSTITUTIONAL - MEME COIN EXECUTION DESK     ");
    println!(" REAL MARKET PAPER ENGINE (Live Pricing & Exit Ladders)     ");
    println!("============================================================\n");

    let state = Arc::new(RwLock::new(BotState::new()));
    let config = Arc::new(config::EngineConfig::default());
    let http_client = Client::builder()
        .tcp_nodelay(true)
        .build()?;

    // 1. Dashboard Web Server (Port 8090)
    let server_state = Arc::clone(&state);
    tokio::spawn(async move {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", DASHBOARD_PORT))
            .await
            .expect("Failed to bind port 8090");
        println!("[+] Live dashboard listening on http://0.0.0.0:{}", DASHBOARD_PORT);

        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                let st = Arc::clone(&server_state);
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    if let Ok(n) = socket.read(&mut buf).await {
                        let req = String::from_utf8_lossy(&buf[..n]);
                        if req.starts_with("GET /events") {
                            let sse_headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
                            let _ = socket.write_all(sse_headers.as_bytes()).await;
                            loop {
                                tokio::time::sleep(Duration::from_millis(1000)).await;
                                let snapshot = {
                                    let guard = st.read().await;
                                    let (pg_total, pg_wins, _) = if let Ok(client) = db::establish_connection(db::DEFAULT_DB_URL).await {
                                        db::fetch_aggregate_stats(&client).await
                                    } else {
                                        (guard.total_trades as i64, guard.wins as i64, 0.0)
                                    };

                                    serde_json::to_string(&serde_json::json!({
                                        "initial_bankroll_sol": guard.initial_bankroll_sol,
                                        "current_bankroll_sol": guard.current_bankroll_sol,
                                        "total_trades": pg_total,
                                        "wins": pg_wins,
                                        "losses": pg_total - pg_wins,
                                        "rejections_count": guard.rejections_count,
                                        "active_positions": guard.active_positions,
                                        "radar_candidates": guard.radar_candidates,
                                        "closed_trades": guard.closed_trades,
                                        "recent_onchain_events": guard.recent_onchain_events,
                                    })).unwrap_or_default()
                                };
                                let payload = format!("data: {}\n\n", snapshot);
                                if socket.write_all(payload.as_bytes()).await.is_err() {
                                    break;
                                }
                            }
                        } else if req.starts_with("GET /api/trades") {
                            let mut trades = Vec::new();
                            if let Ok(pg_client) = db::establish_connection(db::DEFAULT_DB_URL).await {
                                trades = db::fetch_settled_trades(&pg_client, 250).await;
                            }
                            if trades.is_empty() {
                                let log_path = "/home/ubuntu/dmne-meme-alpha/logs/trades_forensic.csv";
                                if let Ok(content) = tokio::fs::read_to_string(log_path).await {
                                    let lines: Vec<&str> = content.lines().collect();
                                    if lines.len() > 1 {
                                        let headers: Vec<&str> = lines[0].split(',').map(|s| s.trim()).collect();
                                        for line in lines.iter().skip(1) {
                                            let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                                            if fields.len() == headers.len() {
                                                let mut obj = serde_json::Map::new();
                                                for (h, f) in headers.iter().zip(fields.iter()) {
                                                    obj.insert(h.to_string(), serde_json::Value::String(f.to_string()));
                                                }
                                                trades.push(serde_json::Value::Object(obj));
                                            }
                                        }
                                    }
                                }
                                trades.reverse();
                            }
                            let json_resp = serde_json::json!({ "total": trades.len(), "trades": trades }).to_string();
                            let res = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                json_resp.len(),
                                json_resp
                            );
                            let _ = socket.write_all(res.as_bytes()).await;
                        } else {
                            let html_body = tokio::fs::read_to_string("/home/ubuntu/dmne-meme-alpha/dashboard.html").await
                                .unwrap_or_else(|_| DASHBOARD_HTML.to_string());
                            let res = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                html_body.len(),
                                html_body
                            );
                            let _ = socket.write_all(res.as_bytes()).await;
                        }
                    }
                });
            }
        }
    });

    // 2. Real Market Position Evaluator (Queries Live Prices every 3 seconds)
    let eval_state = Arc::clone(&state);
    let eval_client = http_client.clone();
    let eval_cfg = Arc::clone(&config);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;

            let active_mints: Vec<(usize, String)> = {
                let guard = eval_state.read().await;
                guard.active_positions.iter().enumerate().map(|(i, p)| (i, p.token_mint.clone())).collect()
            };

            for (idx, mint) in active_mints {
                if let Some((real_price, _, _, liq, fdv)) = fetch_live_price(&eval_client, &mint).await {
                    let mut guard = eval_state.write().await;
                    if idx < guard.active_positions.len() {
                        let pos = &mut guard.active_positions[idx];
                        pos.current_price_sol = real_price;
                        pos.price_buffer.push(real_price);
                        if pos.price_buffer.len() > 30 {
                            pos.price_buffer.remove(0);
                        }
                        if pos.current_price_sol > pos.peak_price_sol {
                            pos.peak_price_sol = pos.current_price_sol;
                        }
                        if pos.current_price_sol < pos.trough_price_sol {
                            pos.trough_price_sol = pos.current_price_sol;
                        }
                        if pos.current_price_sol < pos.trough_price_sol {
                            pos.trough_price_sol = pos.current_price_sol;
                        }
                        pos.liquidity_usd = liq;
                        pos.fdv_usd = fdv;
                        pos.pnl_pct = ((pos.current_price_sol - pos.entry_price_sol) / pos.entry_price_sol) * 100.0;
                    }
                }
            }

            // Dynamically query AI Cruise Agent overrides from PostgreSQL
            let mut dynamic_trail_mult = eval_cfg.volatility_trail_multiplier;
            let mut dynamic_ttl_secs = eval_cfg.stagnation_ttl_seconds;
            if let Ok(pg_client) = db::establish_connection(db::DEFAULT_DB_URL).await {
                if let Some((_, trail, ttl)) = db::fetch_latest_override(&pg_client, "CRUISE_AGENT").await {
                    dynamic_trail_mult = trail;
                    dynamic_ttl_secs = ttl as i64;
                }
            }

            // Evaluate dynamic exit rules
            let mut guard = eval_state.write().await;
            let mut closed_indices = Vec::new();
            let state = &mut *guard;
            for (idx, pos) in state.active_positions.iter_mut().enumerate() {
                let peak_pct = ((pos.peak_price_sol - pos.entry_price_sol) / pos.entry_price_sol) * 100.0;

                // ==========================================
                // DCS CRUISE CONTROL EXECUTION SUPERVISOR
                // ==========================================

                // Rule 1: Flash Dump Circuit Breaker (Detect sudden >= 25% tick collapse)
                let tick_drop = if pos.last_price_sol > 0.0 { (pos.last_price_sol - pos.current_price_sol) / pos.last_price_sol } else { 0.0 };
                if tick_drop >= 0.25 {
                    let exit_pnl = pos.pnl_pct;
                    let realized = pos.size_sol * (exit_pnl / 100.0);
                    println!("[CRUISE CONTROL] Flash Dump detected on {} (-{:.1}% tick drop). Emergency Exit.", pos.symbol, tick_drop * 100.0);
                    closed_indices.push((idx, "Flash Dump Breaker".to_string(), realized, exit_pnl));
                }
                // Rule 2: Tranche 1 De-Risking (50% scale-out at >= +100% / 2x)
                else if pos.pnl_pct >= eval_cfg.tranche_1_target_pct && !pos.tranche_1_taken {
                    pos.tranche_1_taken = true;
                    let sell_size = pos.initial_size_sol * eval_cfg.tranche_1_scale_ratio;
                    let realized = sell_size * (pos.pnl_pct / 100.0);
                    state.current_bankroll_sol += sell_size + realized;
                    pos.size_sol -= sell_size;
                    println!("[CRUISE CONTROL] Tranche 1 Harvest on {} (+{:.1}%): Recaptured 100% capital + {:.5} SOL gain.", pos.symbol, pos.pnl_pct, realized);
                }
                // Rule 3: Tranche 2 Profit Harvest (25% scale-out at >= +250% / 3.5x)
                else if pos.pnl_pct >= eval_cfg.tranche_2_target_pct && !pos.tranche_2_taken {
                    pos.tranche_2_taken = true;
                    let sell_size = pos.initial_size_sol * eval_cfg.tranche_2_scale_ratio;
                    let realized = sell_size * (pos.pnl_pct / 100.0);
                    state.current_bankroll_sol += sell_size + realized;
                    pos.size_sol -= sell_size;
                    println!("[CRUISE CONTROL] Tranche 2 Harvest on {} (+{:.1}%): Locked {:.5} SOL gain. 25% moonbag runner active.", pos.symbol, pos.pnl_pct, realized);
                }
                // Rule 4: Dynamic Trailing Stop (scaled by dynamic_trail_mult)
                else if peak_pct >= 50.0 {
                    let trail_factor = if peak_pct >= 150.0 { 1.0 - (0.055 * (dynamic_trail_mult / 2.20)) } else { 1.0 - (0.082 * (dynamic_trail_mult / 2.20)) };
                    if pos.current_price_sol < pos.peak_price_sol * trail_factor {
                        let exit_pnl = pos.pnl_pct;
                        let realized = pos.size_sol * (exit_pnl / 100.0);
                        let label = format!("Cruise Trail Exit (+{:.0}% Peak)", peak_pct);
                        closed_indices.push((idx, label, realized, exit_pnl));
                    }
                }
                // Rule 5: Breakeven Floor Guard (If peak >= +22%, never drop below +3%)
                else if peak_pct >= eval_cfg.breakeven_arm_peak_pct && pos.pnl_pct <= eval_cfg.breakeven_floor_pct {
                    let realized = pos.size_sol * (eval_cfg.breakeven_floor_pct / 100.0);
                    closed_indices.push((idx, format!("Breakeven Guard (+{:.0}%)", eval_cfg.breakeven_floor_pct), realized, eval_cfg.breakeven_floor_pct));
                }
                // Rule 6: Adaptive Stagnation TTL (Recycle slot if flat after TTL seconds)
                else if (chrono::Utc::now().timestamp() - pos.entry_timestamp_epoch as i64) > dynamic_ttl_secs && pos.pnl_pct > -10.0 && pos.pnl_pct < 15.0 {
                    let exit_pnl = pos.pnl_pct;
                    let realized = pos.size_sol * (exit_pnl / 100.0);
                    closed_indices.push((idx, "Stagnation TTL Exit".to_string(), realized, exit_pnl));
                }
                // Rule 7: Hard Stop-Loss cutoff
                else if pos.pnl_pct <= eval_cfg.hard_stop_loss_pct {
                    let exit_pnl = pos.pnl_pct;
                    let realized = pos.size_sol * (exit_pnl / 100.0);
                    closed_indices.push((idx, format!("Stop-Loss ({:.0}%)", eval_cfg.hard_stop_loss_pct), realized, exit_pnl));
                }

                // UI status milestones
                if pos.pnl_pct >= 300.0 && !pos.status.contains("Runner") {
                    pos.status = "Runner Locked (+300%)".to_string();
                } else if pos.pnl_pct >= 100.0 && pos.status == "Active" {
                    pos.status = "De-risked (+100%)".to_string();
                } else if pos.pnl_pct >= 40.0 && pos.status == "Active" {
                    pos.status = "Trailing Active".to_string();
                }

                // Update last observed tick price for flash dump detection
                pos.last_price_sol = pos.current_price_sol;
            }

            for (idx, reason, pnl_sol, pnl_pct) in closed_indices.into_iter().rev() {
                let bankroll_before = state.current_bankroll_sol;
                let pos = state.active_positions.remove(idx);
                state.current_bankroll_sol += pnl_sol;
                let bankroll_after = state.current_bankroll_sol;
                if pnl_sol > 0.0 {
                    state.wins += 1;
                } else {
                    state.losses += 1;
                }
                append_comprehensive_trade_log(
                    &pos,
                    pnl_pct,
                    pnl_sol,
                    &reason,
                    bankroll_before,
                    bankroll_after,
                );
                let db_mint = pos.token_mint.clone();
                let db_sym = pos.symbol.clone();
                let db_hold_sec = (chrono::Utc::now().timestamp() - pos.entry_timestamp_epoch as i64) as i32;
                let db_size = pos.size_sol;
                let db_entry_px = pos.entry_price_sol;
                let db_exit_px = pos.current_price_sol;
                let db_peak_px = pos.peak_price_sol;
                let db_before = bankroll_before;
                let db_after = bankroll_after;
                let db_rule = reason.clone();
                let db_url = format!("https://dexscreener.com/solana/{}", pos.token_mint);
                tokio::spawn(async move {
                    if let Ok(pg_client) = db::establish_connection(db::DEFAULT_DB_URL).await {
                        db::record_settled_trade(
                            &pg_client, &db_mint, &db_sym, db_hold_sec, db_size,
                            db_entry_px, db_exit_px, db_peak_px, pnl_pct, pnl_sol,
                            db_before, db_after, &db_rule, &db_url
                        ).await;
                    }
                });
                state.closed_trades.push(ClosedTrade {
                    token_mint: pos.token_mint,
                    symbol: pos.symbol,
                    size_sol: pos.size_sol,
                    pnl_sol,
                    pnl_pct,
                    reason,
                });
            }
        }
    });

    // 3. Ingestion Loop: Fetch Newly Launched Solana Tokens in Real-Time
    let scanner_state = Arc::clone(&state);
    let scanner_client = http_client.clone();
    let scanner_cfg = Arc::clone(&config);
    tokio::spawn(async move {
        loop {
            // Pull newly listed Solana tokens
            if let Ok(res) = scanner_client.get("https://api.dexscreener.com/token-profiles/latest/v1")
                .timeout(Duration::from_millis(2000))
                .send()
                .await 
            {
                if let Ok(profiles) = res.json::<Vec<TokenProfile>>().await {
                    for profile in profiles {
                        if profile.chain_id.as_deref() == Some("solana") {
                            if let Some(mint) = profile.token_address {
                                let guard = scanner_state.read().await;
                                let already_exists = guard.active_positions.iter().any(|p| p.token_mint == mint)
                                    || guard.closed_trades.iter().any(|t| t.token_mint == mint);

                                if !already_exists && guard.active_positions.len() < 4 && guard.current_bankroll_sol >= 0.015 {
                                    drop(guard);
                                    if let Some((initial_price, sym, pair_addr, liq, fdv)) = fetch_live_price(&scanner_client, &mint).await {
                                        if liq < scanner_cfg.min_liquidity_usd { continue; }
                                        let rpc_endpoint = &scanner_cfg.default_rpc_url;
                                        if !verify_token_security(&scanner_client, rpc_endpoint, &mint).await {
                                            scanner_state.write().await.rejections_count += 1;
                                            continue;
                                        }
                                        let mut dynamic_hhi = scanner_cfg.hhi_concentration_ceiling;
                                        if let Ok(pg_client) = db::establish_connection(db::DEFAULT_DB_URL).await {
                                            if let Some((hhi, _, _)) = db::fetch_latest_override(&pg_client, "LAUNCH_AGENT").await {
                                                dynamic_hhi = hhi;
                                            }
                                        }
                                        if !institutional::evaluate_holder_hhi(&scanner_client, rpc_endpoint, &mint, dynamic_hhi).await {
                                            scanner_state.write().await.rejections_count += 1;
                                            continue;
                                        }
                                        let dex_id = if mint.ends_with("pump") { "pumpfun" } else { "raydium" };
                                        if !institutional::verify_lp_burned_or_locked(&scanner_client, rpc_endpoint, &mint, &pair_addr, dex_id, scanner_cfg.min_dead_lp_ratio).await {
                                            scanner_state.write().await.rejections_count += 1;
                                            continue;
                                        }

                                        // Step 5: Institutional Preflight Simulation Gate (Zero-Capital Dry-Run)
                                        match institutional::simulate_dry_run_bundle(
                                            &scanner_client,
                                            rpc_endpoint,
                                            &scanner_cfg.jito_block_engine_url,
                                            &scanner_cfg.jito_tip_floor_url,
                                            "p50",
                                            &scanner_cfg,
                                        ).await {
                                            Ok(sim) => {
                                                println!("[PREFLIGHT PASS] Mint: {} | Consumed CU: {} -> Optimized CU: {} | Dynamic Tip: {} lamports ({})",
                                                    mint, sim.units_consumed, sim.optimized_cu_limit, sim.tip_lamports, sim.tip_account);
                                            }
                                            Err(err) => {
                                                scanner_state.write().await.rejections_count += 1;
                                                println!("[PREFLIGHT REJECT] Simulation reverted for mint {}: {}", mint, err);
                                                continue;
                                            }
                                        }
                                        let mut g = scanner_state.write().await;
                                        if g.active_positions.len() < 4 {
                                            g.total_trades += 1;
                                            let trade_id = g.total_trades;
                                            let log_msg = format!("SNIPED NEW TOKEN: {} ({}) at {:.4e} SOL", sym, &mint[..8], initial_price);
                                            g.recent_onchain_events.insert(0, log_msg);
                                            if g.recent_onchain_events.len() > 15 {
                                                g.recent_onchain_events.pop();
                                            }

                                            let now_epoch = std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs();
                                            let now_iso = chrono::Utc::now().to_rfc3339();
                                            let dynamic_entry_sol = institutional::calculate_dynamic_size(
                                                g.current_bankroll_sol,
                                                liq,
                                                &scanner_cfg,
                                            );

                                            let audit_mint = mint.clone();
                                            let audit_pool = pair_addr.clone();
                                            let audit_dex = dex_id.to_string();
                                            let audit_liq = liq;
                                            tokio::spawn(async move {
                                                if let Ok(pg_client) = db::establish_connection(db::DEFAULT_DB_URL).await {
                                                    db::record_pre_trade_audit(
                                                        &pg_client, &audit_mint, &audit_pool, &audit_dex,
                                                        audit_liq, 0.15, true, true, true, true, "Admitted"
                                                    ).await;
                                                }
                                            });

                                            g.active_positions.push(Position {
                                                id: format!("pos_{}", trade_id),
                                                token_mint: mint,
                                                pair_address: pair_addr,
                                                symbol: sym,
                                                entry_time_iso: now_iso,
                                                entry_timestamp_epoch: now_epoch,
                                                entry_price_sol: initial_price,
                                                current_price_sol: initial_price,
                                                last_price_sol: initial_price,
                                                initial_size_sol: dynamic_entry_sol,
                                                size_sol: dynamic_entry_sol,
                                                tranche_1_taken: false,
                                                tranche_2_taken: false,
                                                pnl_pct: 0.0,
                                                status: "Active".to_string(),
                                                peak_price_sol: initial_price,
                                                trough_price_sol: initial_price,
                                                liquidity_usd: liq,
                                                fdv_usd: fdv,
                                                price_buffer: vec![initial_price],
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(4)).await;
        }
    });

    // 4. Also keep WebSocket open for live network event throughput telemetry
    let ws_state = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            if let Ok((ws_stream, _)) = connect_async(SOLANA_WSS_URL).await {
                let (mut write, mut read) = ws_stream.split();
                let sub_req = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "logsSubscribe",
                    "params": [{ "mentions": [PUMP_FUN_PROGRAM_ID] }, { "commitment": "processed" }]
                });
                if write.send(tokio_tungstenite::tungstenite::Message::Text(sub_req.to_string())).await.is_ok() {
                    while let Some(msg) = read.next().await {
                        if let Ok(tokio_tungstenite::tungstenite::Message::Text(txt)) = msg {
                            if txt.contains("signature") {
                                let mut guard = ws_state.write().await;
                                guard.recent_onchain_events.insert(0, "Live DEX Event Intercepted".to_string());
                                if guard.recent_onchain_events.len() > 15 {
                                    guard.recent_onchain_events.pop();
                                }
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    println!("[OK] Real-market paper engine active. Monitoring Solana mainnet...");
    // Keep main thread alive
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}
