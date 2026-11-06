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
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExitAction {
    Hold,
    PartialSell {
        sell_fraction_of_remaining: f64,
        sol_basis_closed: f64,
        rule_label: String,
    },
    FullExit {
        rule_label: String,
        is_toxic: bool,
    },
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

    // 1. Max Adverse Slippage Cutoff
    if pnl_pct <= cfg.max_slippage_loss_pct {
        return ExitAction::FullExit {
            rule_label: format!("Max Slippage Loss ({:.0}%)", cfg.max_slippage_loss_pct),
            is_toxic: true,
        };
    }

    // 2. Hard Stop-Loss
    if pnl_pct <= cfg.stop_loss_pct {
        return ExitAction::FullExit {
            rule_label: format!("Stop-Loss ({:.0}%)", cfg.stop_loss_pct),
            is_toxic: true,
        };
    }

    // 3. Flash Dump Circuit Breaker (Only triggers when underwater <= flash_dump_max_pnl_pct)
    if tick_drop_pct <= cfg.flash_dump_tick_pct && pnl_pct <= cfg.flash_dump_max_pnl_pct {
        return ExitAction::FullExit {
            rule_label: format!("Flash Dump Breaker ({:.0}%)", cfg.flash_dump_tick_pct),
            is_toxic: true,
        };
    }

    // 4. Tier 1 Take-Profit
    if !pos.tp1_executed && pnl_pct >= cfg.tp1_trigger_pct {
        pos.tp1_executed = true;
        let sol_closed = pos.initial_size_sol * cfg.tp1_sell_fraction;
        pos.remaining_size_sol = (pos.remaining_size_sol - sol_closed).max(0.0);

        return ExitAction::PartialSell {
            sell_fraction_of_remaining: cfg.tp1_sell_fraction,
            sol_basis_closed: sol_closed,
            rule_label: format!("TP1 Scale-Out (+{:.0}%)", cfg.tp1_trigger_pct),
        };
    }

    // 5. Tier 2 Take-Profit
    if pos.tp1_executed && !pos.tp2_executed && pnl_pct >= cfg.tp2_trigger_pct {
        pos.tp2_executed = true;
        let sol_closed = pos.initial_size_sol * cfg.tp2_sell_fraction;
        let fraction_of_current = cfg.tp2_sell_fraction / (1.0 - cfg.tp1_sell_fraction);
        pos.remaining_size_sol = (pos.remaining_size_sol - sol_closed).max(0.0);

        return ExitAction::PartialSell {
            sell_fraction_of_remaining: fraction_of_current.min(1.0),
            sol_basis_closed: sol_closed,
            rule_label: format!("TP2 Scale-Out (+{:.0}%)", cfg.tp2_trigger_pct),
        };
    }

    // 6. Moonbag Trailing Stop (True relative drawdown from high-water mark price)
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
        };
    }

    // 7. Uncapped Breakeven Guard
    if !pos.tp1_executed
        && pos.peak_pnl_pct >= cfg.breakeven_arm_pct
        && pnl_pct <= cfg.breakeven_floor_pct
    {
        return ExitAction::FullExit {
            rule_label: format!("Breakeven Guard (+{:.0}%)", cfg.breakeven_floor_pct),
            is_toxic: false,
        };
    }

    // 8. Conditional Stagnation TTL
    if !pos.tp1_executed
        && elapsed_secs >= cfg.stagnation_ttl_secs as u64
        && pos.peak_pnl_pct < cfg.stagnation_max_peak_pct
        && pnl_pct >= cfg.stagnation_band_min_pct
        && pnl_pct <= cfg.stagnation_band_max_pct
    {
        return ExitAction::FullExit {
            rule_label: "Stagnation TTL Exit".to_string(),
            is_toxic: false,
        };
    }

    ExitAction::Hold
}

pub fn is_historical_exit_toxic(exit_pnl_pct: f64, cfg: &RiskConfig) -> bool {
    exit_pnl_pct <= cfg.stop_loss_pct || exit_pnl_pct <= cfg.flash_dump_tick_pct
}

