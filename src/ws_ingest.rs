// DMNE QUANT // DIRECT SOLANA WEBSOCKET INGESTION DAEMON
// Sub-second block event listener for Pump.fun and Raydium programs

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

pub const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

#[derive(Debug, Clone)]
pub struct OnChainProgramEvent {
    pub signature: String,
    pub slot: u64,
    pub log_snippet: String,
}

/// Spawns an asynchronous WebSocket listener subscribing to program logs
pub async fn start_program_logs_stream(
    ws_rpc_url: &str,
    program_id: &str,
    tx_sender: mpsc::Sender<OnChainProgramEvent>,
) {
    let ws_url = ws_rpc_url.to_string();
    let prog = program_id.to_string();

    tokio::spawn(async move {
        loop {
            match connect_async(&ws_url).await {
                Ok((ws_stream, _)) => {
                    println!("[WS STREAM] Connected to Solana WebSocket for {}", prog);
                    let (mut write, mut read) = ws_stream.split();

                    let sub_request = json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "logsSubscribe",
                        "params": [
                            {"mentions": [prog]},
                            {"commitment": "processed"}
                        ]
                    });

                    if let Err(e) = write.send(Message::Text(sub_request.to_string())).await {
                        eprintln!("[WS ERROR] Handshake failed: {}", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                        continue;
                    }

                    while let Some(msg_result) = read.next().await {
                        if let Ok(Message::Text(text)) = msg_result {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                                if let Some(params) = v.get("params") {
                                    if let Some(result) = params.get("result") {
                                        let sig = result.get("value")
                                            .and_then(|val| val.get("signature"))
                                            .and_then(|s| s.as_str())
                                            .unwrap_or_default()
                                            .to_string();
                                        let slot = result.get("context")
                                            .and_then(|c| c.get("slot"))
                                            .and_then(|s| s.as_u64())
                                            .unwrap_or(0);

                                        let _ = tx_sender.send(OnChainProgramEvent {
                                            signature: sig,
                                            slot,
                                            log_snippet: "ProgramLogCommit".to_string(),
                                        }).await;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[WS ERROR] Transport reconnecting: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
                }
            }
        }
    });
}
