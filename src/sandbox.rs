#![allow(dead_code)]
// DMNE QUANT // ADVANCED MICROSTRUCTURE SANDBOX MODULE
// Strict Rust Type-System Firewall: PreEntryContext Observable strictly at t <= entry_epoch
// 5 Institutional Microstructure Methods (Zero Target Leakage / Zero Lookahead Bias):
// 1. Hawkes Self-Exciting Point-Process Intensity & Excitation Ratio
// 2. Sybil & Non-LP Supply Concentration, Symbol Entropy & CU Overhead
// 3. Volume-Synchronized Probability of Toxicity (VPIN) & Market Rejection Pressure
// 4. Empirical Bayes Walk-Forward Microstructure Cohort Estimator & Anti-Reentry Guard
// 5. Bonding-Curve Saturation Danger Zone & Exact AMM Reserve Elasticity

use std::collections::HashMap;
use crate::config::SandboxConfig;

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
    // Real pre-entry audit & market metrics at t <= entry_epoch
    pub pre_entry_arrival_timestamps: Vec<f64>,
    pub pre_entry_recent_rejections: usize,
    pub pre_entry_total_observations: usize,
}

// ============================================================================
// METHOD 1: HAWKES SELF-EXCITING POINT PROCESS (Pre-Entry Arrival Velocity)
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

    /// Branching ratio n = alpha / beta
    pub fn branching_ratio(&self) -> f64 {
        if self.beta > 0.0 {
            self.alpha / self.beta
        } else {
            0.0
        }
    }

    /// Intensity lambda(t) = mu + sum_{t_i < t} alpha * exp(-beta * (t - t_i))
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

    /// Pre-entry gate: validates whether the arrival cascade is active and self-exciting
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
// METHOD 2: SYBIL & BUNDLE FORENSICS (Supply Concentration & Contract Overhead)
// ============================================================================

#[derive(Debug, Clone)]
pub struct SybilBundleAuditor;

impl SybilBundleAuditor {
    pub const BASELINE_SWAP_CU: f64 = 48_500.0;

    /// Symbol character Shannon entropy: H(S) = -sum(p_c * ln(p_c))
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
            entropy -= p * p.ln();
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

    /// Pre-entry gate: checks outside supply concentration, ticker entropy, and CU overhead
    pub fn validate_pre_entry(&self, ctx: &PreEntryContext, _cfg: &SandboxConfig) -> Result<(), &'static str> {
        // a) Non-LP supply concentration (if outside ratio > 96.6%, LP holds < 3.3% of tokens)
        let outside_ratio = Self::compute_outside_supply_ratio(ctx.liquidity_usd, ctx.fdv_usd);
        if outside_ratio > 0.966 {
            return Err("OUTSIDE_SUPPLY_CONCENTRATION_EXCEEDED");
        }

        // b) Symbol Character Shannon Entropy: filters repetitive spam tickers
        let entropy = Self::compute_symbol_shannon_entropy(&ctx.symbol);
        if entropy < 0.60 {
            return Err("SYMBOL_ENTROPY_TOO_LOW");
        }

        // c) Compute Unit Anomaly Ratio: filters malicious hook callbacks and honeypots
        let cu_ratio = ctx.consumed_cu as f64 / Self::BASELINE_SWAP_CU;
        if cu_ratio > 1.40 {
            return Err("COMPUTE_UNIT_ANOMALY_HIGH");
        }

        Ok(())
    }
}

// ============================================================================
// METHOD 3: VOLUME-SYNCHRONIZED PROBABILITY OF TOXICITY (VPIN)
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

    /// Pre-entry VPIN: market rejection toxicity + token-specific liquidity stress
    pub fn compute_pre_entry_vpin(&self, ctx: &PreEntryContext) -> f64 {
        // Market rejection toxicity in [entry_epoch - 300, entry_epoch]
        let mkt_toxicity = if ctx.pre_entry_total_observations > 0 {
            (ctx.pre_entry_recent_rejections as f64 / ctx.pre_entry_total_observations as f64).clamp(0.0, 1.0)
        } else {
            0.50
        };

        // Token liquidity stress: ratio of FDV to (Liquidity * 15.0)
        let token_stress = if ctx.liquidity_usd > 0.0 {
            (ctx.fdv_usd / (ctx.liquidity_usd * 15.0)).clamp(0.0, 1.0)
        } else {
            1.0
        };

        // Blended VPIN metric
        0.50 * mkt_toxicity + 0.50 * token_stress
    }

    pub fn validate_pre_entry(&self, ctx: &PreEntryContext, cfg: &SandboxConfig) -> Result<f64, &'static str> {
        let vpin = self.compute_pre_entry_vpin(ctx);
        if vpin > cfg.vpin_max_entry_toxicity {
            return Err("VPIN_ENTRY_TOXICITY_EXCEEDED");
        }
        Ok(vpin)
    }
}

