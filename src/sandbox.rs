#![allow(dead_code)]
// DMNE QUANT // ADVANCED MICROSTRUCTURE SANDBOX MODULE
// Strict Rust Type-System Firewall: PreEntryContext Observable strictly at t <= entry_epoch
// Verifiable Pre-Entry Microstructure Gates & Deterministic v0.7.1 Exit Simulation:
// 1. AntiChurnQuarantineGate (walk-forward toxic ban & reentry cooldown)
// 2. AmmElasticityAndLiquidityGate (liquidity floor, FDV/liq ratio, bonding danger zone, AMM impact)
// 3. SybilAndEntropyGate (symbol character Shannon entropy in bits, outside supply ratio, CU overhead)
// 4. WalkForwardBayesCohortGate (strictly walk-forward Beta-Binomial cohort estimator + quarantine)
// 5. simulate_v071_exit (deterministic multi-tranche exit envelope counterfactual)

use std::collections::HashMap;
use crate::config::{RiskConfig, SandboxConfig};
use crate::risk::{
    compute_dynamic_entry_size_sol, compute_net_pnl_pct, compute_net_realized_sol_with_tip,
    evaluate_conviction_tier, resolve_exit_tip,
};

// ============================================================================
// STRICT TYPE-SYSTEM FIREWALL: PRE-ENTRY CONTEXT (Observable at t <= entry_epoch)
// ============================================================================

#[derive(Debug, Clone)]
pub struct PreEntryContext {
    pub token_mint: String,
    pub symbol: String,
    pub entry_epoch: u64,
    pub entry_price_sol: f64,
    pub size_sol: f64,
    pub liquidity_usd: f64,
    pub fdv_usd: f64,
    pub bonding_curve_pct: f64,
    pub consumed_cu: u64,
    pub jito_tip_lamports: u64,
    pub pre_entry_arrival_timestamps: Vec<f64>,
    pub pre_entry_recent_rejections: usize,
    pub pre_entry_total_observations: usize,
}

// ============================================================================
// GATE 1: ANTI-CHURN QUARANTINE GATE (Walk-Forward Toxic Ban & Re-Entry Cooldown)
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct AntiChurnQuarantineGate {
    /// Maps identifier (token_mint or symbol) -> list of (exit_epoch, is_toxic)
    pub settled_records: HashMap<String, Vec<(u64, bool)>>,
}

impl AntiChurnQuarantineGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates if candidate token is clear of past toxic bans and re-entry cooldowns
    pub fn validate_pre_entry(&self, ctx: &PreEntryContext, cfg: &RiskConfig) -> Result<(), &'static str> {
        self.check_identifier(&ctx.token_mint, ctx.entry_epoch, cfg, "QUARANTINE_PERMANENT_TOXIC_BAN_MINT", "QUARANTINE_REENTRY_COOLDOWN_MINT")?;
        self.check_identifier(&ctx.symbol, ctx.entry_epoch, cfg, "QUARANTINE_PERMANENT_TOXIC_BAN_SYMBOL", "QUARANTINE_REENTRY_COOLDOWN_SYMBOL")?;
        Ok(())
    }

    fn check_identifier(
        &self,
        id: &str,
        current_entry_epoch: u64,
        cfg: &RiskConfig,
        toxic_err: &'static str,
        cooldown_err: &'static str,
    ) -> Result<(), &'static str> {
        if let Some(records) = self.settled_records.get(id) {
            for &(exit_epoch, is_toxic) in records {
                // Only consider trades that actually settled strictly before or at current entry
                if exit_epoch <= current_entry_epoch {
                    if is_toxic && cfg.toxic_exit_permanent_ban {
                        return Err(toxic_err);
                    }
                    if current_entry_epoch < exit_epoch.saturating_add(cfg.reentry_cooldown_secs) {
                        return Err(cooldown_err);
                    }
                }
            }
        }
        Ok(())
    }

    /// Record settled trade once it has completed (exit_epoch <= current.entry_epoch of future trades)
    pub fn record_settled_trade(&mut self, ctx: &PreEntryContext, exit_epoch: u64, is_toxic: bool) {
        self.settled_records
            .entry(ctx.token_mint.clone())
            .or_default()
            .push((exit_epoch, is_toxic));
        self.settled_records
            .entry(ctx.symbol.clone())
            .or_default()
            .push((exit_epoch, is_toxic));
    }
}

// ============================================================================
// GATE 2: AMM ELASTICITY & LIQUIDITY GATE
// ============================================================================

#[derive(Debug, Clone)]
pub struct AmmElasticityAndLiquidityGate;

