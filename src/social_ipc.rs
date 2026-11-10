use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use crate::{EngineState, RadarCandidate, SocialSignalMetadata};

pub const DEFAULT_SOCIAL_SOCKET_PATH: &str = "/tmp/dmne_social.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialSignal {
    pub mint: String,
    pub channel_id: i64,
    pub channel_title: String,
    pub message_timestamp_utc: i64,
    pub source: String,
}

pub fn spawn_social_ipc_receiver(state: Arc<EngineState>, socket_path: &str) {
    let path = socket_path.to_string();
    tokio::spawn(async move {
        let mut backoff_secs = 3u64;
        loop {
            match UnixStream::connect(&path).await {
                Ok(stream) => {
                    println!("[SOCIAL IPC] Successfully connected to social daemon UDS at {}", path);
                    backoff_secs = 3;
                    let mut reader = BufReader::new(stream).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(signal) = serde_json::from_str::<SocialSignal>(trimmed) {
                            println!(
                                "[SOCIAL IPC] Ingested social signal for mint {} from channel '{}', prioritizing in candidate radar queue",
                                signal.mint, signal.channel_title
                            );
                            let candidate = RadarCandidate {
                                symbol: format!("CALL:{}", signal.channel_title),
                                token_mint: signal.mint.clone(),
                                dex_id: "pumpfun".to_string(),
                                liquidity_usd: 35000.0,
                                bonding_curve_pct: 10,
                                dev_holding_pct: 0.0,
                                mint_authority_revoked: true,
                                freeze_authority_revoked: true,
                                cu_consumed: 50000,
                                social_signal: Some(SocialSignalMetadata {
                                    channel_id: signal.channel_id,
                                    channel_title: signal.channel_title.clone(),
                                    message_timestamp_utc: signal.message_timestamp_utc,
                                    source: signal.source.clone(),
                                    caller_reputation_score: 50.0,
                                }),
                            };

                            let mut g = state.write().await;
                            // Check if already traded or already active
                            let already_active = g.active_positions.iter().any(|p| p.token_mint == signal.mint);
                            let already_banned = g.banned_tokens.contains(&signal.mint);
                            if !already_active && !already_banned {
                                if !g.candidate_queue.iter().any(|c| c.token_mint == signal.mint) {
                                    g.candidate_queue.insert(0, candidate.clone());
                                    if g.candidate_queue.len() > 30 {
                                        g.candidate_queue.truncate(30);
                                    }
                                }
                                if !g.radar_candidates.iter().any(|c| c.token_mint == signal.mint) {
                                    g.radar_candidates.insert(0, candidate);
                                    if g.radar_candidates.len() > 15 {
                                        g.radar_candidates.truncate(15);
                                    }
                                }
                            }
                        }
                    }
                    println!("[SOCIAL IPC] Social daemon UDS connection closed. Reconnecting in {}s...", backoff_secs);
                }
                Err(e) => {
                    // Suppress connection failure spam while socket is offline
                    if backoff_secs == 3 {
                        println!("[SOCIAL IPC] Waiting for social daemon socket {} ({}). Retrying...", path, e);
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(30);
        }
    });
}

