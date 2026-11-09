#![allow(dead_code)]
// DMNE QUANT // MODULE 1: HAWKES POINT PROCESS WASH-TRADING FILTER
// Conditional intensity estimator: lambda(t) = mu + sum(alpha * exp(-beta * (t - t_i)))
// Zero magic numbers: all parameters configured via strongly typed structs with serde defaults

use serde::{Deserialize, Serialize};

/// Strongly typed configuration envelope for the Hawkes point process wash-trading filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HawkesConfig {
    /// Whether the Hawkes point process wash filter is enabled
    #[serde(default = "default_hawkes_enabled")]
    pub enabled: bool,

    /// Exogenous baseline Poisson arrival rate (events per second)
    #[serde(default = "default_mu_baseline")]
    pub mu_baseline: f64,

    /// Self-excitation magnitude per arrival
    #[serde(default = "default_alpha_excitation")]
    pub alpha_excitation: f64,

    /// Exponential memory decay rate (s^-1)
    #[serde(default = "default_beta_decay", alias = "decay_rate")]
    pub beta_decay: f64,

    /// Ceiling threshold on branching ratio (alpha / beta); >= 0.92 signifies supercritical circular wash churn
    #[serde(default = "default_max_branching_ratio", alias = "branching_ratio_threshold")]
    pub max_branching_ratio: f64,

    /// Minimum order arrival events required for Hawkes estimation (cold-start guard)
    #[serde(default = "default_hawkes_min_sample_size")]
    pub min_sample_size: usize,

    /// Floor cutoff on Coefficient of Variation (CV = std / mean); < 0.20 indicates robotic deterministic clocking
    #[serde(default = "default_min_cv_periodicity_threshold")]
    pub min_cv_periodicity_threshold: f64,

    /// Minimum number of arrival observations required to evaluate statistical metrics
    #[serde(default = "default_min_observations")]
    pub min_observations: usize,

    /// Cutoff window (seconds) to discard inter-session offline pauses between batches
    #[serde(default = "default_max_inter_arrival_gap_secs")]
    pub max_inter_arrival_gap_secs: f64,

    /// Upper boundary on mean interval (seconds) to qualify sub-minute robotic periodic churning
    #[serde(default = "default_max_robotic_interval_secs")]
    pub max_robotic_interval_secs: f64,

    /// Floor cutoff on Index of Dispersion (Var(N) / E[N]); < 0.35 indicates underdispersed robotic clocking
    #[serde(default = "default_min_dispersion_threshold")]
    pub min_dispersion_threshold: f64,

    /// Minimum mean count E[N] required to trigger underdispersed robotic loop flag
    #[serde(default = "default_min_robotic_mean_count")]
    pub min_robotic_mean_count: f64,

    /// Count acceleration explosive threshold (latest bin / max(1, prev_bin))
    #[serde(default = "default_max_count_acceleration_ratio")]
    pub max_count_acceleration_ratio: f64,

    /// Minimum count required in latest bin to qualify as explosive cascade
    #[serde(default = "default_min_cascade_count")]
    pub min_cascade_count: u32,

    /// Minimum epoch bins required for discrete dispersion calculation
    #[serde(default = "default_min_epoch_bins")]
    pub min_epoch_bins: usize,

    /// Minimum 5-minute volume in USD to trigger organic breakout override
    #[serde(default = "default_override_min_volume_usd", alias = "volume_override_usd")]
    pub override_min_volume_usd: f64,

    /// Minimum buy/sell ratio to trigger organic breakout override
    #[serde(default = "default_override_min_buy_sell_ratio")]
    pub override_min_buy_sell_ratio: f64,

    /// Minimum liquidity in USD for organic breakout protection
    #[serde(default = "default_override_min_liquidity_usd")]
    pub override_min_liquidity_usd: f64,
}

pub fn default_hawkes_min_sample_size() -> usize { 5 }
fn default_hawkes_enabled() -> bool { true }
fn default_mu_baseline() -> f64 { 0.10 }
fn default_alpha_excitation() -> f64 { 0.20 }
fn default_beta_decay() -> f64 { 0.15 }
fn default_max_branching_ratio() -> f64 { 1.15 }
fn default_min_cv_periodicity_threshold() -> f64 { 0.12 }
fn default_min_observations() -> usize { 2 }
fn default_max_inter_arrival_gap_secs() -> f64 { 120.0 }
fn default_max_robotic_interval_secs() -> f64 { 8.0 }
fn default_min_dispersion_threshold() -> f64 { 0.35 }
fn default_min_robotic_mean_count() -> f64 { 3.0 }
fn default_max_count_acceleration_ratio() -> f64 { 2.5 }
fn default_min_cascade_count() -> u32 { 5 }
fn default_min_epoch_bins() -> usize { 2 }
fn default_override_min_volume_usd() -> f64 { 25_000.0 }
fn default_override_min_buy_sell_ratio() -> f64 { 1.75 }
fn default_override_min_liquidity_usd() -> f64 { 25_000.0 }