impl AmmElasticityAndLiquidityGate {
    pub const SOL_PRICE_USD: f64 = 180.0;

    pub fn compute_one_sided_sol_reserve(liquidity_usd: f64) -> f64 {
        if liquidity_usd > 0.0 {
            (liquidity_usd / 2.0) / Self::SOL_PRICE_USD
        } else {
            0.0
        }
    }

    pub fn compute_amm_price_impact_pct(size_sol: f64, pool_sol_reserve: f64) -> f64 {
        if pool_sol_reserve <= 0.0 {
            return 100.0;
        }
        (2.0 * size_sol / (pool_sol_reserve + size_sol)) * 100.0
    }

    pub fn compute_bonding_curve_saturation_pct(fdv_usd: f64, sandbox_cfg: &SandboxConfig) -> f64 {
        if sandbox_cfg.graduation_target_fdv_usd > 0.0 {
            (fdv_usd / sandbox_cfg.graduation_target_fdv_usd) * 100.0
        } else {
            100.0
        }
    }

    pub fn validate_pre_entry(
        &self,
        ctx: &PreEntryContext,
        risk_cfg: &RiskConfig,
        sandbox_cfg: &SandboxConfig,
    ) -> Result<(), &'static str> {
        // Flag rows with zero liquidity as unverified pre-gate entries
        if ctx.liquidity_usd <= 0.0 {
            return Err("UNVERIFIED_PREGATE_ZERO_LIQUIDITY");
        }

        // 1. Minimum liquidity floor ($20,000)
        if ctx.liquidity_usd < risk_cfg.min_entry_liquidity_usd {
            return Err("LIQUIDITY_BELOW_MIN");
        }

        // 2. FDV to Liquidity ratio (max 15.0x)
        let fdv_ratio = ctx.fdv_usd / ctx.liquidity_usd;
        if fdv_ratio > risk_cfg.max_entry_fdv_to_liquidity_ratio {
            return Err("FDV_TO_LIQUIDITY_RATIO_EXCEEDED");
        }

        // 3. Bonding-Curve Danger Zone: [88%, 102%] of $69,000 graduation target
        let sat_pct = Self::compute_bonding_curve_saturation_pct(ctx.fdv_usd, sandbox_cfg);
        if sat_pct >= sandbox_cfg.graduation_danger_zone_min_pct
            && sat_pct <= sandbox_cfg.graduation_danger_zone_max_pct
        {
            return Err("BONDING_CURVE_DANGER_ZONE");
        }

        // 4. Constant-Product AMM roundtrip price impact
        let pool_sol = Self::compute_one_sided_sol_reserve(ctx.liquidity_usd);
        let impact = Self::compute_amm_price_impact_pct(ctx.size_sol, pool_sol);
        if impact > sandbox_cfg.max_amm_price_impact_pct {
            return Err("AMM_PRICE_IMPACT_EXCEEDED");
        }

        Ok(())
    }
}

// ============================================================================
// GATE 3: SYBIL & ENTROPY GATE (Character Shannon Entropy, Supply & CU Overhead)
// ============================================================================

#[derive(Debug, Clone)]
pub struct SybilAndEntropyGate;

impl SybilAndEntropyGate {
    pub const BASELINE_SWAP_CU: f64 = 48_500.0;

    /// Symbol character Shannon entropy in bits: H(S) = -sum(p_c * log2(p_c))
    pub fn compute_symbol_shannon_entropy(symbol: &str) -> f64 {
        let clean = symbol.trim().to_uppercase();
        let len = clean.chars().count();
        if len == 0 {
            return 0.0;
        }
        let mut freq = HashMap::new();
        for c in clean.chars() {
            *freq.entry(c).or_insert(0usize) += 1;
        }
        let mut entropy = 0.0;
        for &count in freq.values() {
            let p = count as f64 / len as f64;
            if p > 0.0 {
                entropy -= p * p.log2();
            }
        }
        entropy
    }

    /// Computes fraction of supply held outside the constant-product liquidity pool
    pub fn compute_outside_supply_ratio(liquidity_usd: f64, fdv_usd: f64) -> f64 {
        if fdv_usd <= 0.0 {
            return 1.0;
        }
        let lp_pool_token_value = 0.5 * liquidity_usd;
        let lp_share = (lp_pool_token_value / fdv_usd).clamp(0.0, 1.0);
        1.0 - lp_share
    }

    pub fn validate_pre_entry(
        &self,
        ctx: &PreEntryContext,
        sandbox_cfg: &SandboxConfig,
    ) -> Result<(), &'static str> {
        // a) Symbol Character Shannon Entropy in bits (calibrated to 1.25 bits)
        let entropy = Self::compute_symbol_shannon_entropy(&ctx.symbol);
        if entropy < sandbox_cfg.min_holder_shannon_entropy {
            return Err("SYMBOL_ENTROPY_TOO_LOW");
        }

