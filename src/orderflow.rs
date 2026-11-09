// DMNE QUANT // REAL-TIME CUMULATIVE VOLUME DELTA (CVD) & ORDERFLOW ENGINE
// Sub-second Taker Pressure Estimation & Bearish Absorption Divergence Tracking

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const DEFAULT_CVD_WINDOW_SECS: f64 = 60.0;
pub const DEFAULT_MAX_CAPACITY: usize = 5000;

/// Represents an individual executed DEX swap trade tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DexTradeTick {
    pub timestamp_secs: f64,
    pub volume_sol: f64,
    pub is_buy: bool,
}

/// Orderflow divergence classification based on price action vs cumulative volume delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CvdDivergence {
    Neutral,
    BullishConfirmed,
    BearishAbsorption,
}

/// Rolling bounded ring buffer tracking taker trades and cumulative volume delta (CVD).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RollingCvdTracker {
    pub trades: VecDeque<DexTradeTick>,
    pub window_secs: f64,
    pub max_capacity: usize,
}

impl Default for RollingCvdTracker {
    fn default() -> Self {
        Self::new(DEFAULT_CVD_WINDOW_SECS)
    }
}

impl RollingCvdTracker {
    /// Creates a new rolling CVD tracker with the specified lookback window in seconds.
    pub fn new(window_secs: f64) -> Self {
        Self::with_capacity(window_secs, DEFAULT_MAX_CAPACITY)
    }

    /// Creates a new rolling CVD tracker with custom lookback window and capacity limit.
    pub fn with_capacity(window_secs: f64, max_capacity: usize) -> Self {
        Self {
            trades: VecDeque::with_capacity(max_capacity.min(512)),
            window_secs: window_secs.max(1.0),
            max_capacity: max_capacity.max(10),
        }
    }

    /// Prunes expired trade ticks strictly outside the rolling time window.
    pub fn prune_expired(&mut self, current_time_secs: f64) {
        let cutoff = current_time_secs - self.window_secs;
        while let Some(front) = self.trades.front() {
            if front.timestamp_secs < cutoff {
                self.trades.pop_front();
            } else {
                break;
            }
        }
    }

    /// Records an executed trade into the rolling window.
    pub fn record_trade(&mut self, timestamp: f64, volume_sol: f64, is_buy: bool) {
        self.prune_expired(timestamp);
        if self.trades.len() >= self.max_capacity && self.max_capacity > 0 {
            self.trades.pop_front();
        }
        self.trades.push_back(DexTradeTick {
            timestamp_secs: timestamp,
            volume_sol: volume_sol.max(0.0),
            is_buy,
        });
    }

    /// Records aggregated taker volume partitioned by buy and sell transaction counts.
    pub fn record_snapshot_volume(
        &mut self,
        timestamp: f64,
        volume_sol: f64,
        buys: u64,
        sells: u64,
    ) {
        self.prune_expired(timestamp);
        let total = (buys + sells) as f64;
        if total > 0.0 && volume_sol > 0.0 {
            let buy_vol = volume_sol * (buys as f64 / total);
            let sell_vol = volume_sol * (sells as f64 / total);
            if buy_vol > 0.0 {
                self.record_trade(timestamp, buy_vol, true);
            }
            if sell_vol > 0.0 {
                self.record_trade(timestamp, sell_vol, false);
            }
        }
    }

    /// Computes net cumulative volume delta in SOL (Buy Volume - Sell Volume).
    pub fn net_cvd_sol(&self) -> f64 {
        let mut net_cvd = 0.0;
        for trade in &self.trades {
            if trade.is_buy {
                net_cvd += trade.volume_sol;
            } else {
                net_cvd -= trade.volume_sol;
            }
        }
        net_cvd
    }

    /// Computes normalized taker pressure ratio in [-1.0, +1.0]: (Buy Vol - Sell Vol) / Total Vol.
    pub fn pressure_ratio(&self) -> f64 {
        let mut buy_vol = 0.0;
        let mut sell_vol = 0.0;
        for trade in &self.trades {
            if trade.is_buy {
                buy_vol += trade.volume_sol;
            } else {
                sell_vol += trade.volume_sol;
            }
        }
        let total_vol = buy_vol + sell_vol;
        if total_vol > 0.0 {
            (buy_vol - sell_vol) / total_vol
        } else {
            0.0
        }
    }

    /// Evaluates orderflow divergence against price action and absorption thresholds.
    pub fn evaluate_divergence(
        &self,
        price_change_pct: f64,
        absorption_threshold: f64,
    ) -> CvdDivergence {
        let ratio = self.pressure_ratio();
        let effective_absorption_threshold = -absorption_threshold.abs();

        if price_change_pct > 0.0 && ratio <= effective_absorption_threshold {
            CvdDivergence::BearishAbsorption
        } else if price_change_pct > 0.0 && ratio > 0.0 {
            CvdDivergence::BullishConfirmed
        } else {
            CvdDivergence::Neutral
        }
    }
}

