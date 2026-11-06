#![allow(dead_code)]
// DMNE QUANT // ADVANCED MICROSTRUCTURE SANDBOX MODULE
// 5 Institutional Microstructure Methods:
// 1. Hawkes Self-Exciting Cascade Intensity & Apex Exit
// 2. Sybil & Slot-0 Bundle Forensics & Shannon Entropy
// 3. Volume-Synchronized Probability of Toxicity (VPIN)
// 4. Empirical Bayes Walk-Forward Wallet Matrix
// 5. Bonding-Curve Saturation & AMM Reserve Elasticity

use crate::config::SandboxConfig;

// ============================================================================
// METHOD 1: HAWKES SELF-EXCITING POINT PROCESS
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
    pub fn intensity(&self, current_time: f64, arrival_times: &[f64]) -> f64 {
        let mut lambda = self.mu;
        for &t_i in arrival_times {
            if t_i < current_time {
                let dt = current_time - t_i;
                lambda += self.alpha * (-self.beta * dt).exp();
            }
        }
        lambda
    }

    /// Validates whether the arrival cascade is healthy and subcritical
    pub fn is_cascade_healthy(
        &self,
        arrival_times: &[f64],
        current_time: f64,
        cfg: &SandboxConfig,
    ) -> Result<f64, &'static str> {
        let n = self.branching_ratio();
        if n < cfg.hawkes_min_branching_ratio {
            return Err("HAWKES_BRANCHING_RATIO_SUB_CRITICAL");
        }
        if n > cfg.hawkes_max_branching_ratio {
            return Err("HAWKES_BRANCHING_RATIO_EXPLOSIVE");
        }
        let cur_intensity = self.intensity(current_time, arrival_times);
        if cur_intensity <= self.mu {
            return Err("HAWKES_CASCADE_EXHAUSTED");
        }
        Ok(cur_intensity)
    }

    /// Apex exhaustion detector: detects consecutive drops in cascade intensity
    pub fn is_apex_exhaustion(&self, intensity_history: &[f64]) -> bool {
        if intensity_history.len() < 3 {
            return false;
        }
        let len = intensity_history.len();
        let peak = intensity_history[len - 3];
        let mid = intensity_history[len - 2];
        let latest = intensity_history[len - 1];
        mid < peak && latest < mid
    }
}

// ============================================================================
// METHOD 2: SYBIL & SLOT-0 BUNDLE FORENSICS
// ============================================================================

#[derive(Debug, Clone)]
pub struct SybilBundleAuditor;

impl SybilBundleAuditor {
    /// Computes Shannon entropy of top holder balance distribution: H = -sum(p_i * ln(p_i))
    pub fn compute_shannon_entropy(holder_balances: &[f64]) -> f64 {
        let total: f64 = holder_balances.iter().filter(|&&b| b > 0.0).sum();
        if total <= 0.0 {
            return 0.0;
        }
        let mut entropy = 0.0;
        for &b in holder_balances {
            if b > 0.0 {
                let p = b / total;
                entropy -= p * p.ln();
            }
        }
        entropy
    }

    /// Validates slot-0 bundle supply ratio, holder entropy, and shared funder clustering
    pub fn validate_bundle_and_sybil(
        slot0_supply_ratio: f64,
        holder_balances: &[f64],
        shared_funder_cluster_ratio: f64,
        cfg: &SandboxConfig,
    ) -> Result<(), &'static str> {
        if slot0_supply_ratio > cfg.max_slot0_bundle_supply_ratio {
            return Err("SLOT0_BUNDLE_SUPPLY_EXCEEDED");
        }
        let entropy = Self::compute_shannon_entropy(holder_balances);
        if entropy < cfg.min_holder_shannon_entropy {
            return Err("HOLDER_SHANNON_ENTROPY_TOO_LOW");
        }
        if shared_funder_cluster_ratio > cfg.max_shared_funder_cluster_ratio {
            return Err("SHARED_FUNDER_CLUSTER_EXCEEDED");
        }
        Ok(())
    }
}

// ============================================================================
// METHOD 3: VOLUME-SYNCHRONIZED PROBABILITY OF TOXICITY (VPIN)
// ============================================================================