        // b) Outside supply concentration (if outside ratio > 96.6%, LP holds < 3.3% of tokens)
        if ctx.liquidity_usd > 0.0 && ctx.fdv_usd > 0.0 {
            let outside_ratio = Self::compute_outside_supply_ratio(ctx.liquidity_usd, ctx.fdv_usd);
            if outside_ratio > 0.966 {
                return Err("OUTSIDE_SUPPLY_CONCENTRATION_EXCEEDED");
            }
        }

        // c) Compute Unit Anomaly: honeypots, malicious hooks
        if ctx.consumed_cu > 0 {
            let cu_ratio = ctx.consumed_cu as f64 / Self::BASELINE_SWAP_CU;
            if cu_ratio > 1.40 {
                return Err("COMPUTE_UNIT_ANOMALY_HIGH");
            }
        }

        Ok(())
    }
}

// ============================================================================
// GATE 4: WALK-FORWARD EMPIRICAL BAYES COHORT GATE
// ============================================================================

#[derive(Debug, Clone)]
pub struct WalkForwardBayesCohortGate {
    pub prior_alpha: f64,
    pub prior_beta: f64,
    pub min_sample_size: u32,
    pub cohort_registry: HashMap<String, (u32, u32)>,
    pub quarantine: AntiChurnQuarantineGate,
}

impl WalkForwardBayesCohortGate {
    pub const DEFAULT_MIN_SAMPLE_SIZE: u32 = 6;
    pub const DEFAULT_PRIOR_ALPHA: f64 = 3.0;
    pub const DEFAULT_PRIOR_BETA: f64 = 3.0;

    pub fn new(prior_alpha: f64, prior_beta: f64, min_sample_size: u32) -> Self {
        Self {
            prior_alpha,
            prior_beta,
            min_sample_size,
            cohort_registry: HashMap::new(),
            quarantine: AntiChurnQuarantineGate::new(),
        }
    }

    pub fn from_config(_cfg: &SandboxConfig) -> Self {
        Self::new(
            Self::DEFAULT_PRIOR_ALPHA,
            Self::DEFAULT_PRIOR_BETA,
            Self::DEFAULT_MIN_SAMPLE_SIZE,
        )
    }

    pub fn derive_cohort_key(ctx: &PreEntryContext) -> String {
        let liq_tier = if ctx.liquidity_usd < 25_000.0 {
            "LOW_LIQ"
        } else if ctx.liquidity_usd < 50_000.0 {
            "MID_LIQ"
        } else {
            "HIGH_LIQ"
        };

        let fdv_ratio = if ctx.liquidity_usd > 0.0 {
            ctx.fdv_usd / ctx.liquidity_usd
        } else {
            20.0
        };

        let fdv_tier = if fdv_ratio < 4.0 {
            "SAFE_RATIO"
        } else if fdv_ratio < 10.0 {
            "MOD_RATIO"
        } else {
            "EXT_RATIO"
        };

        let bonding_stage = if ctx.fdv_usd < 69_000.0 {
            "BONDING"
        } else {
            "GRADUATED"
        };

        format!("{}_{}_{}", liq_tier, fdv_tier, bonding_stage)
    }

    /// Posterior mean p_hat = (W + alpha) / (N + alpha + beta)
    pub fn cohort_score(&self, cohort_key: &str) -> f64 {
        let (wins, total) = self.cohort_registry.get(cohort_key).copied().unwrap_or((0, 0));
        let num = wins as f64 + self.prior_alpha;
        let den = total as f64 + self.prior_alpha + self.prior_beta;
        num / den
    }

    /// Upper confidence bound: p_hat + sqrt(p_hat * (1 - p_hat) / N)
    pub fn cohort_ucb(&self, cohort_key: &str) -> f64 {
        let (_wins, total) = self.cohort_registry.get(cohort_key).copied().unwrap_or((0, 0));
        let p_hat = self.cohort_score(cohort_key);
        if total == 0 {
            return p_hat;
        }
        let n = total as f64;
        let variance = (p_hat * (1.0 - p_hat)).max(0.0) / n;
        p_hat + variance.sqrt()
    }

    pub fn validate_pre_entry(
        &self,
        ctx: &PreEntryContext,
        risk_cfg: &RiskConfig,
        sandbox_cfg: &SandboxConfig,
    ) -> Result<f64, &'static str> {
        // 1. Anti-Churn Quarantine check
        self.quarantine.validate_pre_entry(ctx, risk_cfg)?;

