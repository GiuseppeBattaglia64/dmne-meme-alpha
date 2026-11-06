// DMNE QUANT // RISK MANAGEMENT & EXIT ARBITRATION SUBSYSTEM
// Multi-Tiered Convex Exit Engine, Uncapped Breakeven Floor Guard & Moonbag Trailing Runner

use serde::{Deserialize, Serialize};
use crate::config::RiskConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivePosition {
    pub token_mint: String,
    pub symbol: String,
    pub entry_price: f64,
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
    },
}

pub fn evaluate_position_exit(
    pos: &mut ActivePosition,
    current_price: f64,
    now_epoch: u64,
    cfg: &RiskConfig,
) -> ExitAction {
    let pnl_pct = (((current_price - pos.entry_price) / pos.entry_price) * 100.0 * 1e6).round() / 1e6;

    // Update high-water mark
    if pnl_pct > pos.peak_pnl_pct {
        pos.peak_pnl_pct = pnl_pct;
    }

    let elapsed_secs = now_epoch.saturating_sub(pos.opened_at_epoch);

    // 0. Max Slippage Loss Protection (-25%)
    if pnl_pct <= cfg.max_slippage_loss_pct {
        return ExitAction::FullExit {
            rule_label: format!("Max Slippage Loss ({:.0}%)", cfg.max_slippage_loss_pct),
        };
    }

    // 1. Hard Stop-Loss (-20%)
    if pnl_pct <= cfg.stop_loss_pct {
        return ExitAction::FullExit {
            rule_label: format!("Stop-Loss ({:.0}%)", cfg.stop_loss_pct),
        };
    }

    // 2. Tier 1 Take-Profit (+45% -> Sell 40% of initial position)
    if !pos.tp1_executed && pnl_pct >= cfg.tp1_trigger_pct {
        pos.tp1_executed = true;
        let sol_closed = pos.initial_size_sol * cfg.tp1_sell_fraction;
        pos.remaining_size_sol = (pos.remaining_size_sol - sol_closed).max(0.0);

        return ExitAction::PartialSell {
            sell_fraction_of_remaining: cfg.tp1_sell_fraction, // 40% of total tokens
            sol_basis_closed: sol_closed,
            rule_label: format!("TP1 Scale-Out (+{:.0}%)", cfg.tp1_trigger_pct),
        };
    }

    // 3. Tier 2 Take-Profit (+90% -> Sell 35% of initial = 58.33% of remaining 60%)
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

    // 4. Moonbag / Post-TP1 Trailing Stop (-18% drawdown from Peak)
    if pos.tp1_executed && pnl_pct <= (pos.peak_pnl_pct - cfg.runner_trailing_drop_pct) {
        return ExitAction::FullExit {
            rule_label: format!(
                "Trailing Runner (Peak +{:.1}%)",
                pos.peak_pnl_pct
            ),
        };
    }

    // 5. Uncapped Breakeven Floor Guard (Armed at +35%, Floor at +12%)
    if !pos.tp1_executed
        && pos.peak_pnl_pct >= cfg.breakeven_arm_pct
        && pnl_pct <= cfg.breakeven_floor_pct
    {
        return ExitAction::FullExit {
            rule_label: format!("Breakeven Guard (+{:.0}%)", cfg.breakeven_floor_pct),
        };
    }

    // 6. Conditional Stagnation TTL (240s, only if token never showed momentum)
    if !pos.tp1_executed
        && elapsed_secs >= cfg.stagnation_ttl_secs as u64
        && pos.peak_pnl_pct < cfg.stagnation_max_peak_pct
        && pnl_pct >= cfg.stagnation_band_min_pct
        && pnl_pct <= cfg.stagnation_band_max_pct
    {
        return ExitAction::FullExit {
            rule_label: "Stagnation TTL Exit".to_string(),
        };
    }

    ExitAction::Hold
}

pub fn is_toxic_exit(reason: &str, pnl_pct: f64, cfg: &RiskConfig) -> bool {
    reason.contains("Stop-Loss")
        || reason.contains("Flash Dump")
        || reason.contains("Max Slippage")
        || pnl_pct <= cfg.stop_loss_pct
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_position() -> ActivePosition {
        ActivePosition {
            token_mint: "EW1b9rcLAJe634mgxb5r4bVYxDoGU37o7YazBzjTpump".to_string(),
            symbol: "RM".to_string(),
            entry_price: 1.0,
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
            ExitAction::FullExit { rule_label } => assert!(rule_label.contains("Stop-Loss")),
            _ => panic!("Expected Stop-Loss exit"),
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

        // Pull back from +60% to +41% (drawdown 19% > 18%)
        let action = evaluate_position_exit(&mut pos, 1.41, 1060, &cfg);
        match action {
            ExitAction::FullExit { rule_label } => assert!(rule_label.contains("Trailing Runner")),
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
            ExitAction::FullExit { rule_label } => assert!(rule_label.contains("Breakeven Guard")),
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
            ExitAction::FullExit { rule_label } => assert!(rule_label.contains("Stagnation TTL")),
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
            ExitAction::FullExit { rule_label } => assert!(rule_label.contains("Max Slippage Loss")),
            _ => panic!("Expected Max Slippage Loss exit"),
        }
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
}

