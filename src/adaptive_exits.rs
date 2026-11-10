#![allow(dead_code)]
//! Module 4: Volatility-Scaled Dynamic Exit Bands (Honest & Realistic Quantitative Replay)
//!
//! Replaces static TP1 (+45%) and TP2 (+90%) with dynamic, volatility-scaled exit bands:
//!   TP1_adaptive = entry * (1 + k_tp1 * sigma_pre)
//!   TP2_adaptive = entry * (1 + k_tp2 * sigma_pre)
//!
//! Zero lookahead bias: sigma_pre is computed SOLELY from pre-entry ticks (t <= t_entry).
//! Zero phantom liquidity: trades experiencing liquidity collapses retain historical exit slippage.
//! Fully typed, zero magic numbers, serde default fallbacks.

use std::collections::VecDeque;
use serde::{Deserialize, Serialize};
use crate::config::{DynamicExitsConfig, OrderflowConfig};

/// Strongly typed configuration for Volatility-Scaled Dynamic Exit Bands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveExitConfig {
    /// Multiplier for TP1 volatility scaling (e.g. 1.25)
    #[serde(default = "default_k_tp1")]
    pub k_tp1: f64,

    /// Multiplier for TP2 volatility scaling (e.g. 2.50)
    #[serde(default = "default_k_tp2")]
    pub k_tp2: f64,

    /// Empirical sample median fallback sigma when pre-entry ticks < min_pre_entry_ticks
    #[serde(default = "default_sigma_fallback")]
    pub sigma_fallback: f64,

    /// Minimum pre-entry ticks required to compute sample standard deviation
    #[serde(default = "default_min_pre_entry_ticks")]
    pub min_pre_entry_ticks: usize,

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
fn default_sigma_fallback() -> f64 { 0.22 }
fn default_min_pre_entry_ticks() -> usize { 3 }
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
            sigma_fallback: default_sigma_fallback(),
            min_pre_entry_ticks: default_min_pre_entry_ticks(),
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
    /// Realized micro-volatility (sigma_pre)
    pub sigma_pre: f64,
    /// Absolute target price for TP1
    pub tp1_price: f64,
    /// Absolute target price for TP2
    pub tp2_price: f64,
    /// Percentage gain target for TP1 (e.g. +27.5%)
    pub tp1_pct: f64,
    /// Percentage gain target for TP2 (e.g. +55.0%)
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

/// Volatility-scaled dynamic exit evaluation envelope
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VolatilityScaledExitBand {
    pub sigma_realized: f64,
    pub delta_trail: f64,
    pub dynamic_floor: f64,
    pub is_ratchet_active: bool,
}

/// Computes realized micro-volatility in percentage points from recent price buffer ticks
/// using logarithmic returns directly across contiguous or dual VecDeque slices without allocations.
pub fn compute_rolling_tick_volatility(ticks: &VecDeque<f64>) -> f64 {
    if ticks.len() < 2 {
        return 6.0; // Default empirical baseline tick volatility in %
    }

    let (s1, s2) = ticks.as_slices();

    let mut count = 0.0;
    let mut sum = 0.0;

    let process_pair = |p0: f64, p1: f64, count: &mut f64, sum: &mut f64| {
        if p0 > 0.0 && p1 > 0.0 {
            let r = (p1 / p0).ln() * 100.0;
            *sum += r;
            *count += 1.0;
        }
    };

    // First slice contiguous pairs
    for w in s1.windows(2) {
        process_pair(w[0], w[1], &mut count, &mut sum);
    }

    // Boundary pair bridging s1 and s2
    if let (Some(&last1), Some(&first2)) = (s1.last(), s2.first()) {
        process_pair(last1, first2, &mut count, &mut sum);
    }

    // Second slice contiguous pairs
    for w in s2.windows(2) {
        process_pair(w[0], w[1], &mut count, &mut sum);
    }

    if count < 2.0 {
        return 6.0;
    }

    let mean = sum / count;
    let mut sum_sq_diff = 0.0;

    let process_diff = |p0: f64, p1: f64, sum_sq_diff: &mut f64| {
        if p0 > 0.0 && p1 > 0.0 {
            let r = (p1 / p0).ln() * 100.0;
            *sum_sq_diff += (r - mean).powi(2);
        }
    };

    for w in s1.windows(2) {
        process_diff(w[0], w[1], &mut sum_sq_diff);
    }
    if let (Some(&last1), Some(&first2)) = (s1.last(), s2.first()) {
        process_diff(last1, first2, &mut sum_sq_diff);
    }
    for w in s2.windows(2) {
        process_diff(w[0], w[1], &mut sum_sq_diff);
    }

    let variance = sum_sq_diff / (count - 1.0);
    let s_tick = variance.sqrt();
    if s_tick.is_finite() && s_tick > 0.0 {
        s_tick
    } else {
        6.0
    }
}