        // 2. Microstructure Cohort Walk-Forward Score with UCB & minimum sample size
        let key = Self::derive_cohort_key(ctx);
        let (_wins, total) = self.cohort_registry.get(&key).copied().unwrap_or((0, 0));
        let p_hat = self.cohort_score(&key);

        // Only block if sample size in cohort >= min_sample_size (6) and UCB < min_score (0.42)
        if total >= self.min_sample_size {
            let ucb = self.cohort_ucb(&key);
            if ucb < sandbox_cfg.bayes_min_wallet_score {
                return Err("EMPIRICAL_BAYES_SCORE_TOO_LOW");
            }
        }

        Ok(p_hat)
    }

    pub fn record_settled_trade(
        &mut self,
        ctx: &PreEntryContext,
        exit_epoch: u64,
        is_win: bool,
        is_toxic: bool,
    ) {
        self.quarantine.record_settled_trade(ctx, exit_epoch, is_toxic);

        let key = Self::derive_cohort_key(ctx);
        let entry = self.cohort_registry.entry(key).or_insert((0, 0));
        entry.1 += 1;
        if is_win {
            entry.0 += 1;
        }
    }
}

// ============================================================================
// HAWKES SELF-EXCITING POINT PROCESS (Pre-Entry Arrival Cascade)
// ============================================================================

#[derive(Debug, Clone)]
pub struct HawkesEstimator {
    pub mu: f64,
    pub alpha: f64,
    pub beta: f64,
}

impl HawkesEstimator {
    pub fn new(mu: f64, alpha: f64, beta: f64) -> Self {
        Self { mu, alpha, beta }
    }

    pub fn from_config(cfg: &SandboxConfig) -> Self {
        Self {
            mu: cfg.hawkes_mu_baseline,
            alpha: cfg.hawkes_alpha_jump,
            beta: cfg.hawkes_beta_decay,
        }
    }

    pub fn branching_ratio(&self) -> f64 {
        if self.beta > 0.0 {
            self.alpha / self.beta
        } else {
            0.0
        }
    }

    pub fn intensity(&self, current_time: f64, arrivals: &[f64]) -> f64 {
        let mut lambda = self.mu;
        for &t_i in arrivals {
            if t_i < current_time {
                let dt = current_time - t_i;
                lambda += self.alpha * (-self.beta * dt).exp();
            }
        }
        lambda
    }

    pub fn validate_pre_entry(&self, ctx: &PreEntryContext, cfg: &SandboxConfig) -> Result<f64, &'static str> {
        let n = self.branching_ratio();
        if n < cfg.hawkes_min_branching_ratio {
            return Err("HAWKES_BRANCHING_RATIO_SUB_CRITICAL");
        }
        if n > cfg.hawkes_max_branching_ratio {
            return Err("HAWKES_BRANCHING_RATIO_EXPLOSIVE");
        }
        let cur_t = ctx.entry_epoch as f64;
        let lambda_t = self.intensity(cur_t, &ctx.pre_entry_arrival_timestamps);
        if lambda_t <= self.mu {
            return Err("HAWKES_CASCADE_EXHAUSTED");
        }
        let excitation_ratio = (lambda_t - self.mu) / lambda_t;
        if excitation_ratio < 0.20 {
            return Err("HAWKES_EXCITATION_TOO_LOW");
        }
        Ok(lambda_t)
    }
}

// ============================================================================
// TOKEN-LEVEL ORDER-FLOW TOXICITY (VPIN)
// ============================================================================

#[derive(Debug, Clone)]
pub struct VpinCalculator {
    pub bucket_size_sol: f64,
}

impl VpinCalculator {
    pub fn new(bucket_size_sol: f64) -> Self {
        Self { bucket_size_sol }
    }

    pub fn from_config(cfg: &SandboxConfig) -> Self {
        Self {
            bucket_size_sol: cfg.vpin_bucket_size_sol,
        }
    }

