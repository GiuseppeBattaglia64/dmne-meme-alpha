// DMNE QUANT // MODULE 1: HAWKES POINT PROCESS WASH-TRADING FILTER
// Conditional intensity estimator: lambda(t) = mu + sum(alpha * exp(-beta * (t - t_i)))
// Zero magic numbers: all parameters configured via strongly typed structs with serde defaults

use serde::{Deserialize, Serialize};

/// Strongly typed configuration envelope for the Hawkes point process wash-trading filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HawkesConfig {
    /// Exogenous baseline Poisson arrival rate (events per second)
    #[serde(default = "default_mu_baseline")]
    pub mu_baseline: f64,

    /// Self-excitation magnitude per arrival
    #[serde(default = "default_alpha_excitation")]
    pub alpha_excitation: f64,

    /// Exponential memory decay rate (s^-1)
    #[serde(default = "default_beta_decay")]
    pub beta_decay: f64,

    /// Ceiling threshold on branching ratio (alpha / beta); >= 0.92 signifies supercritical circular wash churn
    #[serde(default = "default_max_branching_ratio")]
    pub max_branching_ratio: f64,

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
}

fn default_mu_baseline() -> f64 { 0.10 }
fn default_alpha_excitation() -> f64 { 0.20 }
fn default_beta_decay() -> f64 { 0.15 }
fn default_max_branching_ratio() -> f64 { 1.25 }
fn default_min_cv_periodicity_threshold() -> f64 { 0.12 }
fn default_min_observations() -> usize { 2 }
fn default_max_inter_arrival_gap_secs() -> f64 { 120.0 }
fn default_max_robotic_interval_secs() -> f64 { 8.0 }

impl Default for HawkesConfig {
    fn default() -> Self {
        Self {
            mu_baseline: default_mu_baseline(),
            alpha_excitation: default_alpha_excitation(),
            beta_decay: default_beta_decay(),
            max_branching_ratio: default_max_branching_ratio(),
            min_cv_periodicity_threshold: default_min_cv_periodicity_threshold(),
            min_observations: default_min_observations(),
            max_inter_arrival_gap_secs: default_max_inter_arrival_gap_secs(),
            max_robotic_interval_secs: default_max_robotic_interval_secs(),
        }
    }
}

/// Evaluated metrics produced by the Hawkes wash-trading detector
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    let mut sorted_ts = timestamps.to_vec();
    sorted_ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted_ts.retain(|&t| t <= eval_time + 0.5);

    let deltas = compute_inter_arrival_deltas(&sorted_ts, cfg.max_inter_arrival_gap_secs);
    let (mean_dt, std_dt, cv) = compute_regularity_metrics(&deltas);
    let (alpha_hat, branching) = fit_hawkes_mle_alpha(&sorted_ts, cfg.mu_baseline, cfg.beta_decay);
    let intensity = calculate_conditional_intensity(&sorted_ts, eval_time, cfg.mu_baseline, alpha_hat, cfg.beta_decay);

    let mut is_wash = false;
    let mut reason = None;

    if deltas.len() >= cfg.min_observations {
        // 1. Robotic clocking check: high-frequency arrivals with deterministic regularity
        if mean_dt <= cfg.max_robotic_interval_secs && cv < cfg.min_cv_periodicity_threshold {
            is_wash = true;
            reason = Some(format!(
                "ROBOTIC_PERIODICITY: CV {:.3} < floor {:.2} at mean dt {:.2}s",
                cv, cfg.min_cv_periodicity_threshold, mean_dt
            ));
        }
        // 2. Supercritical circular churn check: branching ratio exceeds ceiling
        else if branching >= cfg.max_branching_ratio {
            is_wash = true;
            reason = Some(format!(
                "SUPERCRITICAL_BRANCHING: eta {:.3} >= ceiling {:.2}",
                branching, cfg.max_branching_ratio
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hawkes_defaults_and_serde() {
        let cfg = HawkesConfig::default();
        assert_eq!(cfg.mu_baseline, 0.10);
        assert_eq!(cfg.alpha_excitation, 0.20);
        assert_eq!(cfg.beta_decay, 0.15);
        assert_eq!(cfg.max_branching_ratio, 1.25);
        assert_eq!(cfg.min_cv_periodicity_threshold, 0.12);
        assert_eq!(cfg.min_observations, 2);
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
        // Perfectly clocked bot: arrives every 2.00s with negligible jitter (+- 0.02s)
        let timestamps = vec![1000.0, 1002.02, 1004.01, 1006.03, 1008.02, 1010.01];
        let eval_time = 1010.05;

        let metrics = evaluate_hawkes_filter(&timestamps, eval_time, &cfg);
        assert!(metrics.is_wash_trading);
        assert!(metrics.cv_regularity < 0.05);
        assert!(metrics.rejection_reason.unwrap().contains("ROBOTIC_PERIODICITY"));
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
}