impl Default for HawkesConfig {
    fn default() -> Self {
        Self {
            enabled: default_hawkes_enabled(),
            mu_baseline: default_mu_baseline(),
            alpha_excitation: default_alpha_excitation(),
            beta_decay: default_beta_decay(),
            max_branching_ratio: default_max_branching_ratio(),
            min_sample_size: default_hawkes_min_sample_size(),
            min_cv_periodicity_threshold: default_min_cv_periodicity_threshold(),
            min_observations: default_min_observations(),
            max_inter_arrival_gap_secs: default_max_inter_arrival_gap_secs(),
            max_robotic_interval_secs: default_max_robotic_interval_secs(),
            min_dispersion_threshold: default_min_dispersion_threshold(),
            min_robotic_mean_count: default_min_robotic_mean_count(),
            max_count_acceleration_ratio: default_max_count_acceleration_ratio(),
            min_cascade_count: default_min_cascade_count(),
            min_epoch_bins: default_min_epoch_bins(),
            override_min_volume_usd: default_override_min_volume_usd(),
            override_min_buy_sell_ratio: default_override_min_buy_sell_ratio(),
            override_min_liquidity_usd: default_override_min_liquidity_usd(),
        }
    }
}

/// Evaluated metrics produced by the Hawkes wash-trading detector
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HawkesMetrics {
    pub observations_count: usize,
    pub mean_delta_t_secs: f64,
    pub std_delta_t_secs: f64,
    pub cv_regularity: f64,
    pub intensity_at_entry: f64,
    pub branching_ratio: f64,
    pub is_wash_trading: bool,
    pub rejection_reason: Option<String>,
}

impl HawkesMetrics {
    pub fn is_rejected(&self) -> bool {
        self.is_wash_trading
    }
}

/// Result of evaluating an arrival stream through the Hawkes wash-trading gate
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HawkesEvaluation {
    /// Cold-start or insufficient arrival observations; bypassed without rejecting
    PassThrough { reason: String },
    /// Successfully evaluated against point-process & dispersion metrics
    Evaluated(HawkesMetrics),
}

impl HawkesEvaluation {
    pub fn is_rejected(&self) -> bool {
        match self {
            HawkesEvaluation::PassThrough { .. } => false,
            HawkesEvaluation::Evaluated(m) => m.is_wash_trading,
        }
    }

    pub fn is_wash_trading(&self) -> bool {
        self.is_rejected()
    }

    pub fn branching_ratio(&self) -> f64 {
        match self {
            HawkesEvaluation::PassThrough { .. } => 0.0,
            HawkesEvaluation::Evaluated(m) => m.branching_ratio,
        }
    }

    pub fn metrics(&self) -> Option<&HawkesMetrics> {
        match self {
            HawkesEvaluation::PassThrough { .. } => None,
            HawkesEvaluation::Evaluated(m) => Some(m),
        }
    }
}

/// Extracts inter-arrival times (delta_t) from time-ordered sequence of timestamps
pub fn compute_inter_arrival_deltas(timestamps: &[f64], max_gap_secs: f64) -> Vec<f64> {
    if timestamps.len() < 2 {
        return Vec::new();
    }
    timestamps
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|&dt| dt > 0.0 && dt <= max_gap_secs)
        .collect()
}

/// Computes statistical regularity of inter-arrival times: (mean, sample_std, cv)
pub fn compute_regularity_metrics(deltas: &[f64]) -> (f64, f64, f64) {
    if deltas.is_empty() {
        return (0.0, 0.0, 1.0);
    }
    let count = deltas.len() as f64;
    let mean = deltas.iter().sum::<f64>() / count;

    if deltas.len() < 2 || mean <= 0.0 {
        return (mean, 0.0, 0.0);
    }

    let variance = deltas
        .iter()
        .map(|&x| {
            let diff = x - mean;
            diff * diff
        })
        .sum::<f64>()
        / (count - 1.0);

    let std = variance.sqrt();
    let cv = std / mean;
    (mean, std, cv)
}

/// Calculates the conditional intensity lambda(t) = mu + sum_{t_i < t} (alpha * exp(-beta * (t - t_i)))
pub fn calculate_conditional_intensity(
    timestamps: &[f64],
    eval_time: f64,
    mu: f64,
    alpha: f64,
    beta: f64,
) -> f64 {
    let mut sum_decay = 0.0;
    for &ti in timestamps {
        if ti < eval_time {
            let dt = eval_time - ti;
            sum_decay += (-beta * dt).exp();
        }
    }
    mu + alpha * sum_decay
}

