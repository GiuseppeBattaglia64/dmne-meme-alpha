#![allow(dead_code)]
// DMNE QUANT // RISK MANAGEMENT & EXIT ARBITRATION SUBSYSTEM
// Multi-Tiered Convex Exit Engine, Uncapped Breakeven Floor Guard & Moonbag Trailing Runner

use serde::{Deserialize, Serialize};
use crate::config::RiskConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivePosition {
    pub token_mint: String,
    pub symbol: String,
    pub entry_price: f64,
    pub last_price: f64,
    pub initial_size_sol: f64,
    pub remaining_size_sol: f64,
    pub remaining_token_amount: u64,
    pub peak_pnl_pct: f64,
    pub tp1_executed: bool,
    pub tp2_executed: bool,
    pub opened_at_epoch: u64,
    #[serde(default = "default_position_tip_lamports")]
    pub jito_tip_lamports: u64,
    #[serde(default = "default_position_execution_priority")]
    pub execution_priority: String,
}

fn default_position_tip_lamports() -> u64 { 10_000 }
fn default_position_execution_priority() -> String { "STANDARD_ROUTINE".to_string() }

#[derive(Debug, Clone, PartialEq)]
pub enum ExitAction {
    Hold,
    PartialSell {
        sell_fraction_of_remaining: f64,
        sol_basis_closed: f64,
        rule_label: String,
        tip_lamports: u64,
    },
    FullExit {
        rule_label: String,
        is_toxic: bool,
        tip_lamports: u64,
    },
}

impl ExitAction {
    pub fn tip_lamports(&self, _cfg: &RiskConfig) -> u64 {
        match self {
            ExitAction::Hold => 0,
            ExitAction::PartialSell { tip_lamports, .. } => *tip_lamports,
            ExitAction::FullExit { tip_lamports, .. } => *tip_lamports,
        }
    }

    pub fn is_emergency(&self) -> bool {
        match self {
            ExitAction::Hold => false,
            ExitAction::PartialSell { .. } => false,
            ExitAction::FullExit { rule_label, .. } => is_emergency_risk_exit(rule_label),
        }
    }
}

pub fn is_emergency_risk_exit(rule_label: &str) -> bool {
    let lower = rule_label.to_lowercase();
    lower.contains("stop-loss")
        || lower.contains("stop loss")
        || lower.contains("flash dump")
        || lower.contains("slippage")
        || lower.contains("early abort")
        || lower.contains("hard_stop_loss")
        || lower.contains("early_abort")
}

pub fn resolve_exit_tip(rule_label: &str, cfg: &RiskConfig) -> u64 {
    if is_emergency_risk_exit(rule_label) {
        cfg.emergency_jito_tip_lamports
    } else {
        cfg.jito_tip_lamports
    }
}