#[derive(Debug, Clone)]
pub struct TradeFlow {
    pub volume_sol: f64,
    pub is_buy: bool,
    pub timestamp_sec: u64,
}

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

    /// Computes VPIN across trades by slicing into fixed-volume buckets
    pub fn compute_vpin(&self, trades: &[TradeFlow]) -> f64 {
        if trades.is_empty() || self.bucket_size_sol <= 0.0 {
            return 0.0;
        }

        let mut bucket_imbalances = Vec::new();
        let mut cur_buy_vol = 0.0;
        let mut cur_sell_vol = 0.0;
        let mut cur_accum_vol = 0.0;

        for t in trades {
            let mut remaining_vol = t.volume_sol;
            while remaining_vol > 0.0 {
                let needed = self.bucket_size_sol - cur_accum_vol;
                if remaining_vol >= needed {
                    if t.is_buy {
                        cur_buy_vol += needed;
                    } else {
                        cur_sell_vol += needed;
                    }
                    let imbalance = (cur_buy_vol - cur_sell_vol).abs();
                    bucket_imbalances.push(imbalance);

                    remaining_vol -= needed;
                    cur_buy_vol = 0.0;
                    cur_sell_vol = 0.0;
                    cur_accum_vol = 0.0;
                } else {
                    if t.is_buy {
                        cur_buy_vol += remaining_vol;
                    } else {
                        cur_sell_vol += remaining_vol;
                    }
                    cur_accum_vol += remaining_vol;
                    remaining_vol = 0.0;
                }
            }
        }

        // Include partial tail bucket if non-empty
        if cur_accum_vol > 0.0 {
            let scaled_imbalance = (cur_buy_vol - cur_sell_vol).abs() * (self.bucket_size_sol / cur_accum_vol);
            bucket_imbalances.push(scaled_imbalance);
        }

        if bucket_imbalances.is_empty() {
            return 0.0;
        }

        let total_imbalance: f64 = bucket_imbalances.iter().sum();
        let n = bucket_imbalances.len() as f64;
        (total_imbalance / (n * self.bucket_size_sol)).clamp(0.0, 1.0)
    }

    pub fn validate_entry_vpin(&self, trades: &[TradeFlow], cfg: &SandboxConfig) -> Result<f64, &'static str> {
        let vpin = self.compute_vpin(trades);
        if vpin > cfg.vpin_max_entry_toxicity {
            return Err("VPIN_ENTRY_TOXICITY_EXCEEDED");
        }
        Ok(vpin)
    }

    pub fn check_emergency_exit(&self, trades_during_hold: &[TradeFlow], cfg: &SandboxConfig) -> bool {
        let vpin = self.compute_vpin(trades_during_hold);
        vpin >= cfg.vpin_emergency_exit_toxicity
    }
}

// ============================================================================
// METHOD 4: EMPIRICAL BAYES WALK-FORWARD WALLET MATRIX
// ============================================================================

#[derive(Debug, Clone)]
pub struct EmpiricalBayesMatrix {
    pub prior_alpha: f64,
    pub prior_beta: f64,
    pub registry: std::collections::HashMap<String, (u32, u32)>,
}

impl EmpiricalBayesMatrix {
    pub fn new(prior_alpha: f64, prior_beta: f64) -> Self {
        Self {
            prior_alpha,
            prior_beta,
            registry: std::collections::HashMap::new(),
        }
    }

    pub fn from_config(cfg: &SandboxConfig) -> Self {
        Self {
            prior_alpha: cfg.bayes_prior_alpha,
            prior_beta: cfg.bayes_prior_beta,
            registry: std::collections::HashMap::new(),
        }
    }

    /// Strictly chronological update of historical outcomes: record trade settled prior to current trade
    pub fn record_trade(&mut self, entity_id: &str, is_win: bool) {
        if entity_id.is_empty() {
            return;
        }
        let entry = self.registry.entry(entity_id.to_string()).or_insert((0, 0));
        entry.1 += 1;
        if is_win {
            entry.0 += 1;
        }
    }

    /// Beta-Binomial posterior expectation: (W + alpha) / (N + alpha + beta)
    pub fn score(&self, entity_id: &str) -> f64 {
        let (wins, total) = self.registry.get(entity_id).copied().unwrap_or((0, 0));
        let num = wins as f64 + self.prior_alpha;
        let den = total as f64 + self.prior_alpha + self.prior_beta;
        num / den
    }

    pub fn validate_wallet_score(&self, entity_id: &str, cfg: &SandboxConfig) -> Result<f64, &'static str> {
        let s = self.score(entity_id);
        if s < cfg.bayes_min_wallet_score {
            return Err("EMPIRICAL_BAYES_SCORE_TOO_LOW");
        }
        Ok(s)
    }
}

// ============================================================================
// METHOD 5: BONDING-CURVE SATURATION & AMM ELASTICITY
// ============================================================================

#[derive(Debug, Clone)]
pub struct CurveElasticityModel;

impl CurveElasticityModel {
    pub fn compute_bonding_curve_saturation_pct(fdv_usd: f64, cfg: &SandboxConfig) -> f64 {
        if cfg.graduation_target_fdv_usd > 0.0 {
            (fdv_usd / cfg.graduation_target_fdv_usd) * 100.0
        } else {
            100.0
        }
    }