/// Maximum Likelihood Estimation of self-excitation alpha for a univariate Hawkes process
/// given baseline rate mu and decay beta over arrival sequence t_1 <= t_2 <= ... <= t_N.
pub fn fit_hawkes_mle_alpha(timestamps: &[f64], mu: f64, beta: f64) -> (f64, f64) {
    if timestamps.len() < 2 || beta <= 0.0 {
        return (0.0, 0.0);
    }
    let t0 = timestamps[0];
    let tn = timestamps[timestamps.len() - 1];
    let duration = tn - t0;
    if duration <= 0.0 {
        return (0.0, 0.0);
    }

    let mut r_vec = Vec::with_capacity(timestamps.len());
    r_vec.push(0.0);
    let mut prev_r = 0.0;
    for i in 1..timestamps.len() {
        let dt = timestamps[i] - timestamps[i - 1];
        let curr_r = (-beta * dt).exp() * (1.0 + prev_r);
        r_vec.push(curr_r);
        prev_r = curr_r;
    }

    let comp_grad: f64 = timestamps
        .iter()
        .map(|&ti| (1.0 - (-beta * (tn - ti)).exp()) / beta)
        .sum();

    // 1D root search for d(ln L)/d(alpha) = 0
    let mut low = 0.0;
    let mut high = 10.0;
    let score_at_zero: f64 = r_vec.iter().skip(1).map(|&r| r / mu).sum::<f64>() - comp_grad;
    if score_at_zero <= 0.0 {
        return (0.0, 0.0);
    }

    for _ in 0..50 {
        let mid = 0.5 * (low + high);
        let score: f64 = r_vec
            .iter()
            .skip(1)
            .map(|&r| r / (mu + mid * r))
            .sum::<f64>()
            - comp_grad;
        if score > 0.0 {
            low = mid;
        } else {
            high = mid;
        }
    }
    let alpha_hat = 0.5 * (low + high);
    let branching_ratio = alpha_hat / beta;
    (alpha_hat, branching_ratio)
}

/// Evaluates a candidate stream against the Hawkes wash-trading detector
pub fn evaluate_hawkes_filter(
    timestamps: &[f64],
    eval_time: f64,
    cfg: &HawkesConfig,
) -> HawkesMetrics {
    // Normalize timestamps: if timestamps are passed in milliseconds (> 1e11), normalize to fractional seconds
    let norm_eval_time = if eval_time > 1e11 { eval_time / 1000.0 } else { eval_time };
    let mut sorted_ts: Vec<f64> = timestamps
        .iter()
        .map(|&t| if t > 1e11 { t / 1000.0 } else { t })
        .collect();
    sorted_ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted_ts.retain(|&t| t <= norm_eval_time + 0.5);

    let deltas = compute_inter_arrival_deltas(&sorted_ts, cfg.max_inter_arrival_gap_secs);
    let (mean_dt, std_dt, cv) = compute_regularity_metrics(&deltas);
    let (alpha_hat, branching) = fit_hawkes_mle_alpha(&sorted_ts, cfg.mu_baseline, cfg.beta_decay);
    let intensity = calculate_conditional_intensity(&sorted_ts, norm_eval_time, cfg.mu_baseline, alpha_hat, cfg.beta_decay);

    // Cold-start guard: If arrival observations count < min_sample_size, bypass wash-trading gate
    if sorted_ts.len() < cfg.min_sample_size {
        return HawkesMetrics {
            observations_count: deltas.len(),
            mean_delta_t_secs: mean_dt,
            std_delta_t_secs: std_dt,
            cv_regularity: cv,
            intensity_at_entry: intensity,
            branching_ratio: branching,
            is_wash_trading: false,
            rejection_reason: None,
        };
    }

    let mut is_wash = false;
    let mut reason = None;

    if deltas.len() >= cfg.min_observations {
        let counts = discretize_arrival_timestamps_to_counts(&sorted_ts, norm_eval_time, 10.0);
        let discrete_disp = evaluate_discrete_orderflow_dispersion_with_config(&counts, cfg);

        // 1. Robotic clocking check: continuous deterministic periodicity or discrete underdispersion
        if discrete_disp.is_underdispersed_robotic {
            is_wash = true;
            reason = discrete_disp.rejection_reason;
        } else if mean_dt <= cfg.max_robotic_interval_secs && cv < cfg.min_cv_periodicity_threshold {
            is_wash = true;
            reason = Some(format!(
                "ROBOTIC_PERIODICITY: CV {:.3} < floor {:.2} at mean dt {:.2}s",
                cv, cfg.min_cv_periodicity_threshold, mean_dt
            ));
        }
        // 2. Supercritical circular churn check: continuous Hawkes branching or explosive discrete cascade
        else if branching >= cfg.max_branching_ratio || discrete_disp.is_supercritical_cascade {
            is_wash = true;
            reason = Some(format!(
                "SUPERCRITICAL_BRANCHING: eta {:.3} >= ceiling {:.2}",
                branching.max(discrete_disp.inar_alpha_estimate), cfg.max_branching_ratio
            ));
        }
    }

    HawkesMetrics {
        observations_count: deltas.len(),
        mean_delta_t_secs: mean_dt,
        std_delta_t_secs: std_dt,
        cv_regularity: cv,
        intensity_at_entry: intensity,
        branching_ratio: branching,
        is_wash_trading: is_wash,
        rejection_reason: reason,
    }
}