pub fn evaluate_position_exit(
    pos: &mut ActivePosition,
    current_price: f64,
    now_epoch: u64,
    cfg: &RiskConfig,
) -> ExitAction {
    let pnl_pct = (((current_price - pos.entry_price) / pos.entry_price) * 100.0 * 1e6).round() / 1e6;
    let tick_drop_pct = if pos.last_price > 0.0 {
        ((current_price - pos.last_price) / pos.last_price) * 100.0
    } else {
        0.0
    };

    if pnl_pct > pos.peak_pnl_pct {
        pos.peak_pnl_pct = pnl_pct;
    }

    pos.last_price = current_price;

    let elapsed_secs = now_epoch.saturating_sub(pos.opened_at_epoch);

    // 1. Max Adverse Slippage Cutoff (Emergency tip escalation)
    if pnl_pct <= cfg.max_slippage_loss_pct {
        return ExitAction::FullExit {
            rule_label: format!("Max Slippage Loss ({:.0}%)", cfg.max_slippage_loss_pct),
            is_toxic: true,
            tip_lamports: cfg.emergency_jito_tip_lamports,
        };
    }

    // 2. Hard Stop-Loss (Emergency tip escalation)
    if pnl_pct <= cfg.stop_loss_pct {
        return ExitAction::FullExit {
            rule_label: format!("Stop-Loss ({:.0}%)", cfg.stop_loss_pct),
            is_toxic: true,
            tip_lamports: cfg.emergency_jito_tip_lamports,
        };
    }

    // 3. Flash Dump Circuit Breaker (Emergency tip escalation)
    if tick_drop_pct <= cfg.flash_dump_tick_pct && pnl_pct <= cfg.flash_dump_max_pnl_pct {
        return ExitAction::FullExit {
            rule_label: format!("Flash Dump Breaker ({:.0}%)", cfg.flash_dump_tick_pct),
            is_toxic: true,
            tip_lamports: cfg.emergency_jito_tip_lamports,
        };
    }

    // 3b. Early Fail-Fast Abort for Dead-on-Arrival Entries (Emergency tip escalation)
    if !pos.tp1_executed && elapsed_secs >= cfg.early_abort_window_secs && pos.peak_pnl_pct < cfg.early_abort_max_peak_pct && pnl_pct <= cfg.early_abort_loss_pct {
        return ExitAction::FullExit {
            rule_label: format!("Early Abort ({:.0}%)", cfg.early_abort_loss_pct),
            is_toxic: true,
            tip_lamports: cfg.emergency_jito_tip_lamports,
        };
    }

    // 4. Tier 1 Take-Profit (Standard tip)
    if !pos.tp1_executed && pnl_pct >= cfg.tp1_trigger_pct {
        pos.tp1_executed = true;
        let sol_closed = pos.initial_size_sol * cfg.tp1_sell_fraction;
        pos.remaining_size_sol = (pos.remaining_size_sol - sol_closed).max(0.0);

        return ExitAction::PartialSell {
            sell_fraction_of_remaining: cfg.tp1_sell_fraction,
            sol_basis_closed: sol_closed,
            rule_label: format!("TP1 Scale-Out (+{:.0}%)", cfg.tp1_trigger_pct),
            tip_lamports: cfg.jito_tip_lamports,
        };
    }

    // 5. Tier 2 Take-Profit (Standard tip)
    if pos.tp1_executed && !pos.tp2_executed && pnl_pct >= cfg.tp2_trigger_pct {
        pos.tp2_executed = true;
        let sol_closed = pos.initial_size_sol * cfg.tp2_sell_fraction;
        let fraction_of_current = cfg.tp2_sell_fraction / (1.0 - cfg.tp1_sell_fraction);
        pos.remaining_size_sol = (pos.remaining_size_sol - sol_closed).max(0.0);

        return ExitAction::PartialSell {
            sell_fraction_of_remaining: fraction_of_current.min(1.0),
            sol_basis_closed: sol_closed,
            rule_label: format!("TP2 Scale-Out (+{:.0}%)", cfg.tp2_trigger_pct),
            tip_lamports: cfg.jito_tip_lamports,
        };
    }

    // 6. Moonbag Trailing Stop (Standard tip)
    let peak_mult = 1.0 + (pos.peak_pnl_pct / 100.0);
    let curr_mult = 1.0 + (pnl_pct / 100.0);
    let peak_drawdown_pct = if peak_mult > 0.0 {
        ((peak_mult - curr_mult) / peak_mult) * 100.0
    } else {
        0.0
    };

    if pos.tp1_executed && peak_drawdown_pct >= cfg.runner_trailing_drop_pct {
        return ExitAction::FullExit {
            rule_label: format!(
                "Trailing Runner (-{:.0}% from +{:.1}% Peak)",
                cfg.runner_trailing_drop_pct, pos.peak_pnl_pct
            ),
            is_toxic: false,
            tip_lamports: cfg.jito_tip_lamports,
        };
    }

    // 7. Uncapped Breakeven Guard (Standard tip)
    if !pos.tp1_executed
        && pos.peak_pnl_pct >= cfg.breakeven_arm_pct
        && pnl_pct <= cfg.breakeven_floor_pct
    {
        return ExitAction::FullExit {
            rule_label: format!("Breakeven Guard (+{:.0}%)", cfg.breakeven_floor_pct),
            is_toxic: false,
            tip_lamports: cfg.jito_tip_lamports,
        };
    }

    // 8. Conditional Stagnation TTL (Standard tip)
    if !pos.tp1_executed
        && elapsed_secs >= cfg.stagnation_ttl_secs
        && pos.peak_pnl_pct < cfg.stagnation_max_peak_pct
        && pnl_pct >= cfg.stagnation_band_min_pct
        && pnl_pct <= cfg.stagnation_band_max_pct
    {
        return ExitAction::FullExit {
            rule_label: "Stagnation TTL Exit".to_string(),
            is_toxic: false,
            tip_lamports: cfg.jito_tip_lamports,
        };
    }

    ExitAction::Hold
}

pub fn is_historical_exit_toxic(exit_pnl_pct: f64, cfg: &RiskConfig) -> bool {
    exit_pnl_pct < cfg.stagnation_band_min_pct || exit_pnl_pct <= cfg.stop_loss_pct
}

pub fn validate_entry_gates(
    liquidity_usd: f64,
    fdv_usd: f64,
    m5_price_change_pct: f64,
    h1_price_change_pct: f64,
    buy_sell_ratio: f64,
    m5_txns: u32,
    m5_volume_usd: f64,
    conviction_pct: f64,
    cfg: &RiskConfig,
) -> Result<(), &'static str> {
    if conviction_pct < cfg.min_entry_conviction_pct {
        return Err("CONVICTION_BELOW_FLOOR");
    }
    if liquidity_usd < cfg.min_entry_liquidity_usd {
        return Err("LIQUIDITY_BELOW_FLOOR");
    }
    if liquidity_usd > 0.0 && (fdv_usd / liquidity_usd) > cfg.max_entry_fdv_to_liquidity_ratio {
        return Err("FDV_LIQ_RATIO_EXCEEDED");
    }
    if m5_price_change_pct < cfg.min_entry_price_change_pct {
        return Err("ENTRY_MOMENTUM_BELOW_THRESHOLD");
    }
    if m5_price_change_pct > cfg.max_entry_price_change_pct {
        return Err("ENTRY_MOMENTUM_OVEREXTENDED");
    }
    if h1_price_change_pct < cfg.min_entry_h1_price_change_pct {
        return Err("H1_TREND_BEARISH");
    }
    if buy_sell_ratio < cfg.min_entry_buy_sell_ratio {
        return Err("BUY_SELL_RATIO_BELOW_THRESHOLD");
    }
    if m5_txns < cfg.min_entry_m5_txns {
        return Err("M5_TXNS_BELOW_FLOOR");
    }
    if m5_volume_usd < cfg.min_entry_m5_volume_usd {
        return Err("M5_VOLUME_BELOW_FLOOR");
    }
    Ok(())
}

