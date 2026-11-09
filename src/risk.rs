#![allow(dead_code)]
// DMNE QUANT // RISK MANAGEMENT & EXIT ARBITRATION SUBSYSTEM
// Multi-Tiered Convex Exit Engine, Uncapped Breakeven Floor Guard & Moonbag Trailing Runner

use std::collections::VecDeque;
use serde::{Deserialize, Serialize};
use crate::config::RiskConfig;
#[allow(unused_imports)]
pub use crate::adaptive_exits::{
    compute_rolling_tick_volatility, compute_volatility_scaled_exit_band, VolatilityScaledExitBand,
};

pub const DEFAULT_PRICE_BUFFER_CAPACITY: usize = 30;

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
    #[serde(default)]
    pub price_buffer: VecDeque<f64>,
    #[serde(default)]
    pub cvd_tracker: crate::orderflow::RollingCvdTracker,
}

impl ActivePosition {
    pub fn record_price_tick(&mut self, price: f64, window_size: usize) {
        if self.price_buffer.len() >= window_size && window_size > 0 {
            self.price_buffer.pop_front();
        }
        self.price_buffer.push_back(price);
    }

    pub fn record_trade_tick(&mut self, timestamp: f64, volume_sol: f64, is_buy: bool) {
        self.cvd_tracker.record_trade(timestamp, volume_sol, is_buy);
    }
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

pub fn resolve_exit_tip(rule_label: &str, pnl_sol: f64, _cfg: &RiskConfig) -> u64 {
    if is_emergency_risk_exit(rule_label) || pnl_sol <= 0.0 {
        // Defensive exits (Stop-Loss, Early Abort, Flash Dump, Max Slippage) and any PnL <= 0: return 10_000 lamports
        10_000
    } else {
        // Profitable exits (TP1, TP2, Dynamic Ratchet): scale tip dynamically:
        // max(10_000, min(150_000, (pnl_sol * 1e9 * 0.08) as u64))
        let scaled = (pnl_sol * 1e9 * 0.08) as u64;
        scaled.clamp(10_000, 150_000)
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
    let est_full_pnl_sol = pos.remaining_size_sol * (pnl_pct / 100.0);

    // 1. Max Adverse Slippage Cutoff (Emergency defensive exit: floored at 10k)
    if pnl_pct <= cfg.max_slippage_loss_pct {
        let rule_label = format!("Max Slippage Loss ({:.0}%)", cfg.max_slippage_loss_pct);
        let tip_lamports = resolve_exit_tip(&rule_label, est_full_pnl_sol, cfg);
        return ExitAction::FullExit {
            rule_label,
            is_toxic: true,
            tip_lamports,
        };
    }

    // 2. Hard Stop-Loss (Emergency defensive exit: floored at 10k)
    if pnl_pct <= cfg.stop_loss_pct {
        let rule_label = format!("Stop-Loss ({:.0}%)", cfg.stop_loss_pct);
        let tip_lamports = resolve_exit_tip(&rule_label, est_full_pnl_sol, cfg);
        return ExitAction::FullExit {
            rule_label,
            is_toxic: true,
            tip_lamports,
        };
    }

    // 3. Flash Dump Circuit Breaker (Emergency defensive exit: floored at 10k)
    if tick_drop_pct <= cfg.flash_dump_tick_pct && pnl_pct <= cfg.flash_dump_max_pnl_pct {
        let rule_label = format!("Flash Dump Breaker ({:.0}%)", cfg.flash_dump_tick_pct);
        let tip_lamports = resolve_exit_tip(&rule_label, est_full_pnl_sol, cfg);
        return ExitAction::FullExit {
            rule_label,
            is_toxic: true,
            tip_lamports,
        };
    }

    // 3b. Early Fail-Fast Abort for Dead-on-Arrival Entries (Emergency defensive exit: floored at 10k)
    if !pos.tp1_executed && elapsed_secs >= cfg.early_abort_window_secs && pos.peak_pnl_pct < cfg.early_abort_max_peak_pct && pnl_pct <= cfg.early_abort_loss_pct {
        let rule_label = format!("Early Abort ({:.0}%)", cfg.early_abort_loss_pct);
        let tip_lamports = resolve_exit_tip(&rule_label, est_full_pnl_sol, cfg);
        return ExitAction::FullExit {
            rule_label,
            is_toxic: true,
            tip_lamports,
        };
    }

    // 4. Tier 1 Take-Profit (Profitable exit: dynamic tip scaling)
    if !pos.tp1_executed && pnl_pct >= cfg.tp1_trigger_pct {
        pos.tp1_executed = true;
        let sol_closed = pos.initial_size_sol * cfg.tp1_sell_fraction;
        pos.remaining_size_sol = (pos.remaining_size_sol - sol_closed).max(0.0);
        let rule_label = format!("TP1 Scale-Out (+{:.0}%)", cfg.tp1_trigger_pct);
        let est_pnl_sol = sol_closed * (pnl_pct / 100.0);
        let tip_lamports = resolve_exit_tip(&rule_label, est_pnl_sol, cfg);

        return ExitAction::PartialSell {
            sell_fraction_of_remaining: cfg.tp1_sell_fraction,
            sol_basis_closed: sol_closed,
            rule_label,
            tip_lamports,
        };
    }

    // 5. Tier 2 Take-Profit (Profitable exit: dynamic tip scaling)
    if pos.tp1_executed && !pos.tp2_executed && pnl_pct >= cfg.tp2_trigger_pct {
        pos.tp2_executed = true;
        let sol_closed = pos.initial_size_sol * cfg.tp2_sell_fraction;
        let fraction_of_current = cfg.tp2_sell_fraction / (1.0 - cfg.tp1_sell_fraction);
        pos.remaining_size_sol = (pos.remaining_size_sol - sol_closed).max(0.0);
        let rule_label = format!("TP2 Scale-Out (+{:.0}%)", cfg.tp2_trigger_pct);
        let est_pnl_sol = sol_closed * (pnl_pct / 100.0);
        let tip_lamports = resolve_exit_tip(&rule_label, est_pnl_sol, cfg);

        return ExitAction::PartialSell {
            sell_fraction_of_remaining: fraction_of_current.min(1.0),
            sol_basis_closed: sol_closed,
            rule_label,
            tip_lamports,
        };
    }

    // Volatility-Scaled Dynamic Exit Bands evaluation
    let dyn_band = if cfg.dynamic_exits.enabled {
        Some(compute_volatility_scaled_exit_band(
            &pos.price_buffer,
            pos.peak_pnl_pct,
            &cfg.dynamic_exits,
        ))
    } else {
        None
    };

    // 6. Moonbag Trailing Stop (Dynamic or Static)
    let peak_mult = 1.0 + (pos.peak_pnl_pct / 100.0);
    let curr_mult = 1.0 + (pnl_pct / 100.0);
    let peak_drawdown_pct = if peak_mult > 0.0 {
        ((peak_mult - curr_mult) / peak_mult) * 100.0
    } else {
        0.0
    };

    // Orderflow CVD Bearish Absorption Divergence Floor Ratchet
    let is_bearish_absorption = if cfg.orderflow.enabled && cfg.orderflow.tighten_exit_on_absorption {
        let reference_pnl = pnl_pct.max(pos.peak_pnl_pct);
        pos.cvd_tracker.evaluate_divergence(
            reference_pnl,
            cfg.orderflow.bearish_absorption_threshold,
        ) == crate::orderflow::CvdDivergence::BearishAbsorption
    } else {
        false
    };

    if pos.tp1_executed {
        if let Some(ref band) = dyn_band {
            let effective_floor = if is_bearish_absorption {
                band.dynamic_floor.max(pos.peak_pnl_pct - (band.delta_trail * 0.5)).max(cfg.breakeven_floor_pct)
            } else {
                band.dynamic_floor
            };
            let effective_delta = if is_bearish_absorption {
                band.delta_trail * 0.5
            } else {
                band.delta_trail
            };
            if peak_drawdown_pct >= effective_delta || pnl_pct <= effective_floor {
                let rule_prefix = if is_bearish_absorption {
                    "CVD Absorption Ratchet Runner"
                } else {
                    "Dynamic Trailing Runner"
                };
                let rule_label = format!(
                    "{} (-{:.1}% from +{:.1}% Peak | Floor +{:.1}%)",
                    rule_prefix, effective_delta, pos.peak_pnl_pct, effective_floor
                );
                let tip_lamports = resolve_exit_tip(&rule_label, est_full_pnl_sol, cfg);
                return ExitAction::FullExit {
                    rule_label,
                    is_toxic: false,
                    tip_lamports,
                };
            }
        } else {
            let effective_trail = if is_bearish_absorption {
                cfg.runner_trailing_drop_pct * 0.5
            } else {
                cfg.runner_trailing_drop_pct
            };
            if peak_drawdown_pct >= effective_trail {
                let rule_prefix = if is_bearish_absorption {
                    "CVD Absorption Trailing Runner"
                } else {
                    "Trailing Runner"
                };
                let rule_label = format!(
                    "{} (-{:.0}% from +{:.1}% Peak)",
                    rule_prefix, effective_trail, pos.peak_pnl_pct
                );
                let tip_lamports = resolve_exit_tip(&rule_label, est_full_pnl_sol, cfg);
                return ExitAction::FullExit {
                    rule_label,
                    is_toxic: false,
                    tip_lamports,
                };
            }
        }
    }

    // 6b. Dynamic Ratchet Floor for Pre-TP1 Runups (MFE >= mfe_activation_pct or CVD Absorption)
    if !pos.tp1_executed {
        if is_bearish_absorption && pos.peak_pnl_pct >= cfg.breakeven_floor_pct {
            let absorption_floor = (pos.peak_pnl_pct * 0.80).max(cfg.breakeven_floor_pct);
            if pnl_pct <= absorption_floor {
                let rule_label = format!(
                    "CVD Absorption Ratchet Floor (+{:.1}% from +{:.1}% Peak)",
                    absorption_floor, pos.peak_pnl_pct
                );
                let tip_lamports = resolve_exit_tip(&rule_label, est_full_pnl_sol, cfg);
                return ExitAction::FullExit {
                    rule_label,
                    is_toxic: false,
                    tip_lamports,
                };
            }
        } else if let Some(ref band) = dyn_band {
            if band.is_ratchet_active && pnl_pct <= band.dynamic_floor {
                let rule_label = format!(
                    "Dynamic Ratchet Floor (+{:.1}% from +{:.1}% Peak)",
                    band.dynamic_floor, pos.peak_pnl_pct
                );
                let tip_lamports = resolve_exit_tip(&rule_label, est_full_pnl_sol, cfg);
                return ExitAction::FullExit {
                    rule_label,
                    is_toxic: false,
                    tip_lamports,
                };
            }
        }
    }

    // 7. Uncapped Breakeven Guard (Profitable exit: dynamic tip scaling)
    if !pos.tp1_executed
        && pos.peak_pnl_pct >= cfg.breakeven_arm_pct
        && pnl_pct <= cfg.breakeven_floor_pct
    {
        let rule_label = format!("Breakeven Guard (+{:.0}%)", cfg.breakeven_floor_pct);
        let tip_lamports = resolve_exit_tip(&rule_label, est_full_pnl_sol, cfg);
        return ExitAction::FullExit {
            rule_label,
            is_toxic: false,
            tip_lamports,
        };
    }

    // 8. Conditional Stagnation TTL
    if !pos.tp1_executed
        && elapsed_secs >= cfg.stagnation_ttl_secs
        && pos.peak_pnl_pct < cfg.stagnation_max_peak_pct
        && pnl_pct >= cfg.stagnation_band_min_pct
        && pnl_pct <= cfg.stagnation_band_max_pct
    {
        let rule_label = "Stagnation TTL Exit".to_string();
        let tip_lamports = resolve_exit_tip(&rule_label, est_full_pnl_sol, cfg);
        return ExitAction::FullExit {
            rule_label,
            is_toxic: false,
            tip_lamports,
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
    let gross_sol = basis_sol * (gross_pnl_pct / 100.0);
    let tip = resolve_exit_tip(rule_label, gross_sol, cfg);
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
    Tier1,
    Tier2,
    Tier3,
    LowVelocity,
    Baseline,
    HighConviction,
}

impl ConvictionTier {
    pub fn label(&self) -> &'static str {
        match self {
            ConvictionTier::Tier3 | ConvictionTier::HighConviction => "HIGH_CONVICTION",
            ConvictionTier::Tier2 | ConvictionTier::Baseline => "BASELINE",
            ConvictionTier::Tier1 | ConvictionTier::LowVelocity => "LOW_VELOCITY",
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
        ConvictionTier::Tier3 | ConvictionTier::HighConviction => cfg.max_position_pct_nav * 100.0,
        ConvictionTier::Tier2 | ConvictionTier::Baseline => (cfg.base_position_pct_nav * 1.35).min(cfg.max_position_pct_nav) * 100.0,
        ConvictionTier::Tier1 | ConvictionTier::LowVelocity => cfg.base_position_pct_nav * 100.0,
    }
}

/// Dynamic Conviction-Tiered Kelly Sizing with floating NAV-relative cap
pub fn compute_dynamic_entry_size_sol(
    bankroll_sol: f64,
    conviction_tier: ConvictionTier,
    active_slots_used: usize,
    cfg: &RiskConfig,
) -> f64 {
    // Headroom guard: if maximum active slots are already full, no new position can be opened
    if active_slots_used >= cfg.max_active_slots {
        return 0.0;
    }

    // Solvency guard: ensure minimum operational gas reserve remains untouched
    let available_capital = (bankroll_sol - cfg.min_gas_reserve_sol).max(0.0);
    if available_capital < 0.010 {
        return 0.0;
    }

    let dynamic_fraction = match conviction_tier {
        ConvictionTier::Tier1 | ConvictionTier::LowVelocity => cfg.base_position_pct_nav, // 4.5%
        ConvictionTier::Tier2 | ConvictionTier::Baseline => (cfg.base_position_pct_nav * 1.35).min(cfg.max_position_pct_nav), // ~6.075%
        ConvictionTier::Tier3 | ConvictionTier::HighConviction => cfg.max_position_pct_nav, // 8.5%
    };

    let unconstrained_sol = bankroll_sol * dynamic_fraction;
    let floating_cap = bankroll_sol * cfg.max_position_pct_nav;
    let absolute_cap = cfg.max_entry_sol.min(0.10); // Standard circuit breaker
    let final_cap = floating_cap.min(absolute_cap);

    // Return dynamic size bounded between 0.010 SOL floor and final_cap
    let size = unconstrained_sol.min(final_cap).max(0.010);

    if available_capital < size {
        if available_capital < 0.010 {
            return 0.0;
        }
        return available_capital.min(final_cap);
    }

    size
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
            price_buffer: VecDeque::new(),
            cvd_tracker: crate::orderflow::RollingCvdTracker::default(),
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
                assert_eq!(tip_lamports, 10_000);
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
                assert_eq!(tip_lamports, 10_000);
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
                assert_eq!(tip_lamports, 150_000);
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
                assert_eq!(tip_lamports, 150_000);
            },
            _ => panic!("Expected TP2 PartialSell"),
        }
    }

    #[test]
    fn test_moonbag_trailing_runner() {
        let mut pos = setup_test_position();
        let mut cfg = RiskConfig::default();
        cfg.dynamic_exits.enabled = false;

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
                assert_eq!(tip_lamports, 150_000);
            }
            _ => panic!("Expected Trailing Runner exit"),
        }
    }

    #[test]
    fn test_breakeven_guard() {
        let mut pos = setup_test_position();
        let mut cfg = RiskConfig::default();
        cfg.dynamic_exits.enabled = false;

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
                assert_eq!(tip_lamports, 150_000);
            }
            _ => panic!("Expected Breakeven Guard exit"),
        }
    }

    #[test]
    fn test_volatility_scaled_dynamic_exits() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();
        assert!(cfg.dynamic_exits.enabled);

        // Feed some realistic ticks with ~8% volatility
        pos.price_buffer = vec![1.00, 1.08, 1.02, 1.10, 1.05, 1.15, 1.25, 1.35].into();
        let sigma = compute_rolling_tick_volatility(&pos.price_buffer);
        assert!(sigma > 0.0);

        // 1. Dynamic Ratchet Floor test (Peak +35% >= 30% MFE activation, but before TP1)
        let _ = evaluate_position_exit(&mut pos, 1.35, 1050, &cfg);
        assert!(!pos.tp1_executed);
        assert!((pos.peak_pnl_pct - 35.0).abs() < 1e-6);

        let band = compute_volatility_scaled_exit_band(&pos.price_buffer, pos.peak_pnl_pct, &cfg.dynamic_exits);
        assert!(band.is_ratchet_active);
        assert!(band.dynamic_floor >= cfg.dynamic_exits.min_locked_floor_pct);

        // Retrace to below dynamic_floor (e.g. at price 1.10 = +10% PnL < dynamic_floor >= 12%)
        let action = evaluate_position_exit(&mut pos, 1.10, 1060, &cfg);
        match action {
            ExitAction::FullExit { rule_label, is_toxic, tip_lamports } => {
                assert!(rule_label.contains("Dynamic Ratchet Floor"));
                assert!(!is_toxic);
                assert_eq!(tip_lamports, 150_000);
            }
            _ => panic!("Expected Dynamic Ratchet Floor exit"),
        }

        // 2. Dynamic Trailing Runner test after TP1 (+60%)
        let mut pos2 = setup_test_position();
        pos2.price_buffer = vec![1.00, 1.10, 1.20, 1.35, 1.50, 1.60].into();
        let _ = evaluate_position_exit(&mut pos2, 1.60, 1050, &cfg);
        assert!(pos2.tp1_executed);

        // Pull down from 1.60 to 1.30 (drawdown = (1.60 - 1.30)/1.60 = 18.75%)
        let action2 = evaluate_position_exit(&mut pos2, 1.30, 1065, &cfg);
        match action2 {
            ExitAction::FullExit { rule_label, is_toxic, tip_lamports } => {
                assert!(rule_label.contains("Dynamic Trailing Runner"));
                assert!(!is_toxic);
                assert_eq!(tip_lamports, 150_000);
            }
            _ => panic!("Expected Dynamic Trailing Runner exit"),
        }
    }

    #[test]
    fn test_price_buffer_bounded_capacity() {
        let mut pos = setup_test_position();
        assert_eq!(pos.price_buffer.len(), 0);

        for i in 1..=100 {
            pos.record_price_tick(i as f64, DEFAULT_PRICE_BUFFER_CAPACITY);
        }

        assert_eq!(pos.price_buffer.len(), DEFAULT_PRICE_BUFFER_CAPACITY);
        assert_eq!(pos.price_buffer.len(), 30);

        // Verify the 30 most recent ticks (71 to 100) are preserved in chronological order
        let expected: Vec<f64> = (71..=100).map(|v| v as f64).collect();
        let actual: Vec<f64> = pos.price_buffer.iter().copied().collect();
        assert_eq!(actual, expected);
        assert_eq!(*pos.price_buffer.front().unwrap(), 71.0);
        assert_eq!(*pos.price_buffer.back().unwrap(), 100.0);

        // Verify volatility computation over ring buffer wraparound slices without allocation
        let vol = compute_rolling_tick_volatility(&pos.price_buffer);
        assert!(vol.is_finite() && vol > 0.0);
    }

    #[test]
    fn test_cvd_bearish_absorption_tightens_runner_exit() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();

        // 1. Position reached +50% peak gain
        let _ = evaluate_position_exit(&mut pos, 1.50, 1050, &cfg);
        assert!(pos.tp1_executed);
        assert_eq!(pos.peak_pnl_pct, 50.0);

        // 2. Heavy sell pressure arrives: 20 buy vs 80 sell -> pressure ratio = -0.60
        pos.record_trade_tick(1060.0, 20.0, true);
        pos.record_trade_tick(1061.0, 80.0, false);
        assert_eq!(pos.cvd_tracker.pressure_ratio(), -0.60);

        // 3. Price slightly pulls back from 1.50 to 1.35 (+35% PnL)
        // With absorption, floor is ratcheted up and triggers protective exit.
        let action = evaluate_position_exit(&mut pos, 1.35, 1065, &cfg);
        match action {
            ExitAction::FullExit { rule_label, is_toxic, tip_lamports } => {
                assert!(rule_label.contains("CVD Absorption Ratchet Runner"));
                assert!(!is_toxic);
                assert_eq!(tip_lamports, 150_000);
            }
            _ => panic!("Expected CVD Absorption Ratchet Runner exit, got {:?}", action),
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
                assert_eq!(tip_lamports, 10_000);
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
                assert_eq!(tip_lamports, 10_000);
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
        // Defensive risk exits floor at 10,000 lamports (saving 86.7% overhead)
        assert_eq!(resolve_exit_tip("Stop-Loss (-20%)", -0.010, &cfg), 10_000);
        assert_eq!(resolve_exit_tip("Flash Dump Breaker (-15%)", -0.008, &cfg), 10_000);
        assert_eq!(resolve_exit_tip("Max Slippage Loss (-25%)", -0.015, &cfg), 10_000);
        assert_eq!(resolve_exit_tip("Early Abort (-10%)", -0.005, &cfg), 10_000);

        // Profitable exits scale tip dynamically: max(10_000, min(150_000, pnl_sol * 1e9 * 0.08))
        assert_eq!(resolve_exit_tip("TP1 Scale-Out (+45%)", 0.0001, &cfg), 10_000);
        assert_eq!(resolve_exit_tip("TP2 Scale-Out (+90%)", 0.0010, &cfg), 80_000);
        assert_eq!(resolve_exit_tip("Trailing Runner (-18% from +50.0% Peak)", 0.010, &cfg), 150_000);
        assert_eq!(resolve_exit_tip("Breakeven Guard (+8%)", 0.0005, &cfg), 40_000);
        assert_eq!(resolve_exit_tip("Stagnation TTL Exit", 0.0000, &cfg), 10_000);
    }

    #[test]
    fn test_compute_net_realized_sol_with_emergency_tip() {
        let cfg = RiskConfig::default();
        let basis_sol: f64 = 0.10;
        let gross_pnl_pct: f64 = -20.0;

        let net_standard = compute_net_realized_sol(basis_sol, gross_pnl_pct, &cfg);
        let net_emergency = compute_net_realized_sol_for_exit(basis_sol, gross_pnl_pct, "Stop-Loss (-20%)", &cfg);

        // Defensive exit floors tip at 10,000 lamports, matching standard baseline tip
        assert!((net_standard - net_emergency).abs() < 1e-12);
    }

    #[test]
    fn test_conviction_tier_evaluation() {
        let cfg = RiskConfig::default();

        // 1. High conviction: buy_sell_ratio >= 2.0 (allocates 8.5%)
        let tier_high = evaluate_conviction_tier(2.2, 70.0, 30, 5000.0, &cfg);
        assert_eq!(tier_high, ConvictionTier::HighConviction);
        assert!((resolve_kelly_fraction_pct(tier_high, &cfg) - 8.5).abs() < 1e-4);

        // 2. High conviction via top conviction %: conviction >= 85.0%
        let tier_high_conv = evaluate_conviction_tier(1.6, 90.0, 30, 5000.0, &cfg);
        assert_eq!(tier_high_conv, ConvictionTier::HighConviction);

        // 3. Baseline tier: standard momentum (~6.075%)
        let tier_base = evaluate_conviction_tier(1.6, 70.0, 30, 5000.0, &cfg);
        assert_eq!(tier_base, ConvictionTier::Baseline);
        assert!((resolve_kelly_fraction_pct(tier_base, &cfg) - 6.075).abs() < 1e-4);

        // 4. Low velocity tier: borderline buy/sell ratio or low volume (4.5%)
        let tier_low = evaluate_conviction_tier(1.30, 60.0, 16, 2800.0, &cfg);
        assert_eq!(tier_low, ConvictionTier::LowVelocity);
        assert!((resolve_kelly_fraction_pct(tier_low, &cfg) - 4.5).abs() < 1e-4);
    }

    #[test]
    fn test_compute_dynamic_entry_size_sol_tiers_and_guards() {
        let cfg = RiskConfig::default();

        // 1. Unconstrained scaling with lower bankroll (0.25 SOL)
        let small_bankroll = 0.25;
        let size_high_small = compute_dynamic_entry_size_sol(small_bankroll, ConvictionTier::HighConviction, 0, &cfg);
        let size_base_small = compute_dynamic_entry_size_sol(small_bankroll, ConvictionTier::Baseline, 0, &cfg);
        let size_low_small = compute_dynamic_entry_size_sol(small_bankroll, ConvictionTier::LowVelocity, 0, &cfg);

        // High tier (8.5% of 0.25 = 0.02125 SOL) > Base (~6.075% of 0.25 = 0.0151875 SOL) > Low (4.5% of 0.25 = 0.01125 SOL)
        assert!(size_high_small > size_base_small);
        assert!(size_base_small > size_low_small);
        assert!((size_high_small - 0.02125).abs() < 1e-4);
        assert!((size_base_small - 0.0151875).abs() < 1e-4);
        assert!((size_low_small - 0.01125).abs() < 1e-4);

        // 2. Floating NAV-relative dynamic Kelly sizing on larger bankroll (0.50 SOL)
        let bankroll_sol = 0.50;
        let size_high = compute_dynamic_entry_size_sol(bankroll_sol, ConvictionTier::HighConviction, 0, &cfg);
        let size_base = compute_dynamic_entry_size_sol(bankroll_sol, ConvictionTier::Baseline, 0, &cfg);
        let size_low = compute_dynamic_entry_size_sol(bankroll_sol, ConvictionTier::LowVelocity, 0, &cfg);

        assert!(size_high > size_base);
        assert!(size_base > size_low);
        assert!((size_high - 0.0425).abs() < 1e-4);
        assert!((size_base - 0.030375).abs() < 1e-4);
        assert!((size_low - 0.0225).abs() < 1e-4);

        // All sizes capped by floating_cap (8.5% of 0.50 = 0.0425 SOL) and absolute cap
        assert!(size_high <= bankroll_sol * cfg.max_position_pct_nav);
        assert!(size_high <= cfg.max_entry_sol);
        assert!(size_low >= 0.010);

        // 3. Absolute circuit breaker clamp on very large bankroll (2.0 SOL)
        let large_bankroll = 2.00;
        let size_large = compute_dynamic_entry_size_sol(large_bankroll, ConvictionTier::HighConviction, 0, &cfg);
        // Unconstrained: 2.0 * 0.085 = 0.17 SOL, capped by max_entry_sol = 0.060
        assert_eq!(size_large, 0.060);

        // Tier1, Tier2, Tier3 variants alias test
        let size_t3 = compute_dynamic_entry_size_sol(small_bankroll, ConvictionTier::Tier3, 0, &cfg);
        let size_t2 = compute_dynamic_entry_size_sol(small_bankroll, ConvictionTier::Tier2, 0, &cfg);
        let size_t1 = compute_dynamic_entry_size_sol(small_bankroll, ConvictionTier::Tier1, 0, &cfg);
        assert_eq!(size_t3, size_high_small);
        assert_eq!(size_t2, size_base_small);
        assert_eq!(size_t1, size_low_small);

        // Headroom guard: when active slots == max_active_slots (4), return 0.0
        let full_headroom = compute_dynamic_entry_size_sol(bankroll_sol, ConvictionTier::HighConviction, 4, &cfg);
        assert_eq!(full_headroom, 0.0);

        // Solvency guard: when bankroll is near or below min_gas_reserve_sol (0.030), entry is rejected
        let insolvent_bankroll = 0.032; // only 0.002 available above reserve < 0.010 min entry
        let insolvent_size = compute_dynamic_entry_size_sol(insolvent_bankroll, ConvictionTier::HighConviction, 0, &cfg);
        assert_eq!(insolvent_size, 0.0);
    }
}