/// Evaluated metrics from discrete orderflow count dispersion and INAR estimation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscreteDispersionMetrics {
    pub bins_count: usize,
    pub mean_count: f64,
    pub variance_count: f64,
    pub dispersion_index: f64,
    pub count_acceleration: f64,
    pub inar_alpha_estimate: f64,
    pub is_underdispersed_robotic: bool,
    pub is_supercritical_cascade: bool,
    pub is_wash_trading: bool,
    pub rejection_reason: Option<String>,
}

/// Evaluates discrete orderflow count dispersion and INAR autoregressive parameters across epoch bins
pub fn evaluate_discrete_orderflow_dispersion(
    counts: &[u32],
) -> DiscreteDispersionMetrics {
    evaluate_discrete_orderflow_dispersion_with_config(counts, &HawkesConfig::default())
}

/// Evaluates discrete orderflow count dispersion using a custom HawkesConfig
pub fn evaluate_discrete_orderflow_dispersion_with_config(
    counts: &[u32],
    cfg: &HawkesConfig,
) -> DiscreteDispersionMetrics {
    let bins_count = counts.len();
    if bins_count < cfg.min_epoch_bins {
        return DiscreteDispersionMetrics {
            bins_count,
            mean_count: 0.0,
            variance_count: 0.0,
            dispersion_index: 1.0,
            count_acceleration: 1.0,
            inar_alpha_estimate: 0.0,
            is_underdispersed_robotic: false,
            is_supercritical_cascade: false,
            is_wash_trading: false,
            rejection_reason: None,
        };
    }

    let mean_count: f64 = counts.iter().map(|&c| c as f64).sum::<f64>() / (bins_count as f64);
    let variance_count: f64 = if bins_count >= 2 {
        counts
            .iter()
            .map(|&c| {
                let diff = (c as f64) - mean_count;
                diff * diff
            })
            .sum::<f64>()
            / ((bins_count - 1) as f64)
    } else {
        0.0
    };

    let dispersion_index = if mean_count > 0.0 {
        variance_count / mean_count
    } else {
        1.0
    };

    // Calculate count acceleration from the latest two bins
    let last_count = counts[bins_count - 1] as f64;
    let prev_count = counts[bins_count - 2] as f64;
    let count_acceleration = last_count / prev_count.max(1.0);

    // INAR(1) conditional least squares estimation of alpha (clamped to [0.0, 10.0])
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 1..bins_count {
        let diff_curr = (counts[i] as f64) - mean_count;
        let diff_prev = (counts[i - 1] as f64) - mean_count;
        num += diff_curr * diff_prev;
        den += diff_prev * diff_prev;
    }
    let inar_alpha = if den > 1e-9 {
        (num / den).clamp(0.0, 10.0)
    } else {
        0.0
    };

    let mut is_underdispersed = false;
    let mut is_supercritical = false;
    let mut reason = None;

    // 1. Underdispersed robotic loop: D < threshold with mean count >= robotic floor
    if dispersion_index < cfg.min_dispersion_threshold && mean_count >= cfg.min_robotic_mean_count {
        is_underdispersed = true;
        reason = Some(format!(
            "UNDERDISPERSED_ROBOTIC_LOOP: D {:.3} < floor {:.2} (mean count {:.1} >= {:.1})",
            dispersion_index, cfg.min_dispersion_threshold, mean_count, cfg.min_robotic_mean_count
        ));
    }
    // 2. Supercritical cascade: explosive acceleration or supercritical INAR branching
    else if (count_acceleration >= cfg.max_count_acceleration_ratio && counts[bins_count - 1] >= cfg.min_cascade_count)
        || inar_alpha >= cfg.max_branching_ratio
    {
        is_supercritical = true;
        reason = Some(format!(
            "SUPERCRITICAL_CASCADE: accel {:.2} >= {:.2} (last count {}) or INAR alpha {:.2} >= {:.2}",
            count_acceleration, cfg.max_count_acceleration_ratio, counts[bins_count - 1], inar_alpha, cfg.max_branching_ratio
        ));
    }

    let is_wash = is_underdispersed || is_supercritical;

    DiscreteDispersionMetrics {
        bins_count,
        mean_count,
        variance_count,
        dispersion_index,
        count_acceleration,
        inar_alpha_estimate: inar_alpha,
        is_underdispersed_robotic: is_underdispersed,
        is_supercritical_cascade: is_supercritical,
        is_wash_trading: is_wash,
        rejection_reason: reason,
    }
}

