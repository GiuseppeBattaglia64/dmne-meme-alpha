//! Module 4: Volatility-Scaled Dynamic Exit Bands
//!
//! Replaces static TP1 (+45%) and TP2 (+90%) with dynamic, volatility-scaled exit bands:
//!   TP1_adaptive = entry * (1 + k_tp1 * sigma_tick)
//!   TP2_adaptive = entry * (1 + k_tp2 * sigma_tick)
//!
//! Fully typed, zero magic numbers, serde default fallbacks, and deterministic exit replay.

use serde::{Deserialize, Serialize};

/// Strongly typed configuration for Volatility-Scaled Dynamic Exit Bands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveExitConfig {
    /// Multiplier for TP1 volatility scaling (e.g. 1.25)
    #[serde(default = "default_k_tp1")]
    pub k_tp1: f64,

    /// Multiplier for TP2 volatility scaling (e.g. 2.50)
    #[serde(default = "default_k_tp2")]
    pub k_tp2: f64,

    /// Minimum micro-volatility floor (prevents overly narrow exit bands in low-vol regimes)
    #[serde(default = "default_min_vol_floor")]
    pub min_vol_floor: f64,

    /// Maximum micro-volatility ceiling (prevents unrealistic, unreachable TP targets)
    #[serde(default = "default_max_vol_cap")]
    pub max_vol_cap: f64,

    /// Minimum allowable TP1 percentage cutoff
    #[serde(default = "default_min_tp1_pct")]
    pub min_tp1_pct: f64,

    /// Maximum allowable TP1 percentage cutoff
    #[serde(default = "default_max_tp1_pct")]
    pub max_tp1_pct: f64,

    /// Minimum allowable TP2 percentage cutoff
    #[serde(default = "default_min_tp2_pct")]
    pub min_tp2_pct: f64,

    /// Maximum allowable TP2 percentage cutoff
    #[serde(default = "default_max_tp2_pct")]
    pub max_tp2_pct: f64,

    /// Minimum separation between TP1 and TP2 percentages
    #[serde(default = "default_min_tp_spread_pct")]
    pub min_tp_spread_pct: f64,

    /// Fraction of position sold upon TP1 execution (default: 40%)
    #[serde(default = "default_tp1_sell_fraction")]
    pub tp1_sell_fraction: f64,

    /// Fraction of position sold upon TP2 execution (default: 35%)
    #[serde(default = "default_tp2_sell_fraction")]
    pub tp2_sell_fraction: f64,

    /// Trailing stop drop percentage from peak for runner tranche (default: 18%)
    #[serde(default = "default_runner_trailing_drop_pct")]
    pub runner_trailing_drop_pct: f64,

    /// Breakeven arm threshold (default: +24%)
    #[serde(default = "default_breakeven_arm_pct")]
    pub breakeven_arm_pct: f64,

    /// Breakeven floor profit guarantee (default: +8%)
    #[serde(default = "default_breakeven_floor_pct")]
    pub breakeven_floor_pct: f64,

    /// Early abort evaluation window in seconds (default: 30s)
    #[serde(default = "default_early_abort_window_secs")]
    pub early_abort_window_secs: u64,

    /// Early abort maximum peak threshold (default: +1.0%)
    #[serde(default = "default_early_abort_max_peak_pct")]
    pub early_abort_max_peak_pct: f64,

    /// Early abort loss trigger (default: -10.0%)
    #[serde(default = "default_early_abort_loss_pct")]
    pub early_abort_loss_pct: f64,

    /// Hard stop-loss threshold (default: -20.0%)
    #[serde(default = "default_stop_loss_pct")]
    pub stop_loss_pct: f64,
}