/// Represents an individual volume bucket for VPIN calculation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VolumeBucket {
    pub bucket_volume_sol: f64,
    pub buy_volume_sol: f64,
    pub sell_volume_sol: f64,
}

impl Default for VolumeBucket {
    fn default() -> Self {
        Self {
            bucket_volume_sol: 0.0,
            buy_volume_sol: 0.0,
            sell_volume_sol: 0.0,
        }
    }
}

/// Volume-Synchronized Probability of Toxicity (VPIN) Calculator
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VpinCalculator {
    pub bucket_size_sol: f64,
    pub num_buckets: usize,
    pub completed_buckets: VecDeque<VolumeBucket>,
    pub current_bucket: VolumeBucket,
    pub toxic_consecutive_buckets: u32,
}

impl Default for VpinCalculator {
    fn default() -> Self {
        Self::new(1.0, 10)
    }
}

impl VpinCalculator {
    pub fn new(bucket_size_sol: f64, num_buckets: usize) -> Self {
        Self {
            bucket_size_sol: bucket_size_sol.max(0.001),
            num_buckets: num_buckets.max(5),
            completed_buckets: VecDeque::with_capacity(num_buckets),
            current_bucket: VolumeBucket::default(),
            toxic_consecutive_buckets: 0,
        }
    }

    /// Fills current bucket; when accumulated volume >= bucket_size_sol, pushes to completed_buckets (retaining last num_buckets).
    pub fn add_trade(&mut self, is_buy: bool, amount_sol: f64) {
        if amount_sol <= 0.0 {
            return;
        }

        let mut rem = amount_sol;
        while rem > 0.0 {
            let space = (self.bucket_size_sol - self.current_bucket.bucket_volume_sol).max(0.0);
            if rem < space {
                self.current_bucket.bucket_volume_sol += rem;
                if is_buy {
                    self.current_bucket.buy_volume_sol += rem;
                } else {
                    self.current_bucket.sell_volume_sol += rem;
                }
                rem = 0.0;
            } else {
                self.current_bucket.bucket_volume_sol += space;
                if is_buy {
                    self.current_bucket.buy_volume_sol += space;
                } else {
                    self.current_bucket.sell_volume_sol += space;
                }
                rem -= space;

                if self.completed_buckets.len() >= self.num_buckets {
                    self.completed_buckets.pop_front();
                }
                self.completed_buckets.push_back(self.current_bucket.clone());
                self.current_bucket = VolumeBucket::default();

                if self.is_toxic_sell_dominated() {
                    self.toxic_consecutive_buckets += 1;
                } else {
                    self.toxic_consecutive_buckets = 0;
                }
            }
        }
    }

    /// Computes Volume-Synchronized Probability of Toxicity (VPIN)
    /// Requires at least 5 completed buckets.
    /// VPIN = \sum_{\tau=1}^K |V_\tau^B - V_\tau^S| / (K * V)
    /// Returns Some(vpin) in range [0.0, 1.0].
    pub fn compute_vpin(&self) -> Option<f64> {
        let k = self.completed_buckets.len();
        if k < 5 {
            return None;
        }

        let mut sum_imbalance = 0.0;
        let mut total_vol = 0.0;
        for bucket in &self.completed_buckets {
            sum_imbalance += (bucket.buy_volume_sol - bucket.sell_volume_sol).abs();
            total_vol += bucket.bucket_volume_sol;
        }

        if total_vol <= 0.0 {
            return Some(0.0);
        }

        let vpin = (sum_imbalance / total_vol).clamp(0.0, 1.0);
        Some(vpin)
    }

