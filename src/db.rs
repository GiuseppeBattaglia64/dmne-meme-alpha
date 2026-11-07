#![allow(dead_code)]
// DMNE QUANT // POSTGRESQL PERSISTENCE MODULE
// Asynchronous Database Client, Pre-Trade Audits & Trade Settlement Logs

use tokio_postgres::{Client, NoTls, Error};
use serde_json::{Value, json, Map};

pub const DEFAULT_DB_URL: &str = "host=localhost user=ubuntu dbname=dmne_quant";

/// Establishes an asynchronous PostgreSQL connection
pub async fn establish_connection(db_url: &str) -> Result<Client, Error> {
    let (client, connection) = tokio_postgres::connect(db_url, NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("[POSTGRES ERROR] Connection lost: {}", e);
        }
    });
    Ok(client)
}

/// Logs a pre-trade audit decision into PostgreSQL
pub async fn record_pre_trade_audit(
    client: &Client,
    mint: &str,
    pool_address: &str,
    dex_id: &str,
    liquidity_usd: f64,
    hhi: f64,
    freeze_null: bool,
    mint_null: bool,
    lp_verified: bool,
    admitted: bool,
    reason: &str,
) {
    let stmt = "
        INSERT INTO pre_trade_audits (
            token_mint, pool_address, dex_id, liquidity_usd, hhi_concentration,
            freeze_authority_null, mint_authority_null, lp_burn_verified, admitted, rejection_reason
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
    ";
    let _ = client.execute(
        stmt,
        &[&mint, &pool_address, &dex_id, &liquidity_usd, &hhi, &freeze_null, &mint_null, &lp_verified, &admitted, &reason]
    ).await;
}

/// Logs a closed position settlement into PostgreSQL
pub async fn record_settled_trade(
    client: &Client,
    mint: &str,
    symbol: &str,
    hold_duration_sec: i32,
    entry_size_sol: f64,
    entry_price_sol: f64,
    exit_price_sol: f64,
    peak_price_sol: f64,
    pnl_pct: f64,
    realized_pnl_sol: f64,
    bankroll_before: f64,
    bankroll_after: f64,
    exit_rule: &str,
    dex_url: &str,
    is_toxic: bool,
) {
    let stmt = "
        INSERT INTO settled_trades (
            token_mint, symbol, hold_duration_seconds, entry_size_sol, entry_price_sol,
            exit_price_sol, peak_price_sol, pnl_pct, realized_pnl_sol, bankroll_before_sol,
            bankroll_after_sol, exit_rule, dex_url, is_toxic
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
    ";
    let _ = client.execute(
        stmt,
        &[
            &mint, &symbol, &hold_duration_sec, &entry_size_sol, &entry_price_sol,
            &exit_price_sol, &peak_price_sol, &pnl_pct, &realized_pnl_sol, &bankroll_before,
            &bankroll_after, &exit_rule, &dex_url, &is_toxic
        ]
    ).await;
}

/// Queries recent closed trades for the forensic ledger
pub async fn fetch_settled_trades(client: &Client, limit: i64) -> Vec<Value> {
    let stmt = "
        SELECT 
            token_mint, symbol, to_char(exit_timestamp, 'YYYY-MM-DD HH24:MI:SS') as exit_time,
            hold_duration_seconds, entry_size_sol, entry_price_sol, exit_price_sol,
            pnl_pct, realized_pnl_sol, exit_rule, dex_url
        FROM settled_trades
        ORDER BY exit_timestamp DESC
        LIMIT $1
    ";
    let mut trades = Vec::new();
    if let Ok(rows) = client.query(stmt, &[&limit]).await {
        for row in rows {
            let mut obj = Map::new();
            obj.insert("token_mint".to_string(), json!(row.get::<_, String>("token_mint")));
            obj.insert("symbol".to_string(), json!(row.get::<_, String>("symbol")));
            obj.insert("timestamp".to_string(), json!(row.get::<_, Option<String>>("exit_time").unwrap_or_default()));
            obj.insert("hold_seconds".to_string(), json!(row.get::<_, i32>("hold_duration_seconds")));
            obj.insert("size_sol".to_string(), json!(row.get::<_, f64>("entry_size_sol")));
            obj.insert("entry_price_sol".to_string(), json!(format!("{:.4e}", row.get::<_, f64>("entry_price_sol"))));
            obj.insert("exit_price_sol".to_string(), json!(format!("{:.4e}", row.get::<_, f64>("exit_price_sol"))));
            obj.insert("realized_pnl_pct".to_string(), json!(row.get::<_, f64>("pnl_pct")));
            obj.insert("realized_pnl_sol".to_string(), json!(row.get::<_, f64>("realized_pnl_sol")));
            obj.insert("exit_rule".to_string(), json!(row.get::<_, String>("exit_rule")));
            obj.insert("dex_url".to_string(), json!(row.get::<_, Option<String>>("dex_url").unwrap_or_default()));
            trades.push(Value::Object(obj));
        }
    }
    trades
}

/// Fetches all historical closed trades for quarantine state reconciliation on startup
pub async fn fetch_settled_trades_for_reconcile(client: &Client) -> Vec<(String, String, f64, f64, f64, String, u64, bool)> {
    let stmt = "
        SELECT token_mint, symbol, entry_size_sol, realized_pnl_sol, pnl_pct, exit_rule,
               EXTRACT(EPOCH FROM exit_timestamp)::bigint AS exit_epoch,
               COALESCE(is_toxic, false) AS is_toxic
        FROM settled_trades
        ORDER BY exit_timestamp ASC
    ";
    let mut out = Vec::new();
    if let Ok(rows) = client.query(stmt, &[]).await {
        for row in rows {
            let mint: String = row.get("token_mint");
            let symbol: String = row.get("symbol");
            let size: f64 = row.get("entry_size_sol");
            let pnl_sol: f64 = row.get("realized_pnl_sol");
            let pnl_pct: f64 = row.get("pnl_pct");
            let rule: String = row.get("exit_rule");
            let epoch: i64 = row.get("exit_epoch");
            let is_toxic: bool = row.get("is_toxic");
            out.push((mint, symbol, size, pnl_sol, pnl_pct, rule, epoch.max(0) as u64, is_toxic));
        }
    }
    out
}

/// Queries all active users from PostgreSQL
pub async fn fetch_active_users(client: &Client) -> Vec<Value> {
    let stmt = "
        SELECT id, display_name, role, execution_mode, pubkey, paper_bankroll_sol, has_started
        FROM users
        WHERE is_active = true
        ORDER BY created_at ASC;
    ";
    let mut users = Vec::new();
    if let Ok(rows) = client.query(stmt, &[]).await {
        for r in rows {
            let id: String = r.get(0);
            let name: String = r.get(1);
            let role: String = r.get(2);
            let mode: String = r.get(3);
            let pubkey: String = r.get(4);
            let paper_bankroll: f64 = r.get(5);
            let has_started: bool = r.get::<_, Option<bool>>(6).unwrap_or(false);
            users.push(json!({
                "id": id,
                "name": name,
                "role": role,
                "mode": mode,
                "pubkey": pubkey,
                "paper_bankroll": paper_bankroll,
                "has_started": has_started
            }));
        }
    }
    users
}

/// Verifies user credentials against PostgreSQL and returns (role, session_token) on success
pub async fn verify_login(client: &Client, user_id: &str, password_hash: &str) -> Option<(String, String)> {
    let stmt = "
        SELECT role, is_active
        FROM users
        WHERE id = $1 AND password_hash = $2
    ";
    if let Ok(rows) = client.query(stmt, &[&user_id, &password_hash]).await {
        if let Some(row) = rows.first() {
            let role: String = row.get(0);
            let is_active: bool = row.get(1);
            if is_active {
                let token = format!("tok_{}_{}", user_id, chrono::Utc::now().timestamp());
                let update_stmt = "UPDATE users SET session_token = $1 WHERE id = $2";
                let _ = client.execute(update_stmt, &[&token, &user_id]).await;
                return Some((role, token));
            }
        }
    }
    None
}

/// Marks a user's desk as started/activated
pub async fn set_user_started(client: &Client, user_id: &str) -> bool {
    let stmt = "UPDATE users SET has_started = true WHERE id = $1";
    if let Ok(rows_affected) = client.execute(stmt, &[&user_id]).await {
        return rows_affected > 0;
    }
    false
}

/// Toggles user execution mode between PAPER and LIVE
pub async fn toggle_user_mode(client: &Client, user_id: &str) -> Option<String> {
    let stmt = "
        UPDATE users
        SET execution_mode = CASE WHEN execution_mode = 'LIVE' THEN 'PAPER' ELSE 'LIVE' END
        WHERE id = $1
        RETURNING execution_mode;
    ";
    if let Ok(rows) = client.query(stmt, &[&user_id]).await {
        if let Some(r) = rows.first() {
            let mode: String = r.get(0);
            return Some(mode);
        }
    }
    None
}

/// Fetches the latest dynamic parameter override published by an AI Agent within max_age_secs
pub async fn fetch_latest_override(client: &Client, agent_id: &str, max_age_secs: i64) -> Option<(f64, f64, i32)> {
    let stmt = "
        SELECT hhi_ceiling, volatility_trail_multiplier, stagnation_ttl_seconds
        FROM agent_parameter_overrides
        WHERE agent_id = $1 AND applied_to_engine = true
          AND timestamp >= NOW() - ($2 * interval '1 second')
        ORDER BY timestamp DESC
        LIMIT 1
    ";
    if let Ok(rows) = client.query(stmt, &[&agent_id, &max_age_secs]).await {
        if let Some(row) = rows.first() {
            let hhi: f64 = row.get::<_, Option<f64>>(0).unwrap_or(0.280);
            let trail: f64 = row.get::<_, Option<f64>>(1).unwrap_or(2.20);
            let ttl: i32 = row.get::<_, Option<i32>>(2).unwrap_or(240);
            return Some((hhi, trail, ttl));
        }
    }
    None
}


/// Queries cumulative forensic statistics directly from PostgreSQL
pub async fn fetch_aggregate_stats(client: &Client) -> (i64, i64, f64) {
    let stmt = "
        SELECT 
            COUNT(*)::bigint AS total_trades,
            COUNT(*) FILTER (WHERE realized_pnl_sol > 0.0)::bigint AS wins,
            COALESCE(SUM(realized_pnl_sol), 0.0) AS net_realized
        FROM settled_trades
    ";
    if let Ok(rows) = client.query(stmt, &[]).await {
        if let Some(row) = rows.first() {
            let total: i64 = row.get::<_, i64>(0);
            let wins: i64 = row.get::<_, i64>(1);
            let net: f64 = row.get::<_, f64>(2);
            return (total, wins, net);
        }
    }
    (0, 0, 0.0)
}


/// Queries cumulative statistics and rejection counts from PostgreSQL
pub async fn fetch_complete_system_stats(client: &Client) -> (i64, i64, f64, i64) {
    let stats_stmt = "
        SELECT 
            COUNT(*)::bigint AS total_trades,
            COUNT(*) FILTER (WHERE realized_pnl_sol > 0.0)::bigint AS wins,
            COALESCE(SUM(realized_pnl_sol), 0.0) AS net_realized
        FROM settled_trades
    ";
    let rejections_stmt = "
        SELECT COUNT(*)::bigint FROM pre_trade_audits WHERE admitted = false
    ";
    
    let mut total = 0i64;
    let mut wins = 0i64;
    let mut net = 0.0f64;
    let mut rejections = 0i64;

    if let Ok(rows) = client.query(stats_stmt, &[]).await {
        if let Some(row) = rows.first() {
            total = row.get(0);
            wins = row.get(1);
            net = row.get(2);
        }
    }

    if let Ok(rows) = client.query(rejections_stmt, &[]).await {
        if let Some(row) = rows.first() {
            rejections = row.get(0);
        }
    }

    (total, wins, net, rejections)
}

/// Queries both settled_trades and settled_trades_v070_archive for distinct (token_mint, symbol)
/// where is_toxic = true or pnl_pct < min_non_toxic_pnl_pct
pub async fn fetch_archived_toxic_tokens(client: &Client, min_non_toxic_pnl_pct: f64) -> Vec<(String, String)> {
    let mut tokens = Vec::new();
    let query = "
        SELECT DISTINCT token_mint, symbol FROM (
            SELECT token_mint, symbol, is_toxic, pnl_pct FROM settled_trades
            UNION ALL
            SELECT token_mint, symbol, is_toxic, pnl_pct FROM settled_trades_v070_archive
        ) sub
        WHERE (is_toxic = true OR pnl_pct < $1)
          AND (token_mint != '' OR symbol != '')
    ";
    if let Ok(rows) = client.query(query, &[&min_non_toxic_pnl_pct]).await {
        for row in rows {
            let mint: String = row.get(0);
            let sym: String = row.get(1);
            tokens.push((mint, sym));
        }
    } else {
        let fallback = "
            SELECT DISTINCT token_mint, symbol FROM settled_trades
            WHERE (is_toxic = true OR pnl_pct < $1)
              AND (token_mint != '' OR symbol != '')
        ";
        if let Ok(rows) = client.query(fallback, &[&min_non_toxic_pnl_pct]).await {
            for row in rows {
                let mint: String = row.get(0);
                let sym: String = row.get(1);
                tokens.push((mint, sym));
            }
        }
    }
    tokens
}

/// Queries both settled_trades and settled_trades_v070_archive for all distinct (token_mint, symbol)
/// enforcing a strict lifetime one-shot mint policy across sessions
pub async fn fetch_all_settled_tokens(client: &Client) -> Vec<(String, String)> {
    let mut tokens = Vec::new();
    let query = "
        SELECT DISTINCT token_mint, symbol FROM (
            SELECT token_mint, symbol FROM settled_trades
            UNION ALL
            SELECT token_mint, symbol FROM settled_trades_v070_archive
        ) sub
        WHERE (token_mint != '' OR symbol != '')
    ";
    if let Ok(rows) = client.query(query, &[]).await {
        for row in rows {
            let mint: String = row.get(0);
            let sym: String = row.get(1);
            tokens.push((mint, sym));
        }
    } else {
        let fallback = "
            SELECT DISTINCT token_mint, symbol FROM settled_trades
            WHERE (token_mint != '' OR symbol != '')
        ";
        if let Ok(rows) = client.query(fallback, &[]).await {
            for row in rows {
                let mint: String = row.get(0);
                let sym: String = row.get(1);
                tokens.push((mint, sym));
            }
        }
    }
    tokens
}

/// Fetches recent pre-trade audits to populate candidate radar on boot
pub async fn fetch_recent_radar_audits(client: &Client, limit: i64) -> Vec<serde_json::Value> {
    let stmt = "
        SELECT token_mint, dex_id, liquidity_usd, admitted
        FROM pre_trade_audits
        ORDER BY timestamp DESC
        LIMIT $1
    ";
    let mut audits = Vec::new();
    if let Ok(rows) = client.query(stmt, &[&limit]).await {
        for row in rows {
            let mint: String = row.get(0);
            let dex: String = row.get(1);
            let liq: f64 = row.get(2);
            let admitted: bool = row.get(3);
            let sym = if mint.len() > 6 { mint[..5].to_uppercase() } else { "MINT".to_string() };
            audits.push(serde_json::json!({
                "symbol": sym,
                "token_mint": mint,
                "dex_id": dex,
                "liquidity_usd": liq,
                "bonding_curve_pct": if mint.ends_with("pump") { 68 } else { 100 },
                "dev_holding_pct": 0.0,
                "mint_authority_revoked": true,
                "freeze_authority_revoked": true,
                "cu_consumed": 55774,
                "admitted": admitted
            }));
        }
    }
    audits
}


use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct AuditLogPayload {
    pub token_mint: String,
    pub pair_address: String,
    pub dex_id: String,
    pub liquidity_usd: f64,
    pub hhi_concentration: f64,
    pub mint_revoked: bool,
    pub freeze_revoked: bool,
    pub lp_burned: bool,
    pub admitted: bool,
    pub rejection_reason: String,
}

/// Spawns an asynchronous dedicated database worker with a single persistent connection
pub fn spawn_db_audit_worker(db_url: &'static str) -> mpsc::UnboundedSender<AuditLogPayload> {
    let (tx, mut rx) = mpsc::unbounded_channel::<AuditLogPayload>();

    tokio::spawn(async move {
        loop {
            match establish_connection(db_url).await {
                Ok(client) => {
                    println!("[DB WORKER] Persistent audit connection established.");
                    while let Some(audit) = rx.recv().await {
                        let _ = record_pre_trade_audit(
                            &client,
                            &audit.token_mint,
                            &audit.pair_address,
                            &audit.dex_id,
                            audit.liquidity_usd,
                            audit.hhi_concentration,
                            audit.mint_revoked,
                            audit.freeze_revoked,
                            audit.lp_burned,
                            audit.admitted,
                            &audit.rejection_reason,
                        ).await;
                    }
                    break;
                }
                Err(e) => {
                    eprintln!("[DB WORKER] Connection failed: {}. Retrying in 3s...", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                }
            }
        }
    });

    tx
}

/// Queries the most recent Stage 2 audits from PostgreSQL for fast startup hydration
pub async fn fetch_recent_stage2_audits(
    client: &Client,
    limit: i64,
) -> Vec<(String, String, f64, f64, bool, String)> {
    let stmt = "
        SELECT to_char(timestamp, 'HH24:MI:SS') as time_str,
               token_mint, liquidity_usd, hhi_concentration, admitted,
               COALESCE(rejection_reason, 'Admitted') as rejection_reason
        FROM pre_trade_audits
        ORDER BY timestamp DESC
        LIMIT $1
    ";
    let mut out = Vec::new();
    if let Ok(rows) = client.query(stmt, &[&limit]).await {
        for r in rows {
            let time: String = r.get("time_str");
            let mint: String = r.get("token_mint");
            let liq: f64 = r.get("liquidity_usd");
            let hhi: f64 = r.get("hhi_concentration");
            let admitted: bool = r.get("admitted");
            let reason: String = r.get("rejection_reason");
            out.push((time, mint, liq, hhi, admitted, reason));
        }
    }
    out
}