fn default_k_tp1() -> f64 { 1.25 }
fn default_k_tp2() -> f64 { 2.50 }
fn default_min_vol_floor() -> f64 { 0.15 }
fn default_max_vol_cap() -> f64 { 0.65 }
fn default_min_tp1_pct() -> f64 { 20.0 }
fn default_max_tp1_pct() -> f64 { 55.0 }
fn default_min_tp2_pct() -> f64 { 40.0 }
fn default_max_tp2_pct() -> f64 { 120.0 }
fn default_min_tp_spread_pct() -> f64 { 15.0 }
fn default_tp1_sell_fraction() -> f64 { 0.40 }
fn default_tp2_sell_fraction() -> f64 { 0.35 }
fn default_runner_trailing_drop_pct() -> f64 { 18.0 }
fn default_breakeven_arm_pct() -> f64 { 24.0 }
fn default_breakeven_floor_pct() -> f64 { 8.0 }
fn default_early_abort_window_secs() -> u64 { 30 }
fn default_early_abort_max_peak_pct() -> f64 { 1.0 }
fn default_early_abort_loss_pct() -> f64 { -10.0 }
fn default_stop_loss_pct() -> f64 { -20.0 }

impl Default for AdaptiveExitConfig {
    fn default() -> Self {
        Self {
            k_tp1: default_k_tp1(),
            k_tp2: default_k_tp2(),
            min_vol_floor: default_min_vol_floor(),
            max_vol_cap: default_max_vol_cap(),
            min_tp1_pct: default_min_tp1_pct(),
            max_tp1_pct: default_max_tp1_pct(),
            min_tp2_pct: default_min_tp2_pct(),
            max_tp2_pct: default_max_tp2_pct(),
            min_tp_spread_pct: default_min_tp_spread_pct(),
            tp1_sell_fraction: default_tp1_sell_fraction(),
            tp2_sell_fraction: default_tp2_sell_fraction(),
            runner_trailing_drop_pct: default_runner_trailing_drop_pct(),
            breakeven_arm_pct: default_breakeven_arm_pct(),
            breakeven_floor_pct: default_breakeven_floor_pct(),
            early_abort_window_secs: default_early_abort_window_secs(),
            early_abort_max_peak_pct: default_early_abort_max_peak_pct(),
            early_abort_loss_pct: default_early_abort_loss_pct(),
            stop_loss_pct: default_stop_loss_pct(),
        }
    }
}

/// Dynamically resolved volatility-scaled exit targets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveExitTargets {
    /// Realized micro-volatility (sigma_tick)
    pub sigma_tick: f64,
    /// Absolute target price for TP1
    pub tp1_price: f64,
    /// Absolute target price for TP2
    pub tp2_price: f64,
    /// Percentage gain target for TP1 (e.g. +28.5%)
    pub tp1_pct: f64,
    /// Percentage gain target for TP2 (e.g. +57.0%)
    pub tp2_pct: f64,
}

/// Simulation result of an adaptive exit replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveExitResult {
    pub gross_pnl_pct: f64,
    pub net_pnl_sol: f64,
    pub exit_rule: String,
    pub tp1_hit: bool,
    pub tp2_hit: bool,
    pub runner_hit: bool,
    pub exit_price: f64,
}