    /// Returns true if vpin >= 0.70 AND total_sell_volume > total_buy_volume * 1.5
    pub fn is_toxic_sell_dominated(&self) -> bool {
        if let Some(vpin) = self.compute_vpin() {
            if vpin >= 0.70 {
                let total_buy: f64 = self.completed_buckets.iter().map(|b| b.buy_volume_sol).sum();
                let total_sell: f64 = self.completed_buckets.iter().map(|b| b.sell_volume_sol).sum();
                return total_sell > total_buy * 1.5;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cvd_calculation_balanced_and_skewed() {
        let mut tracker = RollingCvdTracker::new(60.0);

        // 1. Buy-only flow: Expect +1.0 pressure ratio
        tracker.record_trade(100.0, 50.0, true);
        tracker.record_trade(101.0, 25.0, true);
        assert_eq!(tracker.net_cvd_sol(), 75.0);
        assert!((tracker.pressure_ratio() - 1.0).abs() < 1e-6);

        // 2. Sell-only flow: Expect -1.0 pressure ratio
        let mut sell_tracker = RollingCvdTracker::new(60.0);
        sell_tracker.record_trade(100.0, 40.0, false);
        sell_tracker.record_trade(101.0, 60.0, false);
        assert_eq!(sell_tracker.net_cvd_sol(), -100.0);
        assert!((sell_tracker.pressure_ratio() - (-1.0)).abs() < 1e-6);

        // 3. Balanced flow: Expect 0.0 pressure ratio
        let mut balanced_tracker = RollingCvdTracker::new(60.0);
        balanced_tracker.record_trade(100.0, 50.0, true);
        balanced_tracker.record_trade(101.0, 50.0, false);
        assert_eq!(balanced_tracker.net_cvd_sol(), 0.0);
        assert!((balanced_tracker.pressure_ratio() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_cvd_sliding_window_expiration() {
        let mut tracker = RollingCvdTracker::new(60.0);

        tracker.record_trade(100.0, 10.0, true);
        tracker.record_trade(130.0, 20.0, true);
        tracker.record_trade(150.0, 30.0, false);
        assert_eq!(tracker.trades.len(), 3);

        // Advance to t = 165.0: Cutoff is 165.0 - 60.0 = 105.0
        // Trade at 100.0 is expired; trades at 130.0 and 150.0 remain
        tracker.prune_expired(165.0);
        assert_eq!(tracker.trades.len(), 2);
        assert_eq!(tracker.trades[0].timestamp_secs, 130.0);
        assert_eq!(tracker.trades[1].timestamp_secs, 150.0);
        assert_eq!(tracker.net_cvd_sol(), -10.0); // 20.0 (buy) - 30.0 (sell)
    }

    #[test]
    fn test_cvd_bearish_absorption_detection() {
        let mut tracker = RollingCvdTracker::new(60.0);

        // Price +10%, but CVD -40% (30 SOL buy, 70 SOL sell)
        tracker.record_trade(100.0, 30.0, true);
        tracker.record_trade(101.0, 70.0, false);
        assert!((tracker.pressure_ratio() - (-0.40)).abs() < 1e-6);

        let absorption_threshold = -0.30;
        let divergence = tracker.evaluate_divergence(10.0, absorption_threshold);
        assert_eq!(divergence, CvdDivergence::BearishAbsorption);

        // Price +10% with positive CVD -> BullishConfirmed
        let mut bullish_tracker = RollingCvdTracker::new(60.0);
        bullish_tracker.record_trade(100.0, 60.0, true);
        bullish_tracker.record_trade(101.0, 40.0, false);
        assert_eq!(
            bullish_tracker.evaluate_divergence(10.0, absorption_threshold),
            CvdDivergence::BullishConfirmed
        );

        // Price -5% with negative CVD -> Neutral
        assert_eq!(
            tracker.evaluate_divergence(-5.0, absorption_threshold),
            CvdDivergence::Neutral
        );
    }

    #[test]
    fn test_vpin_bucket_filling_and_toxicity_flows() {
        // 1. Test insufficient buckets (< 5 buckets returns None)
        let mut vpin_calc = VpinCalculator::new(1.0, 10);
        vpin_calc.add_trade(false, 3.5); // 3 completed buckets of 1.0 SOL sell, 0.5 in current
        assert_eq!(vpin_calc.completed_buckets.len(), 3);
        assert_eq!(vpin_calc.compute_vpin(), None);

        // 2. Pure Sell Flow (5 buckets of 1.0 SOL sell):
        // Each bucket has 1.0 SOL sell, 0.0 SOL buy -> |0 - 1| = 1.0. VPIN = 5 / 5 = 1.0 >= 0.70.
        // Total sell (5.0) > total buy (0.0) * 1.5 -> is_toxic_sell_dominated() == true.
        vpin_calc.add_trade(false, 2.0); // Now 5 completed buckets
        assert_eq!(vpin_calc.completed_buckets.len(), 5);
        let vpin = vpin_calc.compute_vpin().unwrap();
        assert!((vpin - 1.0).abs() < 1e-6);
        assert!(vpin_calc.is_toxic_sell_dominated());
        assert!(vpin_calc.toxic_consecutive_buckets >= 1);

        // 3. Pure Buy Flow:
        let mut buy_calc = VpinCalculator::new(1.0, 10);
        for _ in 0..6 {
            buy_calc.add_trade(true, 1.0);
        }
        let buy_vpin = buy_calc.compute_vpin().unwrap();
        assert!((buy_vpin - 1.0).abs() < 1e-6);
        // High VPIN, but buy-dominated so NOT toxic sell dominated
        assert!(!buy_calc.is_toxic_sell_dominated());
        assert_eq!(buy_calc.toxic_consecutive_buckets, 0);

        // 4. Balanced Flow:
        let mut balanced_calc = VpinCalculator::new(1.0, 10);
        for _ in 0..6 {
            balanced_calc.add_trade(true, 0.5);
            balanced_calc.add_trade(false, 0.5);
        }
        let balanced_vpin = balanced_calc.compute_vpin().unwrap();
        assert!((balanced_vpin - 0.0).abs() < 1e-6);
        assert!(!balanced_calc.is_toxic_sell_dominated());
    }
}