// ============================================================================
// METHOD 4: EMPIRICAL BAYES WALK-FORWARD MICROSTRUCTURE COHORT MATRIX
// ============================================================================

#[derive(Debug, Clone)]
pub struct EmpiricalBayesMatrix {
    pub prior_alpha: f64,
    pub prior_beta: f64,
    pub cohort_registry: HashMap<String, (u32, u32)>,
    pub settled_token_history: HashMap<String, u64>,
}

impl EmpiricalBayesMatrix {
    pub fn new(prior_alpha: f64, prior_beta: f64) -> Self {
        Self {
            prior_alpha,
            prior_beta,
            cohort_registry: HashMap::new(),
            settled_token_history: HashMap::new(),
        }
    }

    pub fn from_config(cfg: &SandboxConfig) -> Self {
        Self {
            prior_alpha: cfg.bayes_prior_alpha,
            prior_beta: cfg.bayes_prior_beta,
            cohort_registry: HashMap::new(),
            settled_token_history: HashMap::new(),
        }
    }

    /// Derive observable Pre-Entry Microstructure Cohort Key from PreEntryContext
    pub fn derive_cohort_key(ctx: &PreEntryContext) -> String {
        let liq_tier = if ctx.liquidity_usd < 25_000.0 {
            "LOW_LIQ"
        } else if ctx.liquidity_usd < 50_000.0 {
            "MID_LIQ"
        } else {
            "HIGH_LIQ"
        };

        let fdv_ratio = if ctx.liquidity_usd > 0.0 { ctx.fdv_usd / ctx.liquidity_usd } else { 20.0 };
        let fdv_tier = if fdv_ratio < 4.0 {
            "SAFE_RATIO"
        } else if fdv_ratio < 10.0 {
            "MOD_RATIO"
        } else {
            "EXT_RATIO"
        };

        let is_bonding = if ctx.fdv_usd < 69_000.0 { "BONDING" } else { "GRADUATED" };
        format!("{}_{}_{}", liq_tier, fdv_tier, is_bonding)
    }

    /// Beta-Binomial posterior expectation: (W + alpha) / (N + alpha + beta)
    pub fn cohort_score(&self, cohort_key: &str) -> f64 {
        let (wins, total) = self.cohort_registry.get(cohort_key).copied().unwrap_or((0, 0));
        let num = wins as f64 + self.prior_alpha;
        let den = total as f64 + self.prior_alpha + self.prior_beta;
        num / den
    }

    pub fn validate_pre_entry(&self, ctx: &PreEntryContext, cfg: &SandboxConfig) -> Result<f64, &'static str> {
        // a) Anti-Reentry Guard: reject re-entries on tokens that already traded in walk-forward history
        if let Some(&last_exit) = self.settled_token_history.get(&ctx.token_mint) {
            if last_exit <= ctx.entry_epoch {
                return Err("ANTI_REENTRY_CHURN_MINT");
            }
        }
        if let Some(&last_exit) = self.settled_token_history.get(&ctx.symbol) {
            if last_exit <= ctx.entry_epoch {
                return Err("ANTI_REENTRY_CHURN_SYMBOL");
            }
        }

