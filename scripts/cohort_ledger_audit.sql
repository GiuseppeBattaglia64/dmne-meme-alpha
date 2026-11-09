-- scripts/cohort_ledger_audit.sql
-- Article 31: Overnight SQL Cohort Ledger in PostgreSQL
-- Evaluates settled paper trades across holding time cohorts and liquidity brackets.

\c dmne_quant

DROP VIEW IF EXISTS v_trade_cohort_performance CASCADE;

CREATE OR REPLACE VIEW v_trade_cohort_performance AS
WITH trade_enriched AS (
    SELECT
        s.trade_id,
        s.token_mint,
        s.symbol,
        s.hold_duration_seconds,
        s.pnl_pct,
        s.realized_pnl_sol,
        s.entry_size_sol,
        s.exit_rule,
        COALESCE(p.liquidity_usd, 35000.0) AS liquidity_usd,
        COALESCE(a.slippage_drift_pct, 0.0) AS slippage_drift_pct,
        CASE
            WHEN s.hold_duration_seconds < 30 THEN 'Flash (< 30s)'
            WHEN s.hold_duration_seconds <= 120 THEN 'Momentum (30s-120s)'
            WHEN s.hold_duration_seconds <= 300 THEN 'Swing (120s-300s)'
            ELSE 'Decay (> 300s)'
        END AS holding_cohort,
        CASE
            WHEN COALESCE(p.liquidity_usd, 35000.0) < 50000.0 THEN '$35k-$50k'
            WHEN COALESCE(p.liquidity_usd, 35000.0) <= 100000.0 THEN '$50k-$100k'
            ELSE '> $100k'
        END AS liquidity_bracket
    FROM settled_trades s
    LEFT JOIN LATERAL (
        SELECT liquidity_usd
        FROM pre_trade_audits
        WHERE token_mint = s.token_mint
        ORDER BY timestamp DESC
        LIMIT 1
    ) p ON true
    LEFT JOIN trade_forensic_autopsies a ON a.mint = s.token_mint
)
SELECT
    holding_cohort,
    liquidity_bracket,
    COUNT(*)::int AS trades_count,
    ROUND((COUNT(*) FILTER (WHERE realized_pnl_sol > 0)::numeric / NULLIF(COUNT(*), 0)::numeric) * 100.0, 2) AS win_rate_pct,
    ROUND(SUM(realized_pnl_sol)::numeric, 5) AS total_realized_sol,
    ROUND(AVG(pnl_pct)::numeric, 2) AS avg_pnl_pct,
    COALESCE(
        ROUND(
            (SUM(CASE WHEN realized_pnl_sol > 0 THEN realized_pnl_sol ELSE 0 END) /
             NULLIF(ABS(SUM(CASE WHEN realized_pnl_sol < 0 THEN realized_pnl_sol ELSE 0 END)), 0)
            )::numeric, 2
        ),
        CASE WHEN SUM(CASE WHEN realized_pnl_sol > 0 THEN realized_pnl_sol ELSE 0 END) > 0 THEN 99.99 ELSE 0.00 END
    ) AS profit_factor,
    ROUND(AVG(slippage_drift_pct)::numeric, 2) AS avg_slippage_drift_pct
FROM trade_enriched
GROUP BY holding_cohort, liquidity_bracket
ORDER BY
    CASE holding_cohort
        WHEN 'Flash (< 30s)' THEN 1
        WHEN 'Momentum (30s-120s)' THEN 2
        WHEN 'Swing (120s-300s)' THEN 3
        ELSE 4
    END,
    CASE liquidity_bracket
        WHEN '$35k-$50k' THEN 1
        WHEN '$50k-$100k' THEN 2
        ELSE 3
    END;

-- Institutional Ledger Execution Output
SELECT * FROM v_trade_cohort_performance;

