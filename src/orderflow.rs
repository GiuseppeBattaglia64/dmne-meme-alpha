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
}