    pub fn is_in_danger_zone(fdv_usd: f64, cfg: &SandboxConfig) -> bool {
        let sat = Self::compute_bonding_curve_saturation_pct(fdv_usd, cfg);
        sat >= cfg.graduation_danger_zone_min_pct && sat <= cfg.graduation_danger_zone_max_pct
    }

    /// Constant-product AMM reserve elasticity: price impact estimate = trade_sol / pool_sol_reserve
    pub fn estimate_amm_price_impact_pct(size_sol: f64, pool_sol_reserve: f64) -> f64 {
        if pool_sol_reserve <= 0.0 {
            return 100.0;
        }
        (size_sol / pool_sol_reserve) * 100.0
    }

    pub fn compute_trajectory_convexity(
        entry_price_sol: f64,
        peak_price_sol: f64,
        trough_price_sol: f64,
    ) -> f64 {
        let downside = (entry_price_sol - trough_price_sol).max(0.0);
        let upside = (peak_price_sol - entry_price_sol).max(0.0);
        if downside > 0.0 {
            upside / downside
        } else if upside > 0.0 {
            10.0 // highly convex upside
        } else {
            1.0
        }
    }

    pub fn validate_elasticity(
        fdv_usd: f64,
        size_sol: f64,
        liquidity_usd: f64,
        sol_price_usd: f64,
        cfg: &SandboxConfig,
    ) -> Result<(), &'static str> {
        if Self::is_in_danger_zone(fdv_usd, cfg) {
            return Err("BONDING_CURVE_DANGER_ZONE");
        }
        let pool_sol_reserve = if sol_price_usd > 0.0 && liquidity_usd > 0.0 {
            (liquidity_usd / 2.0) / sol_price_usd
        } else {
            100.0
        };
        let impact = Self::estimate_amm_price_impact_pct(size_sol, pool_sol_reserve);
        if impact > cfg.max_amm_price_impact_pct {
            return Err("AMM_PRICE_IMPACT_EXCEEDED");
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

    #[test]
    fn test_hawkes_intensity_and_branching() {
        let cfg = SandboxConfig::default();
        let estimator = HawkesEstimator::from_config(&cfg);

        // Branching ratio = 0.45 / 0.60 = 0.75 (within [0.55, 0.98])
        assert!((estimator.branching_ratio() - 0.75).abs() < 1e-4);

        let arrivals = vec![1.0, 2.0, 2.5, 3.0];
        let current_time = 3.5;
        let intensity = estimator.intensity(current_time, &arrivals);
        assert!(intensity > cfg.hawkes_mu_baseline);

        let res = estimator.is_cascade_healthy(&arrivals, current_time, &cfg);
        assert!(res.is_ok());

        // Apex exhaustion check
        let decaying_intensities = vec![1.2, 0.9, 0.7];
        assert!(estimator.is_apex_exhaustion(&decaying_intensities));

        let rising_intensities = vec![0.7, 0.9, 1.2];
        assert!(!estimator.is_apex_exhaustion(&rising_intensities));
    }

    #[test]
    fn test_sybil_bundle_and_entropy() {
        let cfg = SandboxConfig::default();

        // High entropy distribution: 10 equal holders
        let equal_holders = vec![10.0; 10];
        let h_high = SybilBundleAuditor::compute_shannon_entropy(&equal_holders);
        // H = - 10 * (0.1 * ln(0.1)) = ln(10) ≈ 2.3025 >= 1.85
        assert!(h_high >= cfg.min_holder_shannon_entropy);

        // Low entropy distribution: 1 whale holds 95%, 9 holders hold 5%
        let mut whale_holders = vec![0.5555; 9];
        whale_holders.insert(0, 95.0);
        let h_low = SybilBundleAuditor::compute_shannon_entropy(&whale_holders);
        assert!(h_low < cfg.min_holder_shannon_entropy);

        // Gate validation
        assert!(SybilBundleAuditor::validate_bundle_and_sybil(0.20, &equal_holders, 0.15, &cfg).is_ok());
        assert_eq!(
            SybilBundleAuditor::validate_bundle_and_sybil(0.40, &equal_holders, 0.15, &cfg),
            Err("SLOT0_BUNDLE_SUPPLY_EXCEEDED")
        );
        assert_eq!(
            SybilBundleAuditor::validate_bundle_and_sybil(0.20, &whale_holders, 0.15, &cfg),
            Err("HOLDER_SHANNON_ENTROPY_TOO_LOW")
        );
        assert_eq!(
            SybilBundleAuditor::validate_bundle_and_sybil(0.20, &equal_holders, 0.35, &cfg),
            Err("SHARED_FUNDER_CLUSTER_EXCEEDED")
        );
    }

    #[test]
    fn test_vpin_calculation_and_gates() {
        let cfg = SandboxConfig::default();
        let calc = VpinCalculator::from_config(&cfg);

        // Balanced flow: 10 SOL buy, 10 SOL sell
        let balanced_trades = vec![
            TradeFlow { volume_sol: 10.0, is_buy: true, timestamp_sec: 100 },
            TradeFlow { volume_sol: 10.0, is_buy: false, timestamp_sec: 105 },
        ];
        let _vpin_balanced = calc.compute_vpin(&balanced_trades);
        // Each bucket has 10.0 volume; bucket 1 is 100% buy, bucket 2 is 100% sell -> |10-0| + |0-10| = 20 / 20 = 1.0
        // Sliced into 2 buckets of 10 SOL each

        // Highly balanced trade inside same bucket: 5 SOL buy + 5 SOL sell = 10 SOL bucket
        let mixed_trades = vec![
            TradeFlow { volume_sol: 5.0, is_buy: true, timestamp_sec: 100 },
            TradeFlow { volume_sol: 5.0, is_buy: false, timestamp_sec: 105 },
        ];
        let vpin_mixed = calc.compute_vpin(&mixed_trades);
        // Net imbalance = |5 - 5| = 0.0 -> VPIN = 0.0
        assert_eq!(vpin_mixed, 0.0);
        assert!(calc.validate_entry_vpin(&mixed_trades, &cfg).is_ok());

        // Extreme toxic one-sided flow
        let toxic_trades = vec![
            TradeFlow { volume_sol: 20.0, is_buy: false, timestamp_sec: 100 },
        ];
        let vpin_toxic = calc.compute_vpin(&toxic_trades);
        assert_eq!(vpin_toxic, 1.0);
        assert_eq!(calc.validate_entry_vpin(&toxic_trades, &cfg), Err("VPIN_ENTRY_TOXICITY_EXCEEDED"));
        assert!(calc.check_emergency_exit(&toxic_trades, &cfg));
    }

    #[test]
    fn test_empirical_bayes_walk_forward() {
        let cfg = SandboxConfig::default();
        let mut matrix = EmpiricalBayesMatrix::from_config(&cfg);

        // Unobserved wallet: prior expectation = alpha / (alpha + beta) = 2.0 / 5.0 = 0.40 (< 0.42)
        assert_eq!(matrix.score("unknown_wallet"), 0.40);
        assert_eq!(matrix.validate_wallet_score("unknown_wallet", &cfg), Err("EMPIRICAL_BAYES_SCORE_TOO_LOW"));

        // Entity with 3 wins and 0 losses: (3 + 2) / (3 + 5) = 5 / 8 = 0.625 (>= 0.42)
        matrix.record_trade("smart_deployer", true);
        matrix.record_trade("smart_deployer", true);
        matrix.record_trade("smart_deployer", true);
        assert_eq!(matrix.score("smart_deployer"), 0.625);
        assert!(matrix.validate_wallet_score("smart_deployer", &cfg).is_ok());

        // Toxic entity with 0 wins and 5 losses: (0 + 2) / (5 + 5) = 0.20 (< 0.42)
        for _ in 0..5 {
            matrix.record_trade("rug_deployer", false);
        }
        assert_eq!(matrix.score("rug_deployer"), 0.20);
        assert_eq!(matrix.validate_wallet_score("rug_deployer", &cfg), Err("EMPIRICAL_BAYES_SCORE_TOO_LOW"));
    }

    #[test]
    fn test_curve_elasticity_and_danger_zone() {
        let cfg = SandboxConfig::default();

        // FDV $65,000 USD on $69,000 graduation target = 94.2% -> IN DANGER ZONE [88%, 102%]
        assert!(CurveElasticityModel::is_in_danger_zone(65_000.0, &cfg));
        assert_eq!(
            CurveElasticityModel::validate_elasticity(65_000.0, 0.04, 30_000.0, 180.0, &cfg),
            Err("BONDING_CURVE_DANGER_ZONE")
        );

        // FDV $35,000 USD = 50.7% -> SAFE ZONE
        assert!(!CurveElasticityModel::is_in_danger_zone(35_000.0, &cfg));

        // AMM Price Impact check: pool liq = $36,000 -> pool SOL reserve = $18,000 / $180 = 100 SOL.
        // Trade size 0.04 SOL -> impact = 0.04% <= 2.5% -> Ok
        assert!(CurveElasticityModel::validate_elasticity(35_000.0, 0.04, 36_000.0, 180.0, &cfg).is_ok());

        // Extreme size relative to tiny pool: trade 5 SOL into 100 SOL reserve = 5.0% > 2.5%
        let impact = CurveElasticityModel::estimate_amm_price_impact_pct(5.0, 100.0);
        assert_eq!(impact, 5.0);
    }
}