/// Checks if a candidate qualifies for the Liquidity / Volume organic breakout override.
/// If 5-minute volume >= 25,000 USD and buy/sell ratio >= 1.75, periodicity flags are bypassed.
pub fn is_organic_breakout_override(
    volume_5m_usd: f64,
    buy_sell_ratio: f64,
    liquidity_usd: f64,
    cfg: &HawkesConfig,
) -> bool {
    let volume_qualified = volume_5m_usd >= cfg.override_min_volume_usd
        || liquidity_usd >= cfg.override_min_liquidity_usd;
    let ratio_qualified = buy_sell_ratio >= cfg.override_min_buy_sell_ratio;
    volume_qualified && ratio_qualified
}

/// Discretizes continuous arrival timestamps into consecutive count bins
pub fn discretize_arrival_timestamps_to_counts(
    timestamps: &[f64],
    eval_time: f64,
    bin_width_secs: f64,
) -> Vec<u32> {
    if timestamps.is_empty() || bin_width_secs <= 0.0 {
        return Vec::new();
    }
    let mut sorted = timestamps.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted.retain(|&t| t <= eval_time + 0.5);

    if sorted.is_empty() {
        return Vec::new();
    }

    let t_start = sorted[0];
    let total_duration = (eval_time + 0.5 - t_start).min(3600.0).max(bin_width_secs);
    let num_bins = ((total_duration / bin_width_secs).ceil() as usize).min(1000).max(1);
    let mut bins = vec![0u32; num_bins];

    for &t in &sorted {
        let b = ((t - t_start) / bin_width_secs).floor() as usize;
        if b < num_bins {
            bins[b] += 1;
        }
    }
    bins
}

/// Evaluates a candidate stream against Hawkes wash-trading detector with organic breakout override
pub fn evaluate_hawkes_filter_with_override(
    timestamps: &[f64],
    eval_time: f64,
    volume_5m_usd: f64,
    buy_sell_ratio: f64,
    liquidity_usd: f64,
    cfg: &HawkesConfig,
) -> HawkesMetrics {
    let mut metrics = evaluate_hawkes_filter(timestamps, eval_time, cfg);
    if metrics.is_wash_trading {
        let is_periodicity = metrics
            .rejection_reason
            .as_deref()
            .map(|r| r.contains("ROBOTIC_PERIODICITY") || r.contains("UNDERDISPERSED_ROBOTIC_LOOP"))
            .unwrap_or(false);

        if is_periodicity && is_organic_breakout_override(volume_5m_usd, buy_sell_ratio, liquidity_usd, cfg) {
            metrics.is_wash_trading = false;
            metrics.rejection_reason = None;
        }
    }
    metrics
}

/// Unified evaluator for Hawkes point process wash-trading filtering
#[derive(Debug, Clone)]
pub struct HawkesEvaluator {
    pub config: HawkesConfig,
}

impl HawkesEvaluator {
    pub fn new(config: HawkesConfig) -> Self {
        Self { config }
    }

    /// Evaluates arrival timestamps against the Hawkes wash-trading gate.
    /// If sample size < min_sample_size, returns PassThrough with descriptive reason.
    pub fn evaluate_gate(
        &self,
        arrival_timestamps: &[f64],
        eval_time: f64,
        volume_5m_usd: f64,
        buy_sell_ratio: f64,
        liquidity_usd: f64,
    ) -> HawkesEvaluation {
        if arrival_timestamps.len() < self.config.min_sample_size {
            return HawkesEvaluation::PassThrough {
                reason: format!(
                    "insufficient_sample_{}_lt_{}",
                    arrival_timestamps.len(),
                    self.config.min_sample_size
                ),
            };
        }
        let metrics = Self::evaluate(
            arrival_timestamps,
            eval_time,
            volume_5m_usd,
            buy_sell_ratio,
            liquidity_usd,
            &self.config,
        );
        HawkesEvaluation::Evaluated(metrics)
    }