        // b) Microstructure Cohort Walk-Forward Score
        let cohort_key = Self::derive_cohort_key(ctx);
        let score = self.cohort_score(&cohort_key);
        if score < cfg.bayes_min_wallet_score {
            return Err("EMPIRICAL_BAYES_SCORE_TOO_LOW");
        }
        Ok(score)
    }

    /// Strictly walk-forward update: record outcome ONLY after trade exit (t_exit <= t_entry of future trades)
    pub fn record_settled_trade(&mut self, ctx: &PreEntryContext, exit_epoch: u64, is_win: bool) {
        self.settled_token_history.insert(ctx.token_mint.clone(), exit_epoch);
        self.settled_token_history.insert(ctx.symbol.clone(), exit_epoch);

        let cohort_key = Self::derive_cohort_key(ctx);
        let entry = self.cohort_registry.entry(cohort_key).or_insert((0, 0));
        entry.1 += 1;
        if is_win {
            entry.0 += 1;
        }
    }
}

// ============================================================================
// METHOD 5: BONDING-CURVE SATURATION & AMM ELASTICITY
// ============================================================================

#[derive(Debug, Clone)]
pub struct CurveElasticityModel;

impl CurveElasticityModel {
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

    pub fn compute_bonding_curve_saturation_pct(fdv_usd: f64, cfg: &SandboxConfig) -> f64 {
        if cfg.graduation_target_fdv_usd > 0.0 {
            (fdv_usd / cfg.graduation_target_fdv_usd) * 100.0
        } else {
            100.0
        }
    }

    pub fn validate_pre_entry(&self, ctx: &PreEntryContext, cfg: &SandboxConfig) -> Result<(), &'static str> {
        // 1. Minimum liquidity floor
        if ctx.liquidity_usd < 20_000.0 {
            return Err("LIQUIDITY_BELOW_MIN");
        }

        // 2. Constant-Product AMM price impact
        let pool_sol = Self::compute_one_sided_sol_reserve(ctx.liquidity_usd);
        let impact = Self::compute_amm_price_impact_pct(ctx.size_sol, pool_sol);
        if impact > cfg.max_amm_price_impact_pct {
            return Err("AMM_PRICE_IMPACT_EXCEEDED");
        }

        // 3. Bonding-Curve Danger Zone: [88%, 102%] of $69,000 graduation target
        let sat_pct = Self::compute_bonding_curve_saturation_pct(ctx.fdv_usd, cfg);
        if sat_pct >= cfg.graduation_danger_zone_min_pct && sat_pct <= cfg.graduation_danger_zone_max_pct {
            return Err("BONDING_CURVE_DANGER_ZONE");
        }