/// Computes the realized micro-volatility (sigma_tick) from pre-entry/shadow price ticks
/// and trade extreme excursions.
///
/// 1. If pre-entry price ticks (N >= 3) are provided, computes sample standard deviation of log returns:
///    r_t = ln(P_t / P_{t-1})
/// 2. If pre-entry ticks are sparse, estimates normalized range volatility from early trade extremes:
///    sigma_range = ln(peak / entry) / sqrt(2)
/// 3. Clamps result between min_vol_floor and max_vol_cap.
pub fn compute_realized_micro_volatility(
    pre_entry_ticks: &[f64],
    peak_price: f64,
    trough_price: f64,
    entry_price: f64,
    cfg: &AdaptiveExitConfig,
) -> f64 {
    let mut sigma_estimates: Vec<f64> = Vec::new();

    // 1. Tick log returns volatility
    if pre_entry_ticks.len() >= 3 {
        let mut returns = Vec::with_capacity(pre_entry_ticks.len() - 1);
        for i in 1..pre_entry_ticks.len() {
            let p0 = pre_entry_ticks[i - 1];
            let p1 = pre_entry_ticks[i];
            if p0 > 0.0 && p1 > 0.0 {
                returns.push((p1 / p0).ln());
            }
        }
        if returns.len() >= 2 {
            let n = returns.len() as f64;
            let mean = returns.iter().sum::<f64>() / n;
            let variance = returns.iter().map(|&r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
            let s_tick = variance.sqrt();
            if s_tick > 0.0 && !s_tick.is_nan() {
                sigma_estimates.push(s_tick);
            }
        }
    }

    // 2. Early-trade upward/downward excursion volatility
    if entry_price > 0.0 && peak_price > 0.0 && trough_price > 0.0 {
        let up_ratio = peak_price / entry_price;
        let down_ratio = if trough_price < entry_price { entry_price / trough_price } else { 1.0 };
        let log_up = if up_ratio > 1.0 { up_ratio.ln() } else { 0.0 };
        let log_down = if down_ratio > 1.0 { down_ratio.ln() } else { 0.0 };
        let sigma_excursion = ((log_up.powi(2) + log_down.powi(2)) / 2.0).sqrt();
        if sigma_excursion > 0.0 && !sigma_excursion.is_nan() {
            sigma_estimates.push(sigma_excursion);
        }
    }

    let raw_sigma = if sigma_estimates.is_empty() {
        cfg.min_vol_floor
    } else {
        sigma_estimates.iter().sum::<f64>() / (sigma_estimates.len() as f64)
    };

    raw_sigma.clamp(cfg.min_vol_floor, cfg.max_vol_cap)
}

/// Resolves dynamic exit target prices and percentages from entry price and micro-volatility.
pub fn compute_adaptive_exit_targets(
    entry_price: f64,
    sigma_tick: f64,
    cfg: &AdaptiveExitConfig,
) -> AdaptiveExitTargets {
    let bounded_sigma = sigma_tick.clamp(cfg.min_vol_floor, cfg.max_vol_cap);

    let raw_tp1_pct = cfg.k_tp1 * bounded_sigma * 100.0;
    let tp1_pct = raw_tp1_pct.clamp(cfg.min_tp1_pct, cfg.max_tp1_pct);

    let raw_tp2_pct = cfg.k_tp2 * bounded_sigma * 100.0;
    let mut tp2_pct = raw_tp2_pct.clamp(cfg.min_tp2_pct, cfg.max_tp2_pct);

    // Enforce minimum tranche spread
    if tp2_pct < tp1_pct + cfg.min_tp_spread_pct {
        tp2_pct = tp1_pct + cfg.min_tp_spread_pct;
    }

    let tp1_price = entry_price * (1.0 + tp1_pct / 100.0);
    let tp2_price = entry_price * (1.0 + tp2_pct / 100.0);

    AdaptiveExitTargets {
        sigma_tick: bounded_sigma,
        tp1_price,
        tp2_price,
        tp1_pct,
        tp2_pct,
    }
}

/// Simulates the trade exit given observed historical price curve extremes and adaptive targets.
pub fn simulate_adaptive_exit(
    entry_price: f64,
    peak_price: f64,
    actual_exit_price: f64,
    hold_secs: u64,
    size_sol: f64,
    fee_sol: f64,
    targets: &AdaptiveExitTargets,
    cfg: &AdaptiveExitConfig,
) -> AdaptiveExitResult {
    if entry_price <= 0.0 {
        return AdaptiveExitResult {
            gross_pnl_pct: 0.0,
            net_pnl_sol: -fee_sol,
            exit_rule: "INVALID_ENTRY".to_string(),
            tp1_hit: false,
            tp2_hit: false,
            runner_hit: false,
            exit_price: 0.0,
        };
    }

    let peak_pnl_pct = ((peak_price - entry_price) / entry_price) * 100.0;
    let actual_exit_pnl_pct = ((actual_exit_price - entry_price) / entry_price) * 100.0;

    // Case A: Peak >= TP2_adaptive
    if peak_pnl_pct >= targets.tp2_pct {
        let runner_exit_mult = (1.0 + peak_pnl_pct / 100.0) * (1.0 - cfg.runner_trailing_drop_pct / 100.0);
        let runner_pnl_pct = ((runner_exit_mult - 1.0) * 100.0).max(cfg.breakeven_floor_pct);
        let runner_frac = (1.0 - cfg.tp1_sell_fraction - cfg.tp2_sell_fraction).max(0.0);

        let gross_pct = (cfg.tp1_sell_fraction * targets.tp1_pct)
            + (cfg.tp2_sell_fraction * targets.tp2_pct)
            + (runner_frac * runner_pnl_pct);

        let net_sol = (size_sol * (gross_pct / 100.0)) - fee_sol;
        let exit_price = entry_price * (1.0 + gross_pct / 100.0);

        return AdaptiveExitResult {
            gross_pnl_pct: gross_pct,
            net_pnl_sol: net_sol,
            exit_rule: format!("Adaptive TP2 (+{:.1}%) & Runner", targets.tp2_pct),
            tp1_hit: true,
            tp2_hit: true,
            runner_hit: true,
            exit_price,
        };
    }

    // Case B: Peak >= TP1_adaptive
    if peak_pnl_pct >= targets.tp1_pct {
        let runner_exit_mult = (1.0 + peak_pnl_pct / 100.0) * (1.0 - cfg.runner_trailing_drop_pct / 100.0);
        let runner_pnl_pct = ((runner_exit_mult - 1.0) * 100.0).max(cfg.breakeven_floor_pct);
        let runner_frac = (1.0 - cfg.tp1_sell_fraction).max(0.0);

        let gross_pct = (cfg.tp1_sell_fraction * targets.tp1_pct) + (runner_frac * runner_pnl_pct);
        let net_sol = (size_sol * (gross_pct / 100.0)) - fee_sol;
        let exit_price = entry_price * (1.0 + gross_pct / 100.0);

        return AdaptiveExitResult {
            gross_pnl_pct: gross_pct,
            net_pnl_sol: net_sol,
            exit_rule: format!("Adaptive TP1 (+{:.1}%) Scale-Out", targets.tp1_pct),
            tp1_hit: true,
            tp2_hit: false,
            runner_hit: true,
            exit_price,
        };
    }

    // Case C: Peak >= Breakeven Arm Threshold (+24%)
    if peak_pnl_pct >= cfg.breakeven_arm_pct {
        let gross_pct = actual_exit_pnl_pct.max(cfg.breakeven_floor_pct);
        let net_sol = (size_sol * (gross_pct / 100.0)) - fee_sol;
        let exit_price = entry_price * (1.0 + gross_pct / 100.0);

        return AdaptiveExitResult {
            gross_pnl_pct: gross_pct,
            net_pnl_sol: net_sol,
            exit_rule: format!("Breakeven Floor (+{:.1}%)", cfg.breakeven_floor_pct),
            tp1_hit: false,
            tp2_hit: false,
            runner_hit: false,
            exit_price,
        };
    }

    // Case D: Early Abort Window
    if peak_pnl_pct < cfg.early_abort_max_peak_pct
        && hold_secs >= cfg.early_abort_window_secs
        && actual_exit_pnl_pct <= cfg.early_abort_loss_pct
    {
        let gross_pct = cfg.early_abort_loss_pct;
        let net_sol = (size_sol * (gross_pct / 100.0)) - fee_sol;
        let exit_price = entry_price * (1.0 + gross_pct / 100.0);

        return AdaptiveExitResult {
            gross_pnl_pct: gross_pct,
            net_pnl_sol: net_sol,
            exit_rule: format!("Early Abort Window ({:.1}%)", cfg.early_abort_loss_pct),
            tp1_hit: false,
            tp2_hit: false,
            runner_hit: false,
            exit_price,
        };
    }

    // Case E: Hard Stop Loss or Gap Slippage Exit
    if actual_exit_pnl_pct <= cfg.stop_loss_pct {
        let gross_pct = actual_exit_pnl_pct;
        let net_sol = (size_sol * (gross_pct / 100.0)) - fee_sol;
        let exit_price = entry_price * (1.0 + gross_pct / 100.0);

        return AdaptiveExitResult {
            gross_pnl_pct: gross_pct,
            net_pnl_sol: net_sol,
            exit_rule: "Stop Loss / Gap Slippage".to_string(),
            tp1_hit: false,
            tp2_hit: false,
            runner_hit: false,
            exit_price,
        };
    }

    // Case F: Natural Exit (Stagnation TTL or Flash Dump Breaker)
    let gross_pct = actual_exit_pnl_pct;
    let net_sol = (size_sol * (gross_pct / 100.0)) - fee_sol;
    let exit_price = actual_exit_price;

    AdaptiveExitResult {
        gross_pnl_pct: gross_pct,
        net_pnl_sol: net_sol,
        exit_rule: "Natural / TTL Exit".to_string(),
        tp1_hit: false,
        tp2_hit: false,
        runner_hit: false,
        exit_price,
    }
}

/// Simulates adaptive exit replay taking into account whether a ledger record was an individual
/// tranche harvest (e.g. TP1 40%, TP2 35%, Runner 25%) or an entire position close.
pub fn simulate_adaptive_trade_replay(
    entry_price: f64,
    peak_price: f64,
    actual_exit_price: f64,
    actual_pnl_pct: f64,
    recorded_rule: &str,
    hold_secs: u64,
    size_sol: f64,
    fee_sol: f64,
    targets: &AdaptiveExitTargets,
    cfg: &AdaptiveExitConfig,
) -> AdaptiveExitResult {
    if entry_price <= 0.0 {
        return AdaptiveExitResult {
            gross_pnl_pct: 0.0,
            net_pnl_sol: -fee_sol,
            exit_rule: "INVALID_ENTRY".to_string(),
            tp1_hit: false,
            tp2_hit: false,
            runner_hit: false,
            exit_price: 0.0,
        };
    }

    let peak_pnl_pct = ((peak_price - entry_price) / entry_price) * 100.0;

    // 1. Dedicated Tranche 1 Scale-Out Row
    if recorded_rule.contains("TP1 Scale-Out") {
        let gross_pct = if peak_pnl_pct >= targets.tp1_pct {
            // Unlocked dynamic TP1
            actual_pnl_pct.max(targets.tp1_pct)
        } else {
            actual_pnl_pct
        };
        let net_sol = (size_sol * (gross_pct / 100.0)) - fee_sol;
        return AdaptiveExitResult {
            gross_pnl_pct: gross_pct,
            net_pnl_sol: net_sol,
            exit_rule: format!("Adaptive TP1 (+{:.1}%) Scale-Out", targets.tp1_pct),
            tp1_hit: true,
            tp2_hit: false,
            runner_hit: false,
            exit_price: entry_price * (1.0 + gross_pct / 100.0),
        };
    }

    // 2. Dedicated Tranche 2 Scale-Out Row
    if recorded_rule.contains("TP2 Scale-Out") {
        let gross_pct = if peak_pnl_pct >= targets.tp2_pct {
            // Unlocked dynamic TP2
            actual_pnl_pct.max(targets.tp2_pct)
        } else {
            // Trailed from peak
            let trail = (1.0 + peak_pnl_pct / 100.0) * (1.0 - cfg.runner_trailing_drop_pct / 100.0);
            ((trail - 1.0) * 100.0).max(cfg.breakeven_floor_pct)
        };
        let net_sol = (size_sol * (gross_pct / 100.0)) - fee_sol;
        return AdaptiveExitResult {
            gross_pnl_pct: gross_pct,
            net_pnl_sol: net_sol,
            exit_rule: format!("Adaptive TP2 (+{:.1}%) Scale-Out", targets.tp2_pct),
            tp1_hit: true,
            tp2_hit: peak_pnl_pct >= targets.tp2_pct,
            runner_hit: true,
            exit_price: entry_price * (1.0 + gross_pct / 100.0),
        };
    }

    // 3. Dedicated Trailing Runner Row
    if recorded_rule.contains("Trailing Runner") {
        let trail = (1.0 + peak_pnl_pct / 100.0) * (1.0 - cfg.runner_trailing_drop_pct / 100.0);
        let gross_pct = ((trail - 1.0) * 100.0).max(cfg.breakeven_floor_pct);
        let net_sol = (size_sol * (gross_pct / 100.0)) - fee_sol;
        return AdaptiveExitResult {
            gross_pnl_pct: gross_pct,
            net_pnl_sol: net_sol,
            exit_rule: format!("Adaptive Trailing Runner (-{:.1}%)", cfg.runner_trailing_drop_pct),
            tp1_hit: true,
            tp2_hit: peak_pnl_pct >= targets.tp2_pct,
            runner_hit: true,
            exit_price: entry_price * (1.0 + gross_pct / 100.0),
        };
    }

    // 4. Single Full-Position Exits: evaluate full adaptive ladder
    simulate_adaptive_exit(
        entry_price,
        peak_price,
        actual_exit_price,
        hold_secs,
        size_sol,
        fee_sol,
        targets,
        cfg,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_exit_config_defaults() {
        let cfg = AdaptiveExitConfig::default();
        assert_eq!(cfg.k_tp1, 1.25);
        assert_eq!(cfg.k_tp2, 2.50);
        assert_eq!(cfg.min_vol_floor, 0.15);
        assert_eq!(cfg.max_vol_cap, 0.65);
        assert_eq!(cfg.min_tp1_pct, 20.0);
        assert_eq!(cfg.max_tp1_pct, 55.0);
        assert_eq!(cfg.min_tp2_pct, 40.0);
        assert_eq!(cfg.max_tp2_pct, 120.0);
    }

    #[test]
    fn test_volatility_computation_with_ticks() {
        let cfg = AdaptiveExitConfig::default();
        let ticks = vec![1.0, 1.05, 1.02, 1.08, 1.12];
        let sigma = compute_realized_micro_volatility(&ticks, 1.15, 0.98, 1.0, &cfg);
        assert!(sigma >= cfg.min_vol_floor);
        assert!(sigma <= cfg.max_vol_cap);
    }

    #[test]
    fn test_adaptive_targets_spread_enforcement() {
        let cfg = AdaptiveExitConfig::default();
        let entry = 0.0001;
        let targets = compute_adaptive_exit_targets(entry, 0.15, &cfg);
        assert!(targets.tp1_pct >= cfg.min_tp1_pct);
        assert!(targets.tp2_pct >= targets.tp1_pct + cfg.min_tp_spread_pct);
        assert!(targets.tp1_price > entry);
        assert!(targets.tp2_price > targets.tp1_price);
    }

    #[test]
    fn test_adaptive_tp1_hit_simulation() {
        let cfg = AdaptiveExitConfig::default();
        let entry = 1.0;
        let peak = 1.30; // +30% MFE
        let actual_exit = 0.95; // -5%
        let targets = AdaptiveExitTargets {
            sigma_tick: 0.18,
            tp1_price: 1.225,
            tp2_price: 1.45,
            tp1_pct: 22.5,
            tp2_pct: 45.0,
        };

        let res = simulate_adaptive_exit(entry, peak, actual_exit, 60, 0.04, 0.00105, &targets, &cfg);
        assert!(res.tp1_hit);
        assert!(!res.tp2_hit);
        assert!(res.gross_pnl_pct > 0.0);
        assert!(res.net_pnl_sol > 0.0);
    }

    #[test]
    fn test_adaptive_tp2_hit_simulation() {
        let cfg = AdaptiveExitConfig::default();
        let entry = 1.0;
        let peak = 1.60; // +60% MFE
        let actual_exit = 1.20;
        let targets = AdaptiveExitTargets {
            sigma_tick: 0.20,
            tp1_price: 1.25,
            tp2_price: 1.50,
            tp1_pct: 25.0,
            tp2_pct: 50.0,
        };

        let res = simulate_adaptive_exit(entry, peak, actual_exit, 120, 0.04, 0.00105, &targets, &cfg);
        assert!(res.tp1_hit);
        assert!(res.tp2_hit);
        assert!(res.gross_pnl_pct >= 35.0);
    }
}
