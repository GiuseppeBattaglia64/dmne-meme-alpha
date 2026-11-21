use std::collections::{HashSet, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;

use dmne_meme_alpha::config::{EngineConfig, RiskConfig};
use dmne_meme_alpha::orderflow::RollingCvdTracker;
use dmne_meme_alpha::risk::{self, ActivePosition, ExitAction, TradeOutcome};
use dmne_meme_alpha::{
    CONTRACT_SCHEMA_VERSION, DashboardSnapshot, SettledTradeRecord, TrancheStage,
};

#[test]
fn test_golden_cycle_full_lifecycle_and_replay_parity() {
    let cfg = EngineConfig::default();
    let risk_cfg = RiskConfig::default();
    let initial_bankroll_sol = cfg.calculate_initial_bankroll_sol();
    let mut bankroll_sol = initial_bankroll_sol;
    let mut total_realized_pnl_sol = 0.0f64;

    // 1. Candidate ingestion -> Half-Kelly sizing
    let is_social = true;
    let rep_score = 72.0;
    let total_calls = 12;
    let liquidity_usd = 42_000.0;
    let (_tier, fraction_pct) = risk::resolve_standardized_kelly_allocation(
        is_social,
        rep_score,
        total_calls,
        liquidity_usd,
        &risk_cfg,
    );
    let sized_sol = risk::compute_dynamic_entry_size_sol_from_fraction(
        bankroll_sol,
        fraction_pct,
        0,
        &risk_cfg,
    );
    assert!(sized_sol >= risk_cfg.min_entry_sol);

    let entry_price_sol = 1.0e-6;
    let mut active_pos = ActivePosition {
        token_mint: "GOLDEN_TEST_MINT_111111111111111111111111111".to_string(),
        symbol: "GOLDEN".to_string(),
        entry_price: entry_price_sol,
        last_price: entry_price_sol,
        initial_size_sol: sized_sol,
        remaining_size_sol: sized_sol,
        remaining_token_amount: 0,
        peak_pnl_pct: 0.0,
        tp1_executed: false,
        tp2_executed: false,
        opened_at_epoch: 1_700_000_000,
        jito_tip_lamports: risk_cfg.jito_tip_lamports,
        execution_priority: "STANDARD_ROUTINE".to_string(),
        price_buffer: VecDeque::from(vec![entry_price_sol]),
        cvd_tracker: RollingCvdTracker::new(60.0),
    };

    // 2. Simulate TP1 (40%), TP2 (35%), and Runner (25%) exits
    let tp1_price = entry_price_sol * (1.0 + (risk_cfg.tp1_trigger_pct + 5.0) / 100.0);
    let tp2_price = entry_price_sol * (1.0 + (risk_cfg.tp2_trigger_pct + 5.0) / 100.0);
    let runner_peak_price = entry_price_sol * 3.0; // +200% peak
    let runner_exit_price = runner_peak_price * (1.0 - (risk_cfg.runner_trailing_drop_pct + 2.0) / 100.0);

    let temp_csv_path = std::env::temp_dir().join(format!(
        "dmne_golden_cycle_{}.csv",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&temp_csv_path);

    let mut settled_keys: HashSet<(String, TrancheStage)> = HashSet::new();
    let mut settled_tranches_size_sol = 0.0f64;
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut scratches = 0usize;
    let mut csv_records: Vec<SettledTradeRecord> = Vec::new();

    let ticks = vec![
        (1_700_000_030, tp1_price, "TP1"),
        (1_700_000_060, tp2_price, "TP2"),
        (1_700_000_090, runner_peak_price, "PEAK"),
        (1_700_000_120, runner_exit_price, "RUNNER"),
    ];

    let mut peak_price_sol = entry_price_sol;
    for (ts, price, label) in ticks {
        if price > peak_price_sol {
            peak_price_sol = price;
        }
        let action = risk::evaluate_position_exit_with_congestion(
            &mut active_pos,
            price,
            ts,
            1.0,
            &risk_cfg,
        );
        match action {
            ExitAction::PartialSell {
                sol_basis_closed,
                rule_label,
                tip_lamports,
                ..
            } => {
                let stage = TrancheStage::from_exit_rule(&rule_label, false, false);
                assert!(
                    settled_keys.insert(("pos_golden".to_string(), stage)),
                    "Tranche {:?} must be idempotent",
                    stage
                );
                let gross_pnl_pct = ((price - entry_price_sol) / entry_price_sol) * 100.0;
                let realized = risk::compute_net_realized_sol_with_tip(
                    sol_basis_closed,
                    gross_pnl_pct,
                    &risk_cfg,
                    tip_lamports,
                );
                let net_pnl_pct = risk::compute_net_pnl_pct(sol_basis_closed, realized);
                let before = bankroll_sol;
                bankroll_sol += realized;
                total_realized_pnl_sol += realized;
                settled_tranches_size_sol += sol_basis_closed;

                assert!(
                    active_pos.remaining_size_sol + settled_tranches_size_sol
                        <= active_pos.initial_size_sol + 1e-9
                );
                assert!(settled_tranches_size_sol / active_pos.initial_size_sol <= 1.0001);

                let outcome = match risk::classify_trade_outcome(net_pnl_pct) {
                    TradeOutcome::Win => {
                        wins += 1;
                        "WIN"
                    }
                    TradeOutcome::Scratch => {
                        scratches += 1;
                        "SCRATCH"
                    }
                    TradeOutcome::Loss => {
                        losses += 1;
                        "LOSS"
                    }
                };

                csv_records.push(SettledTradeRecord {
                    schema_version: CONTRACT_SCHEMA_VERSION.to_string(),
                    timestamp_exit_utc: "2026-11-21T20:00:00+00:00".to_string(),
                    timestamp_entry_utc: "2026-11-21T19:58:00+00:00".to_string(),
                    hold_duration_secs: ts - active_pos.opened_at_epoch,
                    token_symbol: active_pos.symbol.clone(),
                    token_mint: active_pos.token_mint.clone(),
                    pair_address: "GOLDEN_PAIR".to_string(),
                    dex_url: format!("https://dexscreener.com/solana/{}", active_pos.token_mint),
                    size_sol: sol_basis_closed,
                    entry_price_sol,
                    peak_price_sol,
                    trough_price_sol: entry_price_sol,
                    exit_price_sol: price,
                    mfe_pnl_pct: ((peak_price_sol - entry_price_sol) / entry_price_sol) * 100.0,
                    mae_pnl_pct: 0.0,
                    realized_pnl_pct: net_pnl_pct,
                    realized_pnl_sol: realized,
                    realized_pnl_eur: realized * 86.0,
                    liquidity_usd,
                    fdv_usd: 150_000.0,
                    exit_rule: rule_label,
                    outcome: outcome.to_string(),
                    actual_fee_sol: (tip_lamports as f64 / 1e9) + 0.000005,
                    bankroll_before_sol: before,
                    bankroll_after_sol: bankroll_sol,
                    is_toxic: false,
                    exit_tip_lamports: tip_lamports,
                    tranche_stage: stage,
                });
            }
            ExitAction::FullExit {
                rule_label,
                is_toxic,
                tip_lamports,
            } => {
                assert!(
                    settled_keys.insert(("pos_golden".to_string(), TrancheStage::Runner)),
                    "Runner tranche must be idempotent"
                );
                let sold_size = active_pos.remaining_size_sol;
                let gross_pnl_pct = ((price - entry_price_sol) / entry_price_sol) * 100.0;
                let realized = risk::compute_net_realized_sol_with_tip(
                    sold_size,
                    gross_pnl_pct,
                    &risk_cfg,
                    tip_lamports,
                );
                let net_pnl_pct = risk::compute_net_pnl_pct(sold_size, realized);
                let before = bankroll_sol;
                bankroll_sol += realized;
                total_realized_pnl_sol += realized;
                settled_tranches_size_sol += sold_size;

                assert!((settled_tranches_size_sol - active_pos.initial_size_sol).abs() < 1e-9);
                assert!(settled_tranches_size_sol / active_pos.initial_size_sol <= 1.0001);

                let outcome = match risk::classify_trade_outcome(net_pnl_pct) {
                    TradeOutcome::Win => {
                        wins += 1;
                        "WIN"
                    }
                    TradeOutcome::Scratch => {
                        scratches += 1;
                        "SCRATCH"
                    }
                    TradeOutcome::Loss => {
                        losses += 1;
                        "LOSS"
                    }
                };

                csv_records.push(SettledTradeRecord {
                    schema_version: CONTRACT_SCHEMA_VERSION.to_string(),
                    timestamp_exit_utc: "2026-11-21T20:02:00+00:00".to_string(),
                    timestamp_entry_utc: "2026-11-21T19:58:00+00:00".to_string(),
                    hold_duration_secs: ts - active_pos.opened_at_epoch,
                    token_symbol: active_pos.symbol.clone(),
                    token_mint: active_pos.token_mint.clone(),
                    pair_address: "GOLDEN_PAIR".to_string(),
                    dex_url: format!("https://dexscreener.com/solana/{}", active_pos.token_mint),
                    size_sol: sold_size,
                    entry_price_sol,
                    peak_price_sol,
                    trough_price_sol: entry_price_sol,
                    exit_price_sol: price,
                    mfe_pnl_pct: ((peak_price_sol - entry_price_sol) / entry_price_sol) * 100.0,
                    mae_pnl_pct: 0.0,
                    realized_pnl_pct: net_pnl_pct,
                    realized_pnl_sol: realized,
                    realized_pnl_eur: realized * 86.0,
                    liquidity_usd,
                    fdv_usd: 150_000.0,
                    exit_rule: rule_label,
                    outcome: outcome.to_string(),
                    actual_fee_sol: (tip_lamports as f64 / 1e9) + 0.000005,
                    bankroll_before_sol: before,
                    bankroll_after_sol: bankroll_sol,
                    is_toxic,
                    exit_tip_lamports: tip_lamports,
                    tranche_stage: TrancheStage::Runner,
                });
            }
            ExitAction::Hold => {
                assert_eq!(label, "PEAK", "Only PEAK tick should Hold");
            }
        }
    }

    assert_eq!(csv_records.len(), 3, "Expected TP1, TP2, and Runner records");
    assert!(settled_keys.contains(&("pos_golden".to_string(), TrancheStage::TP1)));
    assert!(settled_keys.contains(&("pos_golden".to_string(), TrancheStage::TP2)));
    assert!(settled_keys.contains(&("pos_golden".to_string(), TrancheStage::Runner)));

    // 3. Write temporary trades_forensic.csv
    {
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_csv_path)
            .expect("Failed to open temp golden CSV");
        f.write_all(SettledTradeRecord::CSV_HEADER.as_bytes()).unwrap();
        for rec in &csv_records {
            f.write_all(rec.to_csv_row().as_bytes()).unwrap();
        }
    }

    // 4. Reload via [STARTUP RECONCILE] CSV parser & verify conservation + sandbox_replay parity
    let reloaded = SettledTradeRecord::parse_csv_file(temp_csv_path.to_str().unwrap())
        .expect("Failed to parse temp golden CSV");
    assert_eq!(reloaded.len(), 3);

    let reloaded_pnl_sum: f64 = reloaded.iter().map(|r| r.realized_pnl_sol).sum();
    let expected_rounded_pnl: f64 = csv_records
        .iter()
        .map(|r| format!("{:.6}", r.realized_pnl_sol).parse::<f64>().unwrap())
        .sum();
    assert!(
        (reloaded_pnl_sum - expected_rounded_pnl).abs() < 1e-10,
        "Replay PnL parity mismatch: {} vs {}",
        reloaded_pnl_sum,
        expected_rounded_pnl
    );
    assert!((bankroll_sol - (initial_bankroll_sol + total_realized_pnl_sol)).abs() < 1e-8);

    let mut replay_wins = 0usize;
    let mut replay_losses = 0usize;
    let mut replay_scratches = 0usize;
    for r in &reloaded {
        match risk::classify_trade_outcome(r.realized_pnl_pct) {
            TradeOutcome::Win => replay_wins += 1,
            TradeOutcome::Loss => replay_losses += 1,
            TradeOutcome::Scratch => replay_scratches += 1,
        }
    }
    assert_eq!((replay_wins, replay_losses, replay_scratches), (wins, losses, scratches));

    // 5. DashboardSnapshot round-trip serialization
    let snap = DashboardSnapshot {
        schema_version: CONTRACT_SCHEMA_VERSION.to_string(),
        engine_version: "2.0.0".to_string(),
        system_version: "2.0.0".to_string(),
        live_onchain_sol: 0.0,
        is_scanner_paused: false,
        panic_mode: false,
        wallet_pubkey: "GOLDEN_WALLET".to_string(),
        initial_bankroll_sol,
        current_bankroll_sol: initial_bankroll_sol + reloaded_pnl_sum,
        floating_pnl_sol: 0.0,
        live_nav_sol: initial_bankroll_sol + reloaded_pnl_sum,
        total_realized_pnl_sol: reloaded_pnl_sum,
        total_trades: reloaded.len(),
        wins: replay_wins,
        losses: replay_losses,
        scratches: replay_scratches,
        effective_win_rate_pct: 100.0,
        rejections_count: 0,
        persistence_error_count: 0,
        active_positions: serde_json::json!([]),
        radar_candidates: serde_json::json!([]),
        closed_trades: serde_json::to_value(&reloaded).unwrap(),
        recent_onchain_events: vec![],
        risk_config: serde_json::to_value(&risk_cfg).unwrap(),
        config: serde_json::json!({}),
        rejection_gauge: serde_json::json!({}),
        stage2_audit_log: serde_json::json!([]),
    };
    let json = serde_json::to_string(&snap).expect("Serialize DashboardSnapshot");
    let decoded: DashboardSnapshot =
        serde_json::from_str(&json).expect("Deserialize DashboardSnapshot");
    assert_eq!(decoded.schema_version, CONTRACT_SCHEMA_VERSION);
    assert_eq!(decoded.total_trades, 3);
    assert!((decoded.total_realized_pnl_sol - reloaded_pnl_sum).abs() < 1e-10);

    let _ = std::fs::remove_file(&temp_csv_path);
}