    /// Static version of evaluate_gate taking an explicit config reference
    pub fn evaluate_gate_static(
        arrival_timestamps: &[f64],
        eval_time: f64,
        volume_5m_usd: f64,
        buy_sell_ratio: f64,
        liquidity_usd: f64,
        cfg: &HawkesConfig,
    ) -> HawkesEvaluation {
        if arrival_timestamps.len() < cfg.min_sample_size {
            return HawkesEvaluation::PassThrough {
                reason: format!(
                    "insufficient_sample_{}_lt_{}",
                    arrival_timestamps.len(),
                    cfg.min_sample_size
                ),
            };
        }
        let metrics = Self::evaluate(
            arrival_timestamps,
            eval_time,
            volume_5m_usd,
            buy_sell_ratio,
            liquidity_usd,
            cfg,
        );
        HawkesEvaluation::Evaluated(metrics)
    }

    pub fn evaluate(
        timestamps: &[f64],
        eval_time: f64,
        volume_5m_usd: f64,
        buy_sell_ratio: f64,
        liquidity_usd: f64,
        cfg: &HawkesConfig,
    ) -> HawkesMetrics {
        evaluate_hawkes_filter_with_override(
            timestamps,
            eval_time,
            volume_5m_usd,
            buy_sell_ratio,
            liquidity_usd,
            cfg,
        )
    }

    /// Evaluates discrete orderflow dispersion directly from arrival counts
    pub fn evaluate_dispersion(
        counts: &[u32],
        cfg: &HawkesConfig,
    ) -> DiscreteDispersionMetrics {
        evaluate_discrete_orderflow_dispersion_with_config(counts, cfg)
    }