pub fn validate_queue_pop_snapshot(
    liquidity_usd: f64,
    fdv_usd: f64,
    m5_price_change_pct: f64,
    h1_price_change_pct: f64,
    buy_sell_ratio: f64,
    conviction_pct: f64,
    cfg: &RiskConfig,
) -> Result<(), &'static str> {
    if conviction_pct < cfg.min_entry_conviction_pct {
        return Err("CONVICTION_BELOW_FLOOR");
    }
    if liquidity_usd < cfg.min_entry_liquidity_usd {
        return Err("LIQUIDITY_BELOW_FLOOR");
    }
    if liquidity_usd > 0.0 && (fdv_usd / liquidity_usd) > cfg.max_entry_fdv_to_liquidity_ratio {
        return Err("FDV_LIQ_RATIO_EXCEEDED");
    }
    if m5_price_change_pct < cfg.min_entry_price_change_pct {
        return Err("ENTRY_MOMENTUM_BELOW_THRESHOLD");
    }
    if h1_price_change_pct < cfg.min_entry_h1_price_change_pct {
        return Err("H1_TREND_BEARISH");
    }
    if buy_sell_ratio < cfg.min_entry_buy_sell_ratio {
        return Err("BUY_SELL_RATIO_BELOW_THRESHOLD");
    }
    Ok(())
}

pub fn validate_live_tick_confirmation(
    prices: &[f64],
    liquidities: &[f64],
    cfg: &RiskConfig,
) -> Result<(), &'static str> {
    let min_ticks = cfg.entry_confirm_ticks as usize;
    if prices.len() < min_ticks || liquidities.len() < min_ticks {
        return Err("INSUFFICIENT_CONFIRM_TICKS");
    }
    if prices.iter().any(|&p| p <= 0.0) || liquidities.iter().any(|&l| l <= 0.0) {
        return Err("INVALID_TICK_TELEMETRY");
    }

    // Every step prices[i] >= prices[i-1]
    for w in prices.windows(2) {
        if w[1] < w[0] {
            return Err("CONFIRMATION_TICK_DOWN");
        }
    }

    let first_price = prices[0];
    let last_price = *prices.last().unwrap();
    if cfg.min_confirm_tick_gain_pct > 0.0 && last_price <= first_price {
        return Err("CONFIRMATION_ZERO_OR_NEGATIVE_GAIN");
    }

    let gain_pct = ((last_price - first_price) / first_price) * 100.0;
    if gain_pct < cfg.min_confirm_tick_gain_pct {
        return Err("CONFIRMATION_MOMENTUM_BELOW_MIN");
    }
    if gain_pct > cfg.max_confirm_tick_gain_pct {
        return Err("CONFIRMATION_OVEREXTENDED");
    }

    let first_liq = liquidities[0];
    let last_liq = *liquidities.last().unwrap();
    let retention_ratio = last_liq / first_liq;
    if retention_ratio < cfg.min_confirm_liquidity_retention_ratio {
        return Err("CONFIRMATION_LIQUIDITY_DRAIN");
    }

    Ok(())
}

pub const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

pub fn compute_net_realized_sol(basis_sol: f64, gross_pnl_pct: f64, cfg: &RiskConfig) -> f64 {
    compute_net_realized_sol_with_tip(basis_sol, gross_pnl_pct, cfg, cfg.jito_tip_lamports)
}

pub fn compute_net_realized_sol_with_tip(
    basis_sol: f64,
    gross_pnl_pct: f64,
    cfg: &RiskConfig,
    tip_lamports: u64,
) -> f64 {
    let gross_sol = basis_sol * (gross_pnl_pct / 100.0);
    let exit_notional_sol = (basis_sol + gross_sol).max(0.0);
    let half_fee_rate = (cfg.dex_roundtrip_fee_pct / 2.0) / 100.0;
    let dex_fee_sol = (basis_sol * half_fee_rate) + (exit_notional_sol * half_fee_rate);
    let jito_tip_sol = (tip_lamports as f64) / LAMPORTS_PER_SOL;
    gross_sol - dex_fee_sol - jito_tip_sol
}

pub fn compute_net_realized_sol_for_exit(
    basis_sol: f64,
    gross_pnl_pct: f64,
    rule_label: &str,
    cfg: &RiskConfig,
) -> f64 {
    let tip = resolve_exit_tip(rule_label, cfg);
    compute_net_realized_sol_with_tip(basis_sol, gross_pnl_pct, cfg, tip)
}