        Ok(())
    }
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
            pre_entry_recent_rejections: 95,
            pre_entry_total_observations: 100,
        }
    }

    #[test]
    fn test_hawkes_pre_entry_gate() {
        let cfg = SandboxConfig::default();
        let estimator = HawkesEstimator::from_config(&cfg);
        let ctx = sample_ctx();

        let res = estimator.validate_pre_entry(&ctx, &cfg);
        assert!(res.is_ok());

        // Stale / empty arrivals -> exhausted cascade
        let mut stale_ctx = ctx.clone();
        stale_ctx.pre_entry_arrival_timestamps.clear();
        assert_eq!(estimator.validate_pre_entry(&stale_ctx, &cfg), Err("HAWKES_CASCADE_EXHAUSTED"));
    }

    #[test]
    fn test_sybil_and_bundle_auditor() {
        let cfg = SandboxConfig::default();
        let auditor = SybilBundleAuditor;
        let ctx = sample_ctx();

        assert!(auditor.validate_pre_entry(&ctx, &cfg).is_ok());

        // Low entropy ticker: "7" or "XXXXXX"
        let mut low_ent_ctx = ctx.clone();
        low_ent_ctx.symbol = "7".to_string();
        assert_eq!(auditor.validate_pre_entry(&low_ent_ctx, &cfg), Err("SYMBOL_ENTROPY_TOO_LOW"));

        // Extreme outside supply concentration (FDV $800k on $15k liq)
        let mut extreme_outside_ctx = ctx.clone();
        extreme_outside_ctx.fdv_usd = 800_000.0;
        extreme_outside_ctx.liquidity_usd = 15_000.0;
        assert_eq!(auditor.validate_pre_entry(&extreme_outside_ctx, &cfg), Err("OUTSIDE_SUPPLY_CONCENTRATION_EXCEEDED"));

        // CU Anomaly: honeypot callback
        let mut cu_anom_ctx = ctx.clone();
        cu_anom_ctx.consumed_cu = 120_000;
        assert_eq!(auditor.validate_pre_entry(&cu_anom_ctx, &cfg), Err("COMPUTE_UNIT_ANOMALY_HIGH"));
    }

    #[test]
    fn test_vpin_pre_entry_calculator() {
        let cfg = SandboxConfig::default();
        let calc = VpinCalculator::from_config(&cfg);
        let ctx = sample_ctx();

        // Safe balanced market
        let mut safe_ctx = ctx.clone();
        safe_ctx.pre_entry_recent_rejections = 30;
        safe_ctx.pre_entry_total_observations = 100;
        safe_ctx.liquidity_usd = 40000.0;
        safe_ctx.fdv_usd = 50000.0;
        let vpin = calc.compute_pre_entry_vpin(&safe_ctx);
        assert!(vpin <= cfg.vpin_max_entry_toxicity);
        assert!(calc.validate_pre_entry(&safe_ctx, &cfg).is_ok());

        // Toxic environment: 100% rejection rate and high FDV stress
        let mut toxic_ctx = ctx.clone();
        toxic_ctx.pre_entry_recent_rejections = 100;
        toxic_ctx.pre_entry_total_observations = 100;
        toxic_ctx.liquidity_usd = 10000.0;
        toxic_ctx.fdv_usd = 140000.0;
        assert_eq!(calc.validate_pre_entry(&toxic_ctx, &cfg), Err("VPIN_ENTRY_TOXICITY_EXCEEDED"));
    }

    #[test]
    fn test_empirical_bayes_walk_forward() {
        let cfg = SandboxConfig::default();
        let mut matrix = EmpiricalBayesMatrix::from_config(&cfg);
        let ctx = sample_ctx();

        // Baseline unobserved cohort: score = 2 / 5 = 0.40 (< 0.42)
        assert_eq!(matrix.validate_pre_entry(&ctx, &cfg), Err("EMPIRICAL_BAYES_SCORE_TOO_LOW"));

        // Record a win for this cohort from a previous settled trade
        matrix.record_settled_trade(&ctx, ctx.entry_epoch - 60, true);
        // Score becomes (1 + 2) / (1 + 5) = 3 / 6 = 0.50 (>= 0.42)
        assert!(matrix.cohort_score(&EmpiricalBayesMatrix::derive_cohort_key(&ctx)) >= cfg.bayes_min_wallet_score);

        // Anti-reentry guard: trying to trade the same token again after it settled
        let mut reentrant_ctx = ctx.clone();
        reentrant_ctx.entry_epoch = ctx.entry_epoch + 120;
        assert_eq!(matrix.validate_pre_entry(&reentrant_ctx, &cfg), Err("ANTI_REENTRY_CHURN_MINT"));
    }

    #[test]
    fn test_curve_elasticity_model() {
        let cfg = SandboxConfig::default();
        let model = CurveElasticityModel;
        let ctx = sample_ctx();

        assert!(model.validate_pre_entry(&ctx, &cfg).is_ok());

        // Danger zone: FDV $64,000 on $69k target = 92.7%
        let mut danger_ctx = ctx.clone();
        danger_ctx.fdv_usd = 64_000.0;
        assert_eq!(model.validate_pre_entry(&danger_ctx, &cfg), Err("BONDING_CURVE_DANGER_ZONE"));

        // Low liquidity: $12k < $20k
        let mut low_liq_ctx = ctx.clone();
        low_liq_ctx.liquidity_usd = 12_000.0;
        assert_eq!(model.validate_pre_entry(&low_liq_ctx, &cfg), Err("LIQUIDITY_BELOW_MIN"));

        // AMM Price impact exceeded: trade 2.5 SOL into small $22k pool ($11k reserve = 61 SOL) -> 8.2% impact
        let mut high_impact_ctx = ctx.clone();
        high_impact_ctx.liquidity_usd = 22_000.0;
        high_impact_ctx.size_sol = 2.5;
        assert_eq!(model.validate_pre_entry(&high_impact_ctx, &cfg), Err("AMM_PRICE_IMPACT_EXCEEDED"));
    }
}