    /// True token-level order-flow liquidity stress: trade size relative to pool reserve
    pub fn compute_token_toxicity(&self, ctx: &PreEntryContext) -> f64 {
        let pool_reserve_sol = if ctx.liquidity_usd > 0.0 {
            (ctx.liquidity_usd / 2.0) / 180.0
        } else {
            0.0
        };
        if pool_reserve_sol > 0.0 {
            (ctx.size_sol / pool_reserve_sol).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    pub fn validate_pre_entry(&self, ctx: &PreEntryContext, cfg: &SandboxConfig) -> Result<f64, &'static str> {
        let tox = self.compute_token_toxicity(ctx);
        if tox > cfg.vpin_max_entry_toxicity {
            return Err("VPIN_ENTRY_TOXICITY_EXCEEDED");
        }
        Ok(tox)
    }
}

// ============================================================================
// STEP 2: DETERMINISTIC v0.7.1 EXIT ENVELOPE COUNTERFACTUAL
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopLossExecutionMode {
    Ideal,
    RealisticGapSlippage,
}

/// Computes the exact gross PnL % and exit rule according to the deterministic v0.7.1 exit ladder:
/// - Case A: peak_pnl_pct >= tp2_trigger_pct (+90%) -> TP1 (40% @ +45%), TP2 (35% @ +90%), Runner (25% @ Peak - 18% rel drop)
/// - Case B: peak_pnl_pct >= tp1_trigger_pct (+45%) -> TP1 (40% @ +45%), Remaining (60% @ Peak - 18% rel drop)
/// - Case C: peak_pnl_pct >= breakeven_arm_pct (+35%) -> exits at max(actual_exit_pnl_pct, breakeven_floor_pct [+12%])
/// - Case D: peak_pnl_pct < early_abort_max_peak_pct (+1%) AND hold_secs >= early_abort_window_secs (45s) AND actual <= early_abort_loss_pct (-10%) -> early abort (-10%)
/// - Case E: actual_exit_pnl_pct <= stop_loss_pct (-20%) -> Ideal (-20%) or Realistic Gap-Slippage Stop-Loss
/// - Case F: Otherwise natural exit at actual_exit_pnl_pct
pub fn compute_v071_gross_pnl_pct(
    entry_price: f64,
    peak_price: f64,
    actual_exit_price: f64,
    hold_secs: u64,
    risk_cfg: &RiskConfig,
    sl_mode: StopLossExecutionMode,
) -> (f64, &'static str) {
    if entry_price <= 0.0 {
        return (0.0, "INVALID_ENTRY_PRICE");
    }

    let peak_pnl_pct = ((peak_price - entry_price) / entry_price) * 100.0;
    let actual_exit_pnl_pct = ((actual_exit_price - entry_price) / entry_price) * 100.0;

    // Case A: peak_pnl_pct >= tp2_trigger_pct (+90%)
    if peak_pnl_pct >= risk_cfg.tp2_trigger_pct {
        let runner_exit_mult = (1.0 + peak_pnl_pct / 100.0) * (1.0 - risk_cfg.runner_trailing_drop_pct / 100.0);
        let runner_pnl_pct = ((runner_exit_mult - 1.0) * 100.0).max(risk_cfg.breakeven_floor_pct);
        let runner_frac = 1.0 - risk_cfg.tp1_sell_fraction - risk_cfg.tp2_sell_fraction;
        let gross = (risk_cfg.tp1_sell_fraction * risk_cfg.tp1_trigger_pct)
            + (risk_cfg.tp2_sell_fraction * risk_cfg.tp2_trigger_pct)
            + (runner_frac * runner_pnl_pct);
        return (gross, "TP2_RUNNER_EXIT");
    }

    // Case B: peak_pnl_pct >= risk_cfg.tp1_trigger_pct (+45%)
    if peak_pnl_pct >= risk_cfg.tp1_trigger_pct {
        let runner_exit_mult = (1.0 + peak_pnl_pct / 100.0) * (1.0 - risk_cfg.runner_trailing_drop_pct / 100.0);
        let runner_pnl_pct = ((runner_exit_mult - 1.0) * 100.0).max(risk_cfg.breakeven_floor_pct);
        let runner_frac = 1.0 - risk_cfg.tp1_sell_fraction;
        let gross = (risk_cfg.tp1_sell_fraction * risk_cfg.tp1_trigger_pct)
            + (runner_frac * runner_pnl_pct);
        return (gross, "TP1_TRAILING_EXIT");
    }

    // Case C: peak_pnl_pct >= risk_cfg.breakeven_arm_pct (+35%)
    if peak_pnl_pct >= risk_cfg.breakeven_arm_pct {
        let gross = actual_exit_pnl_pct.max(risk_cfg.breakeven_floor_pct);
        return (gross, "BREAKEVEN_GUARD_EXIT");
    }

    // Case D: Early Abort
    if peak_pnl_pct < risk_cfg.early_abort_max_peak_pct
        && hold_secs >= risk_cfg.early_abort_window_secs
        && actual_exit_pnl_pct <= risk_cfg.early_abort_loss_pct
    {
        return (risk_cfg.early_abort_loss_pct, "EARLY_ABORT_EXIT");
    }

    // Case E: Hard Stop-Loss (Ideal vs Realistic Gap-Slippage Mode)
    if actual_exit_pnl_pct <= risk_cfg.stop_loss_pct {
        let gross = match sl_mode {
            StopLossExecutionMode::Ideal => risk_cfg.stop_loss_pct,
            StopLossExecutionMode::RealisticGapSlippage => {
                (risk_cfg.stop_loss_pct + 0.35 * (actual_exit_pnl_pct - risk_cfg.stop_loss_pct))
                    .max(risk_cfg.max_slippage_loss_pct * 1.35)
            }
        };
        return (gross, "HARD_STOP_LOSS_EXIT");
    }

    // Case F: Natural Exit
    (actual_exit_pnl_pct, "NATURAL_EXIT")
}

pub fn simulate_v071_exit(
    entry_price: f64,
    peak_price: f64,
    actual_exit_price: f64,
    hold_secs: u64,
    size_sol: f64,
    risk_cfg: &RiskConfig,
    sl_mode: StopLossExecutionMode,
) -> (f64, f64, String) {
    let (gross_pnl_pct, reason) = compute_v071_gross_pnl_pct(
        entry_price,
        peak_price,
        actual_exit_price,
        hold_secs,
        risk_cfg,
        sl_mode,
    );
    let tip = resolve_exit_tip(reason, risk_cfg);
    let net_sol = compute_net_realized_sol_with_tip(size_sol, gross_pnl_pct, risk_cfg, tip);
    let net_pnl_pct = compute_net_pnl_pct(size_sol, net_sol);
    (net_pnl_pct, net_sol, reason.to_string())
}

/// Dynamic Conviction-Tiered Kelly Sizing evaluator for Sandbox simulation contexts
pub fn compute_sandbox_dynamic_size(
    ctx: &PreEntryContext,
    bankroll_sol: f64,
    active_slots_used: usize,
    risk_cfg: &RiskConfig,
) -> f64 {
    let buy_sell_ratio = if ctx.liquidity_usd > 0.0 {
        (ctx.fdv_usd / ctx.liquidity_usd).clamp(1.0, 5.0)
    } else {
        1.5
    };
    let tier = evaluate_conviction_tier(
        buy_sell_ratio,
        ctx.bonding_curve_pct,
        (ctx.pre_entry_total_observations as u64).max(20),
        ctx.liquidity_usd * 0.10,
        risk_cfg,
    );
    compute_dynamic_entry_size_sol(bankroll_sol, tier, active_slots_used, risk_cfg)
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ctx() -> PreEntryContext {
        PreEntryContext {
            token_mint: "TestMint11111111111111111111111111111111111".to_string(),
            symbol: "ALPHA".to_string(),
            entry_epoch: 1793980100,
            entry_price_sol: 0.000005,
            size_sol: 0.02,
            liquidity_usd: 35000.0,
            fdv_usd: 50000.0,
            bonding_curve_pct: 72.0,
            consumed_cu: 52000,
            jito_tip_lamports: 10000,
            pre_entry_arrival_timestamps: vec![1793980060.0, 1793980080.0, 1793980090.0, 1793980098.0],
            pre_entry_recent_rejections: 10,
            pre_entry_total_observations: 100,
        }
    }

    #[test]
    fn test_anti_churn_quarantine_gate() {
        let risk_cfg = RiskConfig::default();
        let mut gate = AntiChurnQuarantineGate::new();
        let ctx = sample_ctx();

        // Initially no history -> passes
        assert!(gate.validate_pre_entry(&ctx, &risk_cfg).is_ok());

        // Record a toxic exit that settled at t = 1793980000 (before current entry 1793980100)
        gate.record_settled_trade(&ctx, 1793980000, true);
        assert_eq!(gate.validate_pre_entry(&ctx, &risk_cfg), Err("QUARANTINE_PERMANENT_TOXIC_BAN_MINT"));

        // Test non-toxic exit with cooldown
        let mut non_toxic_gate = AntiChurnQuarantineGate::new();
        non_toxic_gate.record_settled_trade(&ctx, 1793980000, false);
        // Current entry is 100s later, cooldown is 3600s -> rejects
        assert_eq!(non_toxic_gate.validate_pre_entry(&ctx, &risk_cfg), Err("QUARANTINE_REENTRY_COOLDOWN_MINT"));

        // If current entry is 4000s later -> passes cooldown
        let mut future_ctx = ctx.clone();
        future_ctx.entry_epoch = 1793980000 + 4000;
        assert!(non_toxic_gate.validate_pre_entry(&future_ctx, &risk_cfg).is_ok());
    }

    #[test]
    fn test_amm_elasticity_and_liquidity_gate() {
        let risk_cfg = RiskConfig::default();
        let sandbox_cfg = SandboxConfig::default();
        let gate = AmmElasticityAndLiquidityGate;
        let ctx = sample_ctx();

        assert!(gate.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg).is_ok());

        // Zero liquidity -> unverified pre-gate error
        let mut zero_liq = ctx.clone();
        zero_liq.liquidity_usd = 0.0;
        assert_eq!(gate.validate_pre_entry(&zero_liq, &risk_cfg, &sandbox_cfg), Err("UNVERIFIED_PREGATE_ZERO_LIQUIDITY"));

        // Liquidity below min ($15,000 < $20,000)
        let mut low_liq = ctx.clone();
        low_liq.liquidity_usd = 15000.0;
        assert_eq!(gate.validate_pre_entry(&low_liq, &risk_cfg, &sandbox_cfg), Err("LIQUIDITY_BELOW_MIN"));

        // FDV to Liquidity ratio exceeded ($400,000 / $22,000 = 18.2x > 15.0x)
        let mut high_fdv = ctx.clone();
        high_fdv.liquidity_usd = 22000.0;
        high_fdv.fdv_usd = 400000.0;
        assert_eq!(gate.validate_pre_entry(&high_fdv, &risk_cfg, &sandbox_cfg), Err("FDV_TO_LIQUIDITY_RATIO_EXCEEDED"));

        // Danger zone: FDV $64,000 on $69k target = 92.7%
        let mut danger_ctx = ctx.clone();
        danger_ctx.fdv_usd = 64_000.0;
        assert_eq!(gate.validate_pre_entry(&danger_ctx, &risk_cfg, &sandbox_cfg), Err("BONDING_CURVE_DANGER_ZONE"));
    }