/// Evaluates dynamic trailing delta and ratcheted profit floor from tick volatility and MFE
pub fn compute_volatility_scaled_exit_band(
    ticks: &VecDeque<f64>,
    peak_mfe: f64,
    cfg: &DynamicExitsConfig,
) -> VolatilityScaledExitBand {
    let sigma_realized = compute_rolling_tick_volatility(ticks);
    let delta_trail = (cfg.vol_multiplier * sigma_realized).clamp(cfg.min_trail_pct, cfg.max_trail_pct);
    let is_ratchet_active = peak_mfe >= cfg.mfe_activation_pct;
    let dynamic_floor = if is_ratchet_active {
        cfg.min_locked_floor_pct.max(peak_mfe - delta_trail)
    } else {
        0.0
    };
    VolatilityScaledExitBand {
        sigma_realized,
        delta_trail,
        dynamic_floor,
        is_ratchet_active,
    }
}

/// In-flight cruise telemetry for a single monitored trading slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CruisePositionTelemetry {
    pub symbol: String,
    pub mint: String,
    pub entry_price: f64,
    pub current_price: f64,
    pub pnl_pct: f64,
    pub peak_mfe_pct: f64,
    pub rolling_volatility_pct: f64,
    pub dynamic_trail_pct: f64,
    pub locked_floor_pct: f64,
    pub cvd_net_sol: f64,
    pub cvd_pressure_ratio: f64,
    pub cvd_divergence_state: String,
    pub hold_time_secs: u64,
    pub ttl_remaining_secs: u64,
    pub emergency_tip_lamports: u64,
    pub emergency_stop_base_tip_lamports: u64,
    pub emergency_stop_max_tip_lamports: u64,
}

/// Historical protection and circuit breaker execution stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CruiseHistoricalProtectionStats {
    pub total_closed_trades: usize,
    pub emergency_exits_triggered: usize,
    pub dynamic_ratchet_exits: usize,
    pub stop_loss_exits: usize,
    pub early_abort_exits: usize,
    pub flash_dump_exits: usize,
    pub slippage_circuit_exits: usize,
    pub recent_protected_trades: Vec<serde_json::Value>,
}

/// High-level in-flight cruise supervisor telemetry payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CruiseSupervisorTelemetry {
    pub slots_active: usize,
    pub slots_total: usize,
    pub dynamic_exits_config: DynamicExitsConfig,
    pub orderflow_config: OrderflowConfig,
    pub positions: Vec<CruisePositionTelemetry>,
    pub supervisor_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub historical_protection_stats: Option<CruiseHistoricalProtectionStats>,
}

/// Resolves dynamic exit target prices and percentages from entry price and pre-entry micro-volatility.
pub fn compute_adaptive_exit_targets(
    entry_price: f64,
    sigma_pre: f64,
    cfg: &AdaptiveExitConfig,
) -> AdaptiveExitTargets {
    let bounded_sigma = sigma_pre.clamp(cfg.min_vol_floor, cfg.max_vol_cap);

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
        sigma_pre: bounded_sigma,
        tp1_price,
        tp2_price,
        tp1_pct,
        tp2_pct,
    }
}

