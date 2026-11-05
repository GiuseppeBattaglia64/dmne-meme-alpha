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
) {
    let stmt = "
        INSERT INTO settled_trades (
            token_mint, symbol, hold_duration_seconds, entry_size_sol, entry_price_sol,
            exit_price_sol, peak_price_sol, pnl_pct, realized_pnl_sol, bankroll_before_sol,
            bankroll_after_sol, exit_rule, dex_url
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
    ";
    let _ = client.execute(
        stmt,
        &[
            &mint, &symbol, &hold_duration_sec, &entry_size_sol, &entry_price_sol,
            &exit_price_sol, &peak_price_sol, &pnl_pct, &realized_pnl_sol, &bankroll_before,
            &bankroll_after, &exit_rule, &dex_url
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