    #[test]
    fn test_sybil_and_entropy_gate() {
        let sandbox_cfg = SandboxConfig::default();
        let gate = SybilAndEntropyGate;
        let ctx = sample_ctx();

        assert!(gate.validate_pre_entry(&ctx, &sandbox_cfg).is_ok());

        // Low entropy ticker: "7" (0.0 bits < 1.25)
        let mut low_ent = ctx.clone();
        low_ent.symbol = "7".to_string();
        assert_eq!(gate.validate_pre_entry(&low_ent, &sandbox_cfg), Err("SYMBOL_ENTROPY_TOO_LOW"));

        // High entropy 4-character ticker: "DOGE" (2.0 bits >= 1.25)
        let mut doge = ctx.clone();
        doge.symbol = "DOGE".to_string();
        assert!(gate.validate_pre_entry(&doge, &sandbox_cfg).is_ok());

        // Extreme outside supply concentration (FDV $800k on $15k liq)
        let mut extreme_outside = ctx.clone();
        extreme_outside.fdv_usd = 800_000.0;
        extreme_outside.liquidity_usd = 15_000.0;
        assert_eq!(gate.validate_pre_entry(&extreme_outside, &sandbox_cfg), Err("OUTSIDE_SUPPLY_CONCENTRATION_EXCEEDED"));

        // Compute Unit Anomaly: honeypot callback (120,000 CU)
        let mut cu_anom = ctx.clone();
        cu_anom.consumed_cu = 120_000;
        assert_eq!(gate.validate_pre_entry(&cu_anom, &sandbox_cfg), Err("COMPUTE_UNIT_ANOMALY_HIGH"));
    }