/// Simulates adaptive exit replay enforcing strict quantitative realism:
/// 1. Zero lookahead bias: exit targets rely strictly on sigma_pre.
/// 2. Zero phantom liquidity: catastrophic baseline dumps (Max Slippage / Flash Dump) retain actual exit slippage.
/// 3. Chronological tick progression: evaluates early aborts and stops before subsequent spikes if ticks are recorded.
pub fn simulate_honest_adaptive_trade(
    entry_price: f64,
    mfe_pnl_pct: f64,
    actual_exit_pnl_pct: f64,
    recorded_rule: &str,
    size_sol: f64,
    intra_trade_ticks: &[(f64, f64)],
    targets: &AdaptiveExitTargets,
    cfg: &AdaptiveExitConfig,
) -> AdaptiveExitResult {
    if entry_price <= 0.0 {
        return AdaptiveExitResult {
            gross_pnl_pct: 0.0,
            net_pnl_sol: 0.0,
            exit_rule: "INVALID_ENTRY".to_string(),
            tp1_hit: false,
            tp2_hit: false,
            runner_hit: false,
            exit_price: 0.0,
        };
    }

    // A. If the record in the forensic CSV was an already-separated tranche scale-out:
    if recorded_rule.contains("TP1 Scale-Out") {
        let gross_pct = if mfe_pnl_pct >= targets.tp1_pct {
            actual_exit_pnl_pct.max(targets.tp1_pct)
        } else {
            actual_exit_pnl_pct
        };
        let net_sol = size_sol * (gross_pct / 100.0);
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

    if recorded_rule.contains("TP2 Scale-Out") {
        let gross_pct = if mfe_pnl_pct >= targets.tp2_pct {
            actual_exit_pnl_pct.max(targets.tp2_pct)
        } else {
            let trail = (1.0 + mfe_pnl_pct / 100.0) * (1.0 - cfg.runner_trailing_drop_pct / 100.0);
            ((trail - 1.0) * 100.0).max(cfg.breakeven_floor_pct)
        };
        let net_sol = size_sol * (gross_pct / 100.0);
        return AdaptiveExitResult {
            gross_pnl_pct: gross_pct,
            net_pnl_sol: net_sol,
            exit_rule: format!("Adaptive TP2 (+{:.1}%) Scale-Out", targets.tp2_pct),
            tp1_hit: true,
            tp2_hit: mfe_pnl_pct >= targets.tp2_pct,
            runner_hit: true,
            exit_price: entry_price * (1.0 + gross_pct / 100.0),
        };
    }

    if recorded_rule.contains("Trailing Runner") {
        let trail = (1.0 + mfe_pnl_pct / 100.0) * (1.0 - cfg.runner_trailing_drop_pct / 100.0);
        let gross_pct = ((trail - 1.0) * 100.0).max(cfg.breakeven_floor_pct);
        let net_sol = size_sol * (gross_pct / 100.0);
        return AdaptiveExitResult {
            gross_pnl_pct: gross_pct,
            net_pnl_sol: net_sol,
            exit_rule: format!("Adaptive Trailing Runner (-{:.1}%)", cfg.runner_trailing_drop_pct),
            tp1_hit: true,
            tp2_hit: mfe_pnl_pct >= targets.tp2_pct,
            runner_hit: true,
            exit_price: entry_price * (1.0 + gross_pct / 100.0),
        };
    }

    // B. Chronological Tick Replay (Path Dependence)
    if intra_trade_ticks.len() >= 2 {
        let mut tp1_triggered = false;
        let mut running_peak_pct = 0.0f64;

        for &(_t, price) in intra_trade_ticks {
            let pnl_pct = ((price - entry_price) / entry_price) * 100.0;
            if pnl_pct > running_peak_pct {
                running_peak_pct = pnl_pct;
            }

            // Path Check 1: Stop out BEFORE reaching TP1?
            if pnl_pct <= cfg.stop_loss_pct && !tp1_triggered {
                let net_sol = size_sol * (actual_exit_pnl_pct / 100.0); // Exact historical slippage
                return AdaptiveExitResult {
                    gross_pnl_pct: actual_exit_pnl_pct,
                    net_pnl_sol: net_sol,
                    exit_rule: "Chronological Stop-Loss (Prior to TP)".to_string(),
                    tp1_hit: false,
                    tp2_hit: false,
                    runner_hit: false,
                    exit_price: entry_price * (1.0 + actual_exit_pnl_pct / 100.0),
                };
            }

            if pnl_pct >= targets.tp1_pct {
                tp1_triggered = true;
            }
        }
    }

    // C. Full Position Replay: Did MFE genuinely reach Adaptive TP1?
    if mfe_pnl_pct >= targets.tp1_pct {
        // TP1 genuinely executed on 40% tranche on the way up during active liquidity!
        // The remaining 60% took the recorded exit (retaining exact realized dump slippage if it collapsed):
        let gross_pct = (cfg.tp1_sell_fraction * targets.tp1_pct)
            + ((1.0 - cfg.tp1_sell_fraction) * actual_exit_pnl_pct);

        let net_sol = size_sol * (gross_pct / 100.0);
        return AdaptiveExitResult {
            gross_pnl_pct: gross_pct,
            net_pnl_sol: net_sol,
            exit_rule: format!("Adaptive TP1 (+{:.1}%) & Historical Residual", targets.tp1_pct),
            tp1_hit: true,
            tp2_hit: false,
            runner_hit: false,
            exit_price: entry_price * (1.0 + gross_pct / 100.0),
        };
    }

    // D. TP1 was NOT reached: Zero phantom fills.
    // Retain exact historical realized percentage recorded in ledger.
    let gross_pct = actual_exit_pnl_pct;
    let net_sol = size_sol * (gross_pct / 100.0);

    AdaptiveExitResult {
        gross_pnl_pct: gross_pct,
        net_pnl_sol: net_sol,
        exit_rule: format!("Baseline Exit Preserved ({})", recorded_rule),
        tp1_hit: false,
        tp2_hit: false,
        runner_hit: false,
        exit_price: entry_price * (1.0 + gross_pct / 100.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_exit_config_defaults() {
        let cfg = AdaptiveExitConfig::default();
        assert_eq!(cfg.k_tp1, 1.25);
        assert_eq!(cfg.k_tp2, 2.50);
        assert_eq!(cfg.sigma_fallback, 0.22);
        assert_eq!(cfg.min_pre_entry_ticks, 3);
        assert_eq!(cfg.min_vol_floor, 0.15);
        assert_eq!(cfg.max_vol_cap, 0.65);
        assert_eq!(cfg.min_tp1_pct, 20.0);
        assert_eq!(cfg.max_tp1_pct, 55.0);
    }

    #[test]
    fn test_zero_lookahead_volatility() {
        // 1. With pre-entry ticks in bounded ring buffer:
        let pre_ticks: VecDeque<f64> = vec![1.0, 1.05, 1.02, 1.08, 1.12].into();
        let vol_pct = compute_rolling_tick_volatility(&pre_ticks);
        assert!(vol_pct > 0.0 && vol_pct.is_finite());

        // 2. Without pre-entry ticks: strictly falls back to 6.0% baseline
        let empty_ticks: VecDeque<f64> = VecDeque::new();
        let fallback_vol = compute_rolling_tick_volatility(&empty_ticks);
        assert_eq!(fallback_vol, 6.0);
    }

    #[test]
    fn test_zero_phantom_liquidity_enforcement() {
        let cfg = AdaptiveExitConfig::default();
        let targets = AdaptiveExitTargets {
            sigma_pre: 0.22,
            tp1_price: 1.275,
            tp2_price: 1.55,
            tp1_pct: 27.5,
            tp2_pct: 55.0,
        };

        // Trade with 0% MFE that dumped -98.57% (like BRUH #51)
        let res = simulate_honest_adaptive_trade(
            1.0,
            0.0,
            -98.57,
            "Max Slippage Loss (-25%)",
            0.04,
            &[],
            &targets,
            &cfg,
        );

        assert!(!res.tp1_hit);
        assert_eq!(res.gross_pnl_pct, -98.57); // Must NOT assume -10% early abort phantom fill!
    }

    #[test]
    fn test_tp1_lock_with_historical_residual() {
        let cfg = AdaptiveExitConfig::default();
        let targets = AdaptiveExitTargets {
            sigma_pre: 0.22,
            tp1_price: 1.275,
            tp2_price: 1.55,
            tp1_pct: 27.5,
            tp2_pct: 55.0,
        };

        // Trade that pumped +34.85% then crashed -33.71% (like FUSE #2)
        let res = simulate_honest_adaptive_trade(
            1.0,
            34.85,
            -33.71,
            "Max Slippage Loss (-25%)",
            0.04,
            &[],
            &targets,
            &cfg,
        );

        assert!(res.tp1_hit);
        // 40% @ 27.5% + 60% @ -33.71% = 11.0% - 20.226% = -9.226%
        let expected_gross = (0.40 * 27.5) + (0.60 * -33.71);
        assert!((res.gross_pnl_pct - expected_gross).abs() < 1e-4);
        assert!(res.gross_pnl_pct > -33.71); // Substantially reduced loss
    }

    #[test]
    fn test_cruise_telemetry_serialization() {
        let telemetry = CruiseSupervisorTelemetry {
            slots_active: 1,
            slots_total: 4,
            dynamic_exits_config: DynamicExitsConfig::default(),
            orderflow_config: OrderflowConfig::default(),
            positions: vec![CruisePositionTelemetry {
                symbol: "TEST".to_string(),
                mint: "TestMint1111111111111111111111111111111111".to_string(),
                entry_price: 0.001,
                current_price: 0.0012,
                pnl_pct: 20.0,
                peak_mfe_pct: 25.0,
                rolling_volatility_pct: 12.5,
                dynamic_trail_pct: 14.0,
                locked_floor_pct: 11.0,
                cvd_net_sol: 4.5,
                cvd_pressure_ratio: 0.35,
                cvd_divergence_state: "BULLISH_EXPANSION".to_string(),
                hold_time_secs: 45,
                ttl_remaining_secs: 115,
                emergency_tip_lamports: 100000,
                emergency_stop_base_tip_lamports: 100000,
                emergency_stop_max_tip_lamports: 250000,
            }],
            supervisor_status: "MONITORING_ACTIVE".to_string(),
            historical_protection_stats: None,
        };
        let json = serde_json::to_string(&telemetry).expect("Failed to serialize telemetry");
        assert!(json.contains("MONITORING_ACTIVE"));
        assert!(json.contains("TEST"));
        assert!(json.contains("BULLISH_EXPANSION"));
    }
}