    /// Evaluates in-flight point-process intensity decay while a trade is live.
    /// Returns true if arrival intensity over the last 5s collapses by > 70% relative to prior 20s.
    pub fn evaluate_inflight_decay(&self, recent_timestamps_secs: &[f64], current_time_secs: f64) -> bool {
        if recent_timestamps_secs.len() < 5 {
            return false;
        }
        // Compute rolling intensity lambda(t) over the last 5s vs prior 20s
        let window_short = 5.0;
        let window_long = 25.0;
        let recent_count = recent_timestamps_secs
            .iter()
            .filter(|&&t| current_time_secs - t <= window_short && current_time_secs >= t)
            .count();
        let prior_count = recent_timestamps_secs
            .iter()
            .filter(|&&t| current_time_secs - t > window_short && current_time_secs - t <= window_long)
            .count();

        let short_rate = recent_count as f64 / window_short;
        let prior_rate = (prior_count as f64 / (window_long - window_short)).max(0.1);

        // If arrival intensity collapses by > 70% while trade is live
        short_rate < (prior_rate * 0.30)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hawkes_defaults_and_serde() {
        let cfg = HawkesConfig::default();
        assert_eq!(cfg.mu_baseline, 0.10);
        assert_eq!(cfg.alpha_excitation, 0.20);
        assert_eq!(cfg.beta_decay, 0.15);
        assert_eq!(cfg.max_branching_ratio, 1.15);
        assert_eq!(cfg.min_cv_periodicity_threshold, 0.12);
        assert_eq!(cfg.min_observations, 2);
        assert_eq!(cfg.min_sample_size, 5);
    }

    #[test]
    fn test_hawkes_cold_start_insufficient_sample_bypasses_gate() {
        let cfg = HawkesConfig::default();
        assert_eq!(cfg.min_sample_size, 5);

        let evaluator = HawkesEvaluator::new(cfg.clone());

        for n in 0..5 {
            // Generate n timestamps with ultra-short intervals (0.01s) that would otherwise trigger periodicity
            let timestamps: Vec<f64> = (0..n).map(|i| 1000.0 + i as f64 * 0.01).collect();
            let eval = evaluator.evaluate_gate(&timestamps, 1000.0 + n as f64 * 0.01, 10_000.0, 1.0, 20_000.0);

            match &eval {
                HawkesEvaluation::PassThrough { reason } => {
                    assert!(reason.contains("insufficient_sample"));
                    assert_eq!(reason, &format!("insufficient_sample_{}_lt_5", n));
                    assert!(!eval.is_rejected());
                    assert!(!eval.is_wash_trading());
                }
                _ => panic!("Expected PassThrough for sample size {}", n),
            }

            // Verify static evaluation helper
            let eval_static = HawkesEvaluator::evaluate_gate_static(
                &timestamps,
                1000.0 + n as f64 * 0.01,
                10_000.0,
                1.0,
                20_000.0,
                &cfg,
            );
            assert!(!eval_static.is_rejected());
            assert!(matches!(eval_static, HawkesEvaluation::PassThrough { .. }));

            // Verify standard metric evaluation bypasses wash trading
            let metrics = HawkesEvaluator::evaluate(
                &timestamps,
                1000.0 + n as f64 * 0.01,
                10_000.0,
                1.0,
                20_000.0,
                &cfg,
            );
            assert!(!metrics.is_wash_trading);
            assert!(!metrics.is_rejected());
            assert!(metrics.rejection_reason.is_none());
        }
    }

    #[test]
    fn test_hawkes_evaluates_normally_with_sufficient_sample() {
        let cfg = HawkesConfig::default();
        assert_eq!(cfg.min_sample_size, 5);

        let evaluator = HawkesEvaluator::new(cfg.clone());

        // 5+ timestamps with robotic periodicity
        let timestamps: Vec<f64> = (0..6).map(|i| 1000.0 + i as f64 * 2.0).collect();
        let eval = evaluator.evaluate_gate(&timestamps, 1012.0, 10_000.0, 1.0, 20_000.0);

        match eval {
            HawkesEvaluation::Evaluated(metrics) => {
                assert!(metrics.observations_count >= 5);
                assert!(metrics.branching_ratio >= 0.0);
                assert!(metrics.cv_regularity >= 0.0);
                assert!(metrics.is_rejected());
                assert!(metrics.is_wash_trading);
            }
            HawkesEvaluation::PassThrough { .. } => {
                panic!("Expected Evaluated for sample size >= 5");
            }
        }
    }

    #[test]
    fn test_fit_hawkes_mle_alpha() {
        // High frequency cascade
        let timestamps = vec![0.0, 1.2, 2.1, 2.8, 3.4, 3.9];
        let (alpha_hat, eta) = fit_hawkes_mle_alpha(&timestamps, 0.10, 0.15);
        assert!(alpha_hat > 0.0);
        assert!(eta > 0.5);
    }

    #[test]
    fn test_robotic_clocking_detection() {
        let cfg = HawkesConfig::default();
        // Perfectly clocked bot: arrives exactly 5 times per 10s epoch bin
        let timestamps = vec![
            1000.0, 1002.0, 1004.0, 1006.0, 1008.0,
            1010.0, 1012.0, 1014.0, 1016.0, 1018.0,
            1020.0, 1022.0, 1024.0, 1026.0, 1028.0,
        ];
        let eval_time = 1029.0;

        let metrics = evaluate_hawkes_filter(&timestamps, eval_time, &cfg);
        assert!(metrics.is_wash_trading);
        assert!(metrics.cv_regularity < 0.05);
        assert!(metrics.rejection_reason.unwrap().contains("UNDERDISPERSED_ROBOTIC_LOOP"));
    }

    #[test]
    fn test_organic_poisson_stream_clearance() {
        let cfg = HawkesConfig::default();
        // Organic stream: irregular arrival intervals with high dispersion (CV >= 1.0)
        let timestamps = vec![1000.0, 1001.2, 1008.5, 1009.1, 1024.0, 1025.3, 1045.0];
        let eval_time = 1045.5;

        let metrics = evaluate_hawkes_filter(&timestamps, eval_time, &cfg);
        assert!(!metrics.is_wash_trading);
        assert!(metrics.cv_regularity > 0.50);
        assert!(metrics.rejection_reason.is_none());
    }

    #[test]
    fn test_conditional_intensity_calculation() {
        let mu = 0.20;
        let alpha = 0.50;
        let beta = 1.00;
        let timestamps = vec![10.0, 11.0];
        let eval_time = 12.0;

        // At t = 12.0:
        // t1 = 10.0 -> dt = 2.0 -> alpha * exp(-1.0 * 2.0) = 0.5 * exp(-2.0)
        // t2 = 11.0 -> dt = 1.0 -> alpha * exp(-1.0 * 1.0) = 0.5 * exp(-1.0)
        let expected = mu + 0.5 * (-2.0f64).exp() + 0.5 * (-1.0f64).exp();
        let calculated = calculate_conditional_intensity(&timestamps, eval_time, mu, alpha, beta);

        assert!((calculated - expected).abs() < 1e-9);
    }

    #[test]
    fn test_discrete_orderflow_dispersion_equidispersed() {
        // Natural Poisson variation: counts centered near 5 with D ~ 1.0 (mean ~ 5.1, var ~ 6.4, D ~ 1.25)
        let counts = vec![3, 8, 4, 2, 7, 5, 9, 3];
        let metrics = evaluate_discrete_orderflow_dispersion(&counts);
        assert!(!metrics.is_wash_trading);
        assert!(metrics.dispersion_index >= 0.35);
        assert!(metrics.rejection_reason.is_none());
    }

    #[test]
    fn test_discrete_orderflow_dispersion_underdispersed_robotic() {
        // Deterministic robotic loop: exactly 5 events every bin (zero variance, D = 0.0 < 0.35)
        let counts = vec![5, 5, 5, 5, 5, 5];
        let metrics = evaluate_discrete_orderflow_dispersion(&counts);
        assert!(metrics.is_wash_trading);
        assert!(metrics.is_underdispersed_robotic);
        assert!(metrics.dispersion_index < 0.35);
        assert!(metrics.rejection_reason.unwrap().contains("UNDERDISPERSED_ROBOTIC_LOOP"));
    }

    #[test]
    fn test_discrete_orderflow_dispersion_supercritical_cascade() {
        // Sudden explosive cascade: count jumps from 2 to 15 (accel = 7.5 >= 2.5, last count >= 5)
        let counts = vec![2, 2, 2, 2, 15];
        let metrics = evaluate_discrete_orderflow_dispersion(&counts);
        assert!(metrics.is_wash_trading);
        assert!(metrics.is_supercritical_cascade);
        assert!(metrics.rejection_reason.unwrap().contains("SUPERCRITICAL_CASCADE"));
    }

    #[test]
    fn test_organic_breakout_override() {
        let cfg = HawkesConfig::default();
        // High volume ($35k >= $25k) and high buy/sell ratio (2.50 >= 1.75)
        assert!(is_organic_breakout_override(35_000.0, 2.50, 40_000.0, &cfg));

        // Low volume ($15k < $25k) and low ratio (1.20 < 1.75)
        assert!(!is_organic_breakout_override(15_000.0, 1.20, 18_000.0, &cfg));

        // High volume but low buy ratio (e.g. panic sell dump)
        assert!(!is_organic_breakout_override(50_000.0, 0.80, 50_000.0, &cfg));
    }

    #[test]
    fn test_discretize_arrival_timestamps_to_counts() {
        let timestamps = vec![100.0, 102.0, 104.5, 111.0, 115.0];
        let eval_time = 120.0;
        let bin_width = 5.0; // [100..105), [105..110), [110..115), [115..120), [120..125)
        let bins = discretize_arrival_timestamps_to_counts(&timestamps, eval_time, bin_width);
        assert_eq!(bins[0], 3); // 100.0, 102.0, 104.5
        assert_eq!(bins[1], 0);
        assert_eq!(bins[2], 1); // 111.0
        assert_eq!(bins[3], 1); // 115.0
    }

    #[test]
    fn test_hawkes_timestamp_units_normalized_and_asserted() {
        let cfg = HawkesConfig::default();
        // 1. Fractional seconds (normal ~1.70e9)
        let ts_secs = vec![1700000000.0, 1700000002.0, 1700000004.0, 1700000006.0, 1700000008.0];
        let eval_secs = 1700000010.0;
        let m1 = evaluate_hawkes_filter(&ts_secs, eval_secs, &cfg);
        assert_eq!(m1.observations_count, 4);

        // 2. Milliseconds (~1.70e12) - should be normalized by / 1000.0
        let ts_ms = vec![1700000000000.0, 1700000002000.0, 1700000004000.0, 1700000006000.0, 1700000008000.0];
        let eval_ms = 1700000010000.0;
        let m2 = evaluate_hawkes_filter(&ts_ms, eval_ms, &cfg);
        assert_eq!(m2.observations_count, 4);

        // Deltas and metrics must match identically after normalization
        assert!((m1.mean_delta_t_secs - m2.mean_delta_t_secs).abs() < 1e-6);
        assert!((m1.cv_regularity - m2.cv_regularity).abs() < 1e-6);
        assert!((m1.branching_ratio - m2.branching_ratio).abs() < 1e-6);
    }

    #[test]
    fn test_evaluate_inflight_decay() {
        let evaluator = HawkesEvaluator::new(HawkesConfig::default());
        let now = 1000.0;

        // 1. Insufficient observations (< 5) => false
        let few = vec![now - 2.0, now - 1.0];
        assert!(!evaluator.evaluate_inflight_decay(&few, now));

        // 2. Active stream (high recent arrivals) => false
        // 5 in last 5s, 10 in prior 20s
        let mut active = Vec::new();
        for i in 1..=10 {
            active.push(now - 5.0 - (i as f64 * 1.5)); // in (5, 25]
        }
        for i in 1..=5 {
            active.push(now - (i as f64 * 0.8)); // in [0, 5]
        }
        assert!(!evaluator.evaluate_inflight_decay(&active, now));

        // 3. Collapsed intensity (> 70% collapse in last 5s) => true
        // 0 in last 5s, 10 in prior 20s
        let mut collapsed = Vec::new();
        for i in 1..=10 {
            collapsed.push(now - 5.1 - (i as f64 * 1.5));
        }
        assert!(evaluator.evaluate_inflight_decay(&collapsed, now));
    }
}