    #[test]
    fn test_walk_forward_bayes_cohort_gate() {
        let risk_cfg = RiskConfig::default();
        let sandbox_cfg = SandboxConfig::default();
        let mut gate = WalkForwardBayesCohortGate::from_config(&sandbox_cfg);
        let ctx = sample_ctx();

        // Baseline unobserved cohort (N=0 < 6): exploration phase -> passes!
        assert!(gate.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg).is_ok());

        // Record 8 consecutive losses from other tokens in the same cohort
        for i in 0..8 {
            let mut other_ctx = ctx.clone();
            other_ctx.token_mint = format!("OtherMint{}", i);
            other_ctx.symbol = format!("OTH{}", i);
            gate.record_settled_trade(&other_ctx, ctx.entry_epoch - 800 + i * 50, false, false);
        }
        // Total N=8, Wins=0: score = (0 + 3) / (8 + 6) = 3/14 = 0.214
        // UCB = 0.214 + sqrt(0.214 * 0.786 / 8) = 0.214 + 0.145 = 0.359 < 0.42 -> BLOCKED!
        assert_eq!(gate.validate_pre_entry(&ctx, &risk_cfg, &sandbox_cfg), Err("EMPIRICAL_BAYES_SCORE_TOO_LOW"));
    }

    #[test]
    fn test_simulate_v071_exit_all_cases() {
        let risk_cfg = RiskConfig::default();
        let entry_price = 1.0;
        let size_sol = 0.10;

        // Case A: peak +100% (>= 90%) -> TP2 Runner Exit
        let (net_pnl, _net_sol, reason) = simulate_v071_exit(entry_price, 2.0, 1.5, 300, size_sol, &risk_cfg, StopLossExecutionMode::Ideal);
        assert_eq!(reason, "TP2_RUNNER_EXIT");
        assert!(net_pnl > 60.0);

        // Case B: peak +60% (>= 45%, < 90%) -> TP1 Trailing Exit
        let (net_pnl, _net_sol, reason) = simulate_v071_exit(entry_price, 1.6, 1.2, 200, size_sol, &risk_cfg, StopLossExecutionMode::Ideal);
        assert_eq!(reason, "TP1_TRAILING_EXIT");
        assert!(net_pnl > 30.0);

        // Case C: peak +40% (>= 24%, < 45%), exit at 0.95 (-5%) -> Breakeven Guard (+8% floor)
        let (net_pnl, _net_sol, reason) = simulate_v071_exit(entry_price, 1.4, 0.95, 150, size_sol, &risk_cfg, StopLossExecutionMode::Ideal);
        assert_eq!(reason, "BREAKEVEN_GUARD_EXIT");
        assert!(net_pnl > 5.0 && net_pnl <= risk_cfg.breakeven_floor_pct);

        // Case D: peak +0.5% (< 1%), hold 50s (>= 45s), exit at 0.85 (-15% <= -10%) -> Early Abort (-10%)
        let (net_pnl, _net_sol, reason) = simulate_v071_exit(entry_price, 1.005, 0.85, 50, size_sol, &risk_cfg, StopLossExecutionMode::Ideal);
        assert_eq!(reason, "EARLY_ABORT_EXIT");
        assert!(net_pnl <= -10.0 && net_pnl > -15.0);

        // Case E (Ideal SL): exit at 0.50 (-50% <= -20%) -> Hard Stop Loss (-20%)
        let (net_pnl_ideal, _net_sol, reason) = simulate_v071_exit(entry_price, 1.10, 0.50, 100, size_sol, &risk_cfg, StopLossExecutionMode::Ideal);
        assert_eq!(reason, "HARD_STOP_LOSS_EXIT");
        assert!(net_pnl_ideal <= -20.0 && net_pnl_ideal > -25.0);

        // Case E (Realistic Gap-Slippage SL): exit at 0.50 (-50%) -> Gap exit around -30.5% gross (-31.2% net)
        let (net_pnl_gap, _net_sol, reason) = simulate_v071_exit(entry_price, 1.10, 0.50, 100, size_sol, &risk_cfg, StopLossExecutionMode::RealisticGapSlippage);
        assert_eq!(reason, "HARD_STOP_LOSS_EXIT");
        assert!(net_pnl_gap < -25.0 && net_pnl_gap >= -35.0);

        // Case F: peak +15%, exit at 1.08 (+8%) -> Natural Exit
        let (net_pnl, _net_sol, reason) = simulate_v071_exit(entry_price, 1.15, 1.08, 100, size_sol, &risk_cfg, StopLossExecutionMode::Ideal);
        assert_eq!(reason, "NATURAL_EXIT");
        assert!(net_pnl > 6.0 && net_pnl < 8.0);
    }

    #[test]
    fn test_sandbox_conviction_tiered_sizing() {
        let risk_cfg = RiskConfig::default();
        let mut ctx = sample_ctx();
        let bankroll_sol = 0.50;

        // Baseline sizing
        let base_size = compute_sandbox_dynamic_size(&ctx, bankroll_sol, 0, &risk_cfg);
        assert!(base_size >= risk_cfg.min_entry_sol && base_size <= risk_cfg.max_entry_sol);

        // High conviction context (FDV / liq >= 2.0 and high bonding curve)
        ctx.bonding_curve_pct = 90.0;
        ctx.fdv_usd = 80_000.0;
        ctx.liquidity_usd = 35_000.0;
        let high_size = compute_sandbox_dynamic_size(&ctx, bankroll_sol, 0, &risk_cfg);
        assert!(high_size >= base_size);
        assert!((high_size - 0.0425).abs() < 1e-4);

        // Solvency & headroom bounds
        let full_headroom = compute_sandbox_dynamic_size(&ctx, bankroll_sol, 4, &risk_cfg);
        assert_eq!(full_headroom, 0.0);
    }
}