pub fn compute_net_pnl_pct(basis_sol: f64, net_realized_sol: f64) -> f64 {
    if basis_sol > 0.0 {
        (net_realized_sol / basis_sol) * 100.0
    } else {
        0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConvictionTier {
    LowVelocity,
    Baseline,
    HighConviction,
}

impl ConvictionTier {
    pub fn label(&self) -> &'static str {
        match self {
            ConvictionTier::HighConviction => "HIGH_CONVICTION",
            ConvictionTier::Baseline => "BASELINE",
            ConvictionTier::LowVelocity => "LOW_VELOCITY",
        }
    }
}

/// Evaluates candidate conviction tier based on buy/sell ratio, conviction %, and 5m velocity metrics
pub fn evaluate_conviction_tier(
    buy_sell_ratio: f64,
    conviction_pct: f64,
    m5_txns: u64,
    m5_volume_usd: f64,
    cfg: &RiskConfig,
) -> ConvictionTier {
    // Upper-tier cutoff: buy/sell ratio >= high_conviction_buy_ratio_threshold or top conviction score
    if buy_sell_ratio >= cfg.high_conviction_buy_ratio_threshold || conviction_pct >= 85.0 {
        ConvictionTier::HighConviction
    } else if buy_sell_ratio <= (cfg.min_entry_buy_sell_ratio + 0.15)
        || m5_txns <= (cfg.min_entry_m5_txns as u64 + 3)
        || m5_volume_usd <= (cfg.min_entry_m5_volume_usd * 1.25)
    {
        ConvictionTier::LowVelocity
    } else {
        ConvictionTier::Baseline
    }
}

/// Resolves the Kelly fraction percentage for a given conviction tier
pub fn resolve_kelly_fraction_pct(tier: ConvictionTier, cfg: &RiskConfig) -> f64 {
    match tier {
        ConvictionTier::HighConviction => cfg.max_kelly_fraction_nav_pct,
        ConvictionTier::Baseline => cfg.base_kelly_fraction_nav_pct,
        ConvictionTier::LowVelocity => cfg.min_kelly_fraction_nav_pct,
    }
}

/// Dynamic Conviction-Tiered Kelly Sizing with slot headroom and bankroll solvency guards
pub fn compute_dynamic_entry_size_sol(
    bankroll_sol: f64,
    tier: ConvictionTier,
    active_slots_used: usize,
    cfg: &RiskConfig,
) -> f64 {
    // Headroom guard: if maximum active slots are already full, no new position can be opened
    if active_slots_used >= cfg.max_active_slots {
        return 0.0;
    }

    // 1. Determine Kelly fraction according to conviction tier
    let kelly_fraction = resolve_kelly_fraction_pct(tier, cfg) / 100.0;
    let raw_allocation = bankroll_sol * kelly_fraction;

    // 2. Headroom guard: cannot exceed equal-share capacity across active slots
    let max_slots = (cfg.max_active_slots as f64).max(1.0);
    let slot_headroom_cap = bankroll_sol / max_slots;

    // 3. Solvency guard: ensure minimum operational gas reserve remains untouched
    let available_capital = (bankroll_sol - cfg.min_gas_reserve_sol).max(0.0);

    let constrained = raw_allocation
        .min(slot_headroom_cap)
        .min(available_capital);

    // 4. Bound between min_entry_sol and max_entry_sol
    if constrained < cfg.min_entry_sol {
        if available_capital < cfg.min_entry_sol {
            return 0.0; // Preserve solvency when bankroll cannot safely support even minimum entry
        }
        cfg.min_entry_sol
    } else {
        constrained.min(cfg.max_entry_sol)
    }
}

pub fn format_position_status(pos: &ActivePosition, cfg: &RiskConfig) -> String {
    if pos.tp2_executed {
        let runner_pct = (1.0 - cfg.tp1_sell_fraction - cfg.tp2_sell_fraction) * 100.0;
        format!("{:.0}% Runner Moonbag", runner_pct)
    } else if pos.tp1_executed {
        format!("TP1 Harvested (+{:.0}%)", cfg.tp1_trigger_pct)
    } else if pos.peak_pnl_pct >= cfg.breakeven_arm_pct {
        format!("Breakeven Armed (+{:.0}% Floor)", cfg.breakeven_floor_pct)
    } else {
        "Monitoring Momentum".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_position() -> ActivePosition {
        ActivePosition {
            token_mint: "EW1b9rcLAJe634mgxb5r4bVYxDoGU37o7YazBzjTpump".to_string(),
            symbol: "RM".to_string(),
            entry_price: 1.0,
            last_price: 1.0,
            initial_size_sol: 0.10,
            remaining_size_sol: 0.10,
            remaining_token_amount: 1_000_000,
            peak_pnl_pct: 0.0,
            tp1_executed: false,
            tp2_executed: false,
            opened_at_epoch: 1000,
            jito_tip_lamports: 10_000,
            execution_priority: "STANDARD_ROUTINE".to_string(),
        }
    }

    #[test]
    fn test_hard_stop_loss() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();
        let action = evaluate_position_exit(&mut pos, 0.79, 1050, &cfg);
        match action {
            ExitAction::FullExit { rule_label, is_toxic, tip_lamports } => {
                assert!(rule_label.contains("Stop-Loss"));
                assert!(is_toxic);
                assert_eq!(tip_lamports, cfg.emergency_jito_tip_lamports);
            }
            _ => panic!("Expected Stop-Loss exit"),
        }
    }

    #[test]
    fn test_flash_dump_breaker() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();

        // Single-tick drop of -16% from last_price 1.0 to 0.84, with PnL <= 0%
        let action = evaluate_position_exit(&mut pos, 0.84, 1050, &cfg);
        match action {
            ExitAction::FullExit { rule_label, is_toxic, tip_lamports } => {
                assert!(rule_label.contains("Flash Dump Breaker"));
                assert!(is_toxic);
                assert_eq!(tip_lamports, cfg.emergency_jito_tip_lamports);
            }
            _ => panic!("Expected Flash Dump Breaker exit"),
        }
    }

    #[test]
    fn test_tp1_and_tp2_scale_out() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();

        // TP1 at +45% (price 1.45)
        let action1 = evaluate_position_exit(&mut pos, 1.45, 1050, &cfg);
        match action1 {
            ExitAction::PartialSell { sell_fraction_of_remaining, sol_basis_closed, tip_lamports, .. } => {
                assert_eq!(sell_fraction_of_remaining, 0.40);
                assert!((sol_basis_closed - 0.04).abs() < 1e-6);
                assert!((pos.remaining_size_sol - 0.06).abs() < 1e-6);
                assert!(pos.tp1_executed);
                assert_eq!(tip_lamports, cfg.jito_tip_lamports);
            },
            _ => panic!("Expected TP1 PartialSell"),
        }

        // TP2 at +90% (price 1.90)
        let action2 = evaluate_position_exit(&mut pos, 1.90, 1060, &cfg);
        match action2 {
            ExitAction::PartialSell { sol_basis_closed, tip_lamports, .. } => {
                assert!((sol_basis_closed - 0.035).abs() < 1e-6);
                assert!((pos.remaining_size_sol - 0.025).abs() < 1e-6); // 25% moonbag runner
                assert!(pos.tp2_executed);
                assert_eq!(tip_lamports, cfg.jito_tip_lamports);
            },
            _ => panic!("Expected TP2 PartialSell"),
        }
    }

    #[test]
    fn test_moonbag_trailing_runner() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();

        // Reach +60% (triggers TP1)
        let _ = evaluate_position_exit(&mut pos, 1.60, 1050, &cfg);
        assert!(pos.tp1_executed);
        assert!((pos.peak_pnl_pct - 60.0).abs() < 1e-6);

        // Pull back from +60% (price 1.60) to price 1.29 (relative peak drawdown of 19.375% > 18%)
        let action = evaluate_position_exit(&mut pos, 1.29, 1060, &cfg);
        match action {
            ExitAction::FullExit { rule_label, is_toxic, tip_lamports } => {
                assert!(rule_label.contains("Trailing Runner (-18% from +60.0% Peak)"));
                assert!(!is_toxic);
                assert_eq!(tip_lamports, cfg.jito_tip_lamports);
            }
            _ => panic!("Expected Trailing Runner exit"),
        }
    }

    #[test]
    fn test_breakeven_guard() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();

        // Climb to +36% (arms breakeven guard, peak = 36%)
        let _ = evaluate_position_exit(&mut pos, 1.36, 1050, &cfg);
        assert!(!pos.tp1_executed);
        assert!((pos.peak_pnl_pct - 36.0).abs() < 1e-6);

        // Retrace to +7% (below +8% floor)
        let action = evaluate_position_exit(&mut pos, 1.07, 1060, &cfg);
        match action {
            ExitAction::FullExit { rule_label, is_toxic, tip_lamports } => {
                assert!(rule_label.contains("Breakeven Guard"));
                assert!(!is_toxic);
                assert_eq!(tip_lamports, cfg.jito_tip_lamports);
            }
            _ => panic!("Expected Breakeven Guard exit"),
        }
    }

    #[test]
    fn test_stagnation_ttl() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();

        // After 250s, peak was only +4%, currently -2%
        let action = evaluate_position_exit(&mut pos, 0.98, 1250, &cfg);
        match action {
            ExitAction::FullExit { rule_label, is_toxic, tip_lamports } => {
                assert!(rule_label.contains("Stagnation TTL"));
                assert!(!is_toxic);
                assert_eq!(tip_lamports, cfg.jito_tip_lamports);
            }
            _ => panic!("Expected Stagnation TTL exit"),
        }
    }

    #[test]
    fn test_max_slippage_loss() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();

        // Gap down to -26% (exceeds -25% max slippage)
        let action = evaluate_position_exit(&mut pos, 0.74, 1050, &cfg);
        match action {
            ExitAction::FullExit { rule_label, is_toxic, tip_lamports } => {
                assert!(rule_label.contains("Max Slippage Loss"));
                assert!(is_toxic);
                assert_eq!(tip_lamports, cfg.emergency_jito_tip_lamports);
            }
            _ => panic!("Expected Max Slippage Loss exit"),
        }
    }

    #[test]
    fn test_is_historical_exit_toxic() {
        let cfg = RiskConfig::default();
        // stagnation_band_min_pct = -8.0, stop_loss_pct = -20.0
        assert!(is_historical_exit_toxic(-20.0, &cfg));
        assert!(is_historical_exit_toxic(-22.0, &cfg));
        assert!(is_historical_exit_toxic(-12.5, &cfg)); // underwater flash dump
        assert!(is_historical_exit_toxic(-8.1, &cfg));
        assert!(!is_historical_exit_toxic(-5.0, &cfg)); // stagnation band
        assert!(!is_historical_exit_toxic(0.0, &cfg));
        assert!(!is_historical_exit_toxic(12.0, &cfg));
        assert!(!is_historical_exit_toxic(45.0, &cfg));
    }

    #[test]
    fn test_format_position_status() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();

        assert_eq!(format_position_status(&pos, &cfg), "Monitoring Momentum");

        pos.peak_pnl_pct = 36.0;
        assert_eq!(format_position_status(&pos, &cfg), format!("Breakeven Armed (+{:.0}% Floor)", cfg.breakeven_floor_pct));

        pos.tp1_executed = true;
        assert_eq!(format_position_status(&pos, &cfg), "TP1 Harvested (+45%)");

        pos.tp2_executed = true;
        assert_eq!(format_position_status(&pos, &cfg), "25% Runner Moonbag");
    }

    #[test]
    fn test_validate_entry_gates() {
        let cfg = RiskConfig::default();
        // Passing valid baseline (liq $30k >= $20k, ratio 1.5 >= 1.25, conviction 75% >= 55%)
        assert!(validate_entry_gates(30_000.0, 150_000.0, 5.0, 2.0, 1.5, 25, 5_000.0, 75.0, &cfg).is_ok());

        // 0. Conviction below floor (45% < 55%)
        assert_eq!(validate_entry_gates(30_000.0, 150_000.0, 5.0, 2.0, 1.5, 25, 5_000.0, 45.0, &cfg), Err("CONVICTION_BELOW_FLOOR"));

        // 1. Liquidity below floor ($15,000 < $20,000)
        assert_eq!(validate_entry_gates(15_000.0, 100_000.0, 5.0, 2.0, 1.5, 25, 5_000.0, 75.0, &cfg), Err("LIQUIDITY_BELOW_FLOOR"));

        // 2. FDV to Liquidity ratio exceeded (FDV $500,000 / Liq $30,000 = 16.6x > 15.0x)
        assert_eq!(validate_entry_gates(30_000.0, 500_000.0, 5.0, 2.0, 1.5, 25, 5_000.0, 75.0, &cfg), Err("FDV_LIQ_RATIO_EXCEEDED"));

        // 3. Entry momentum below threshold (+0.5% < +1.5%)
        assert_eq!(validate_entry_gates(30_000.0, 150_000.0, 0.5, 2.0, 1.5, 25, 5_000.0, 75.0, &cfg), Err("ENTRY_MOMENTUM_BELOW_THRESHOLD"));

        // 4. Entry momentum overextended (+90.0% > +85.0%)
        assert_eq!(validate_entry_gates(30_000.0, 150_000.0, 90.0, 2.0, 1.5, 25, 5_000.0, 75.0, &cfg), Err("ENTRY_MOMENTUM_OVEREXTENDED"));

        // 5. H1 trend bearish (-12.0% < -10.0%)
        assert_eq!(validate_entry_gates(30_000.0, 150_000.0, 5.0, -12.0, 1.5, 25, 5_000.0, 75.0, &cfg), Err("H1_TREND_BEARISH"));

        // 6. Buy/sell ratio below threshold (1.15 < 1.25)
        assert_eq!(validate_entry_gates(30_000.0, 150_000.0, 5.0, 2.0, 1.15, 25, 5_000.0, 75.0, &cfg), Err("BUY_SELL_RATIO_BELOW_THRESHOLD"));

        // 7. M5 txns below floor (10 < 15)
        assert_eq!(validate_entry_gates(30_000.0, 150_000.0, 5.0, 2.0, 1.5, 10, 5_000.0, 75.0, &cfg), Err("M5_TXNS_BELOW_FLOOR"));

        // 8. M5 volume below floor ($1,000 < $2,500)
        assert_eq!(validate_entry_gates(30_000.0, 150_000.0, 5.0, 2.0, 1.5, 25, 1_000.0, 75.0, &cfg), Err("M5_VOLUME_BELOW_FLOOR"));
    }

    #[test]
    fn test_validate_queue_pop_snapshot() {
        let cfg = RiskConfig::default();
        // Valid baseline: liq $25,000 >= $20,000, FDV/liq = 4x <= 15x, m5 = +2.5% >= 1.5%, h1 = +5% >= -10%, ratio = 1.5 >= 1.25, conviction = 75% >= 55%
        assert!(validate_queue_pop_snapshot(25_000.0, 100_000.0, 2.5, 5.0, 1.5, 75.0, &cfg).is_ok());

        // 0. Conviction below floor (45% < 55%)
        assert_eq!(validate_queue_pop_snapshot(25_000.0, 100_000.0, 2.5, 5.0, 1.5, 45.0, &cfg), Err("CONVICTION_BELOW_FLOOR"));

        // 1. Liquidity below floor ($15,000 < $20,000)
        assert_eq!(validate_queue_pop_snapshot(15_000.0, 100_000.0, 2.5, 5.0, 1.5, 75.0, &cfg), Err("LIQUIDITY_BELOW_FLOOR"));

        // 2. FDV to Liquidity ratio exceeded (FDV $400,000 / Liq $25,000 = 16x > 15x)
        assert_eq!(validate_queue_pop_snapshot(25_000.0, 400_000.0, 2.5, 5.0, 1.5, 75.0, &cfg), Err("FDV_LIQ_RATIO_EXCEEDED"));

        // 3. M5 momentum below threshold (+0.5% < +1.5%)
        assert_eq!(validate_queue_pop_snapshot(25_000.0, 100_000.0, 0.5, 5.0, 1.5, 75.0, &cfg), Err("ENTRY_MOMENTUM_BELOW_THRESHOLD"));

        // 4. H1 trend bearish (-12.0% < -10.0%)
        assert_eq!(validate_queue_pop_snapshot(25_000.0, 100_000.0, 2.5, -12.0, 1.5, 75.0, &cfg), Err("H1_TREND_BEARISH"));

        // 5. Buy/sell ratio below threshold (1.15 < 1.25)
        assert_eq!(validate_queue_pop_snapshot(25_000.0, 100_000.0, 2.5, 5.0, 1.15, 75.0, &cfg), Err("BUY_SELL_RATIO_BELOW_THRESHOLD"));
    }

    #[test]
    fn test_early_abort_exit() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();
        pos.peak_pnl_pct = 0.5;
        let action = evaluate_position_exit(&mut pos, 0.89, 1046, &cfg);
        match action {
            ExitAction::FullExit { rule_label, is_toxic, tip_lamports } => {
                assert!(rule_label.contains("Early Abort"));
                assert!(is_toxic);
                assert_eq!(tip_lamports, cfg.emergency_jito_tip_lamports);
            }
            _ => panic!("Expected Early Abort exit"),
        }
    }

    #[test]
    fn test_compute_net_realized_sol() {
        let cfg = RiskConfig {
            dex_roundtrip_fee_pct: 0.60,
            jito_tip_lamports: 10_000,
            ..RiskConfig::default()
        };
        let basis_sol: f64 = 0.10;
        let gross_pnl_pct: f64 = 50.0;
        let gross_sol: f64 = basis_sol * (gross_pnl_pct / 100.0); // 0.05 SOL
        let exit_notional_sol: f64 = (basis_sol + gross_sol).max(0.0); // 0.15 SOL
        let half_fee_rate: f64 = (cfg.dex_roundtrip_fee_pct / 2.0) / 100.0; // 0.003
        let dex_fee_sol: f64 = (basis_sol * half_fee_rate) + (exit_notional_sol * half_fee_rate); // 0.00075 SOL
        let jito_tip_sol: f64 = (cfg.jito_tip_lamports as f64) / LAMPORTS_PER_SOL; // 0.00001 SOL
        let expected_net: f64 = gross_sol - dex_fee_sol - jito_tip_sol;

        let net = compute_net_realized_sol(basis_sol, gross_pnl_pct, &cfg);
        assert!((net - expected_net).abs() < 1e-12);
        assert!(net < gross_sol);

        let net_pnl_pct = compute_net_pnl_pct(basis_sol, net);
        let expected_pnl_pct = (net / basis_sol) * 100.0;
        assert!((net_pnl_pct - expected_pnl_pct).abs() < 1e-12);
        assert_eq!(compute_net_pnl_pct(0.0, 1.0), 0.0);
    }

    #[test]
    fn test_validate_live_tick_confirmation() {
        let cfg = RiskConfig::default(); // entry_confirm_ticks = 2, min_confirm_tick_gain_pct = 0.0

        // 1. Valid non-decreasing monotonic confirmation with positive gain & liquidity retention
        let valid_prices = vec![1.000, 1.010];
        let valid_liq = vec![30_000.0, 30_100.0];
        assert!(validate_live_tick_confirmation(&valid_prices, &valid_liq, &cfg).is_ok());

        // 2. Flat candle surviving DexScreener CDN cache window (0.0% gain passes when min_confirm_tick_gain_pct == 0.0)
        let flat_prices = vec![1.000, 1.000];
        assert!(validate_live_tick_confirmation(&flat_prices, &valid_liq, &cfg).is_ok());

        // 3. Insufficient confirm ticks (1 < 2)
        let short_prices = vec![1.000];
        let short_liq = vec![30_000.0];
        assert_eq!(validate_live_tick_confirmation(&short_prices, &short_liq, &cfg), Err("INSUFFICIENT_CONFIRM_TICKS"));

        // 4. Invalid zero or negative telemetry
        let zero_prices = vec![1.000, 0.0];
        assert_eq!(validate_live_tick_confirmation(&zero_prices, &valid_liq, &cfg), Err("INVALID_TICK_TELEMETRY"));

        // 5. Confirmation tick down (reversal / falling knife)
        let dip_prices = vec![1.000, 0.990];
        assert_eq!(validate_live_tick_confirmation(&dip_prices, &valid_liq, &cfg), Err("CONFIRMATION_TICK_DOWN"));

        // 6. Cumulative gain overextended (+30.0% > +25.0%)
        let parabolic_prices = vec![1.000, 1.300];
        assert_eq!(validate_live_tick_confirmation(&parabolic_prices, &valid_liq, &cfg), Err("CONFIRMATION_OVEREXTENDED"));

        // 7. Liquidity drain / rugging during confirmation ($27k / $30k = 0.90 < 0.98)
        let drain_liq = vec![30_000.0, 27_000.0];
        assert_eq!(validate_live_tick_confirmation(&valid_prices, &drain_liq, &cfg), Err("CONFIRMATION_LIQUIDITY_DRAIN"));

        // 8. Strict minimum gain enforcement when min_confirm_tick_gain_pct > 0.0
        let mut strict_cfg = cfg.clone();
        strict_cfg.min_confirm_tick_gain_pct = 0.8;
        assert_eq!(validate_live_tick_confirmation(&flat_prices, &valid_liq, &strict_cfg), Err("CONFIRMATION_ZERO_OR_NEGATIVE_GAIN"));

        let slow_prices = vec![1.000, 1.004];
        assert_eq!(validate_live_tick_confirmation(&slow_prices, &valid_liq, &strict_cfg), Err("CONFIRMATION_MOMENTUM_BELOW_MIN"));
    }

    #[test]
    fn test_emergency_tip_resolution() {
        let cfg = RiskConfig::default();
        // Emergency risk exits escalate to 75,000 lamports
        assert_eq!(resolve_exit_tip("Stop-Loss (-20%)", &cfg), cfg.emergency_jito_tip_lamports);
        assert_eq!(resolve_exit_tip("Flash Dump Breaker (-15%)", &cfg), cfg.emergency_jito_tip_lamports);
        assert_eq!(resolve_exit_tip("Max Slippage Loss (-25%)", &cfg), cfg.emergency_jito_tip_lamports);
        assert_eq!(resolve_exit_tip("Early Abort (-10%)", &cfg), cfg.emergency_jito_tip_lamports);

        // Standard exits stay at 10,000 lamports
        assert_eq!(resolve_exit_tip("TP1 Scale-Out (+45%)", &cfg), cfg.jito_tip_lamports);
        assert_eq!(resolve_exit_tip("TP2 Scale-Out (+90%)", &cfg), cfg.jito_tip_lamports);
        assert_eq!(resolve_exit_tip("Trailing Runner (-18% from +50.0% Peak)", &cfg), cfg.jito_tip_lamports);
        assert_eq!(resolve_exit_tip("Breakeven Guard (+8%)", &cfg), cfg.jito_tip_lamports);
        assert_eq!(resolve_exit_tip("Stagnation TTL Exit", &cfg), cfg.jito_tip_lamports);
    }

    #[test]
    fn test_compute_net_realized_sol_with_emergency_tip() {
        let cfg = RiskConfig::default();
        let basis_sol: f64 = 0.10;
        let gross_pnl_pct: f64 = -20.0;

        let net_standard = compute_net_realized_sol(basis_sol, gross_pnl_pct, &cfg);
        let net_emergency = compute_net_realized_sol_for_exit(basis_sol, gross_pnl_pct, "Stop-Loss (-20%)", &cfg);

        // Emergency tip deducts 75,000 lamports (0.000075 SOL) vs standard 10,000 lamports (0.000010 SOL)
        let tip_difference = (cfg.emergency_jito_tip_lamports - cfg.jito_tip_lamports) as f64 / LAMPORTS_PER_SOL;
        assert!((net_standard - net_emergency - tip_difference).abs() < 1e-12);
        assert!(net_emergency < net_standard);
    }

    #[test]
    fn test_conviction_tier_evaluation() {
        let cfg = RiskConfig::default();

        // 1. High conviction: buy_sell_ratio >= 2.0 (allocates 8.5%)
        let tier_high = evaluate_conviction_tier(2.2, 70.0, 30, 5000.0, &cfg);
        assert_eq!(tier_high, ConvictionTier::HighConviction);
        assert_eq!(resolve_kelly_fraction_pct(tier_high, &cfg), 8.5);

        // 2. High conviction via top conviction %: conviction >= 85.0%
        let tier_high_conv = evaluate_conviction_tier(1.6, 90.0, 30, 5000.0, &cfg);
        assert_eq!(tier_high_conv, ConvictionTier::HighConviction);

        // 3. Baseline tier: standard momentum
        let tier_base = evaluate_conviction_tier(1.6, 70.0, 30, 5000.0, &cfg);
        assert_eq!(tier_base, ConvictionTier::Baseline);
        assert_eq!(resolve_kelly_fraction_pct(tier_base, &cfg), 6.5);

        // 4. Low velocity tier: borderline buy/sell ratio or low volume
        let tier_low = evaluate_conviction_tier(1.30, 60.0, 16, 2800.0, &cfg);
        assert_eq!(tier_low, ConvictionTier::LowVelocity);
        assert_eq!(resolve_kelly_fraction_pct(tier_low, &cfg), 4.5);
    }

    #[test]
    fn test_compute_dynamic_entry_size_sol_tiers_and_guards() {
        let cfg = RiskConfig::default();
        let bankroll_sol = 0.50; // $90-100 equivalent

        // Tiered sizing tests
        let size_high = compute_dynamic_entry_size_sol(bankroll_sol, ConvictionTier::HighConviction, 0, &cfg);
        let size_base = compute_dynamic_entry_size_sol(bankroll_sol, ConvictionTier::Baseline, 0, &cfg);
        let size_low = compute_dynamic_entry_size_sol(bankroll_sol, ConvictionTier::LowVelocity, 0, &cfg);

        // High tier (8.5% of 0.50 = 0.0425 SOL) > Base (6.5% of 0.50 = 0.0325 SOL) > Low (4.5% of 0.50 = 0.0225 SOL)
        assert!(size_high > size_base);
        assert!(size_base > size_low);
        assert!((size_high - 0.0425).abs() < 1e-4);
        assert!((size_base - 0.0325).abs() < 1e-4);
        assert!((size_low - 0.0225).abs() < 1e-4);

        // Clamping to bounds
        assert!(size_high <= cfg.max_entry_sol);
        assert!(size_low >= cfg.min_entry_sol);

        // Headroom guard: when active slots == max_active_slots (4), return 0.0
        let full_headroom = compute_dynamic_entry_size_sol(bankroll_sol, ConvictionTier::HighConviction, 4, &cfg);
        assert_eq!(full_headroom, 0.0);

        // Solvency guard: when bankroll is near or below min_gas_reserve_sol (0.030), entry is rejected
        let insolvent_bankroll = 0.032; // only 0.002 available above reserve < 0.010 min entry
        let insolvent_size = compute_dynamic_entry_size_sol(insolvent_bankroll, ConvictionTier::HighConviction, 0, &cfg);
        assert_eq!(insolvent_size, 0.0);
    }
}