pub fn is_toxic_exit(reason: &str, pnl_pct: f64, cfg: &RiskConfig) -> bool {
    reason.contains("Stop-Loss")
        || reason.contains("Flash Dump")
        || reason.contains("Max Slippage")
        || is_historical_exit_toxic(pnl_pct, cfg)
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
        }
    }

    #[test]
    fn test_hard_stop_loss() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();
        let action = evaluate_position_exit(&mut pos, 0.79, 1050, &cfg);
        match action {
            ExitAction::FullExit { rule_label, is_toxic } => {
                assert!(rule_label.contains("Stop-Loss"));
                assert!(is_toxic);
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
            ExitAction::FullExit { rule_label, is_toxic } => {
                assert!(rule_label.contains("Flash Dump Breaker"));
                assert!(is_toxic);
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
            ExitAction::PartialSell { sell_fraction_of_remaining, sol_basis_closed, .. } => {
                assert_eq!(sell_fraction_of_remaining, 0.40);
                assert!((sol_basis_closed - 0.04).abs() < 1e-6);
                assert!((pos.remaining_size_sol - 0.06).abs() < 1e-6);
                assert!(pos.tp1_executed);
            },
            _ => panic!("Expected TP1 PartialSell"),
        }

        // TP2 at +90% (price 1.90)
        let action2 = evaluate_position_exit(&mut pos, 1.90, 1060, &cfg);
        match action2 {
            ExitAction::PartialSell { sol_basis_closed, .. } => {
                assert!((sol_basis_closed - 0.035).abs() < 1e-6);
                assert!((pos.remaining_size_sol - 0.025).abs() < 1e-6); // 25% moonbag runner
                assert!(pos.tp2_executed);
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
            ExitAction::FullExit { rule_label, is_toxic } => {
                assert!(rule_label.contains("Trailing Runner (-18% from +60.0% Peak)"));
                assert!(!is_toxic);
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

        // Retrace to +11% (below +12% floor)
        let action = evaluate_position_exit(&mut pos, 1.11, 1060, &cfg);
        match action {
            ExitAction::FullExit { rule_label, is_toxic } => {
                assert!(rule_label.contains("Breakeven Guard"));
                assert!(!is_toxic);
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
            ExitAction::FullExit { rule_label, is_toxic } => {
                assert!(rule_label.contains("Stagnation TTL"));
                assert!(!is_toxic);
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
            ExitAction::FullExit { rule_label, is_toxic } => {
                assert!(rule_label.contains("Max Slippage Loss"));
                assert!(is_toxic);
            }
            _ => panic!("Expected Max Slippage Loss exit"),
        }
    }

    #[test]
    fn test_is_historical_exit_toxic() {
        let cfg = RiskConfig::default();
        // stop_loss_pct = -20.0, flash_dump_tick_pct = -15.0
        assert!(is_historical_exit_toxic(-20.0, &cfg));
        assert!(is_historical_exit_toxic(-22.0, &cfg));
        assert!(is_historical_exit_toxic(-15.0, &cfg));
        assert!(is_historical_exit_toxic(-17.5, &cfg));
        assert!(!is_historical_exit_toxic(-10.0, &cfg));
        assert!(!is_historical_exit_toxic(0.0, &cfg));
        assert!(!is_historical_exit_toxic(45.0, &cfg));
    }

    #[test]
    fn test_toxic_exit_detection() {
        let cfg = RiskConfig::default();
        assert!(is_toxic_exit("Stop-Loss (-20%)", -20.5, &cfg));
        assert!(is_toxic_exit("Flash Dump Breaker (-15%)", -16.0, &cfg));
        assert!(is_toxic_exit("Max Slippage Loss (-25%)", -26.0, &cfg));
        assert!(!is_toxic_exit("TP1 Scale-Out (+45%)", 45.0, &cfg));
        assert!(!is_toxic_exit("Trailing Runner (Peak +60.0%)", 41.0, &cfg));
    }

    #[test]
    fn test_format_position_status() {
        let mut pos = setup_test_position();
        let cfg = RiskConfig::default();

        assert_eq!(format_position_status(&pos, &cfg), "Monitoring Momentum");

        pos.peak_pnl_pct = 36.0;
        assert_eq!(format_position_status(&pos, &cfg), "Breakeven Armed (+12% Floor)");

        pos.tp1_executed = true;
        assert_eq!(format_position_status(&pos, &cfg), "TP1 Harvested (+45%)");

        pos.tp2_executed = true;
        assert_eq!(format_position_status(&pos, &cfg), "25% Runner Moonbag");
    }
}
