//! DMNE QUANT // RADAR SCANNER PRE-TRADE REJECTION GAUGE & STAGE 2 DEEP AUDIT LOG
//!
//! Thread-safe rolling telemetry tracking dominant pre-trade rejection vectors:
//!   - LOW_LIQUIDITY (Liquidity < minimum threshold)
//!   - HAWKES_WASH (Discrete Hawkes branching ratio supercritical / wash trading)
//!   - LOW_DISPERSION (Buyer/Seller Shannon entropy < threshold)
//!   - DEV_CLUSTER (Dev cluster / HHI concentration ceiling)
//!   - ADMITTED (Cleared all pre-flight checks -> Execution Slot)
//!
//! In-memory fixed ring buffer (capacity: 8) storing recent Stage 2 deep audit verdicts.
//!
//! Language: English only | Zero magic numbers | Institutional Type-Safe Schema

use std::collections::VecDeque;
use std::sync::RwLock;
use serde::{Deserialize, Serialize};

/// Maximum fixed capacity of the Stage 2 Deep Audit Log ring buffer
pub const STAGE2_AUDIT_LOG_CAPACITY: usize = 8;

/// Fixed capacity for the rolling pre-trade rejection sliding window
pub const ROLLING_WINDOW_CAPACITY: usize = 1000;

/// Rejection classification categories for the Pre-Trade Rejection Gauge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RejectionCategory {
    /// Liquidity < minimum parameter threshold
    LowLiquidity,
    /// Conviction score, momentum, volume, or transactions below cutoff
    LowConviction,
    /// Hawkes process self-excitation supercritical or cyclic periodicity
    HawkesWash,
    /// Shannon entropy or order flow dispersion below cutoff
    LowDispersion,
    /// Dev cluster or holder HHI concentration ceiling exceeded
    DevCluster,
    /// Successfully passed all pre-flight checks and admitted to candidate queue
    Admitted,
}

impl RejectionCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            RejectionCategory::LowLiquidity => "LOW_LIQUIDITY",
            RejectionCategory::LowConviction => "LOW_CONVICTION",
            RejectionCategory::HawkesWash => "HAWKES_WASH",
            RejectionCategory::LowDispersion => "LOW_DISPERSION",
            RejectionCategory::DevCluster => "DEV_CLUSTER",
            RejectionCategory::Admitted => "ADMITTED",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            RejectionCategory::LowLiquidity => 0,
            RejectionCategory::LowConviction => 1,
            RejectionCategory::HawkesWash => 2,
            RejectionCategory::LowDispersion => 3,
            RejectionCategory::DevCluster => 4,
            RejectionCategory::Admitted => 5,
        }
    }
}

/// O(1) sliding window tracker maintaining rolling category counts over the last 1,000 evaluations
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RollingRejectionTracker {
    window: VecDeque<RejectionCategory>,
    counts: [u32; 6], // [LowLiquidity, LowConviction, HawkesWash, LowDispersion, DevCluster, Admitted]
    capacity: usize,
}

impl Default for RollingRejectionTracker {
    fn default() -> Self {
        Self::new(ROLLING_WINDOW_CAPACITY)
    }
}

impl RollingRejectionTracker {
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            window: VecDeque::with_capacity(cap),
            counts: [0; 6],
            capacity: cap,
        }
    }

    /// Records an evaluation category into the sliding window in O(1) time
    pub fn record(&mut self, category: RejectionCategory) {
        if self.window.len() >= self.capacity {
            if let Some(old) = self.window.pop_front() {
                let idx = old.index();
                if self.counts[idx] > 0 {
                    self.counts[idx] -= 1;
                }
            }
        }
        self.counts[category.index()] += 1;
        self.window.push_back(category);
    }

    pub fn len(&self) -> usize {
        self.window.len()
    }

    pub fn is_empty(&self) -> bool {
        self.window.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn counts(&self) -> [u32; 6] {
        self.counts
    }

    /// Produces a normalized telemetry snapshot over the active rolling window
    pub fn snapshot(&self) -> RejectionGaugeSnapshot {
        let total = self.window.len() as f64;
        let count_liq = self.counts[0] as u64;
        let count_conv = self.counts[1] as u64;
        let count_wash = self.counts[2] as u64;
        let count_disp = self.counts[3] as u64;
        let count_dev = self.counts[4] as u64;
        let count_admitted = self.counts[5] as u64;

        let pct = |count: u64| -> f64 {
            if total > 0.0 {
                ((count as f64 / total) * 100.0 * 10.0).round() / 10.0
            } else {
                0.0
            }
        };

        RejectionGaugeSnapshot {
            total_evaluated: self.window.len() as u64,
            low_liquidity: count_liq,
            low_conviction: count_conv,
            count_low_conviction: count_conv,
            hawkes_wash: count_wash,
            low_dispersion: count_disp,
            dev_cluster: count_dev,
            admitted: count_admitted,
            pct_low_liquidity: pct(count_liq),
            pct_low_conviction: pct(count_conv),
            pct_hawkes_wash: pct(count_wash),
            pct_low_dispersion: pct(count_disp),
            pct_dev_cluster: pct(count_dev),
            pct_admitted: pct(count_admitted),
        }
    }
}

/// Thread-safe concurrent wrapper around RollingRejectionTracker
#[derive(Debug)]
pub struct ConcurrentRollingRejectionTracker {
    inner: RwLock<RollingRejectionTracker>,
}

impl Default for ConcurrentRollingRejectionTracker {
    fn default() -> Self {
        Self::new(ROLLING_WINDOW_CAPACITY)
    }
}

impl ConcurrentRollingRejectionTracker {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: RwLock::new(RollingRejectionTracker::new(capacity)),
        }
    }

    pub fn record(&self, category: RejectionCategory) {
        if let Ok(mut guard) = self.inner.write() {
            guard.record(category);
        }
    }

    pub fn snapshot(&self) -> RejectionGaugeSnapshot {
        if let Ok(guard) = self.inner.read() {
            guard.snapshot()
        } else {
            RejectionGaugeSnapshot::default()
        }
    }
}

/// Flat, lightweight DTO with primitive scalars representing a rolling telemetry snapshot
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RejectionGaugeSnapshot {
    pub total_evaluated: u64,
    pub low_liquidity: u64,
    pub low_conviction: u64,
    #[serde(default)]
    pub count_low_conviction: u64,
    pub hawkes_wash: u64,
    pub low_dispersion: u64,
    pub dev_cluster: u64,
    pub admitted: u64,
    pub pct_low_liquidity: f64,
    pub pct_low_conviction: f64,
    pub pct_hawkes_wash: f64,
    pub pct_low_dispersion: f64,
    pub pct_dev_cluster: f64,
    pub pct_admitted: f64,
}

impl RejectionGaugeSnapshot {
    /// Institutional breakdown of rejection counts by category
    pub fn rejection_breakdown(&self) -> std::collections::HashMap<&'static str, u64> {
        let mut map = std::collections::HashMap::new();
        map.insert("low_liquidity", self.low_liquidity);
        map.insert("low_conviction", self.low_conviction);
        map.insert("hawkes_wash", self.hawkes_wash);
        map.insert("low_dispersion", self.low_dispersion);
        map.insert("dev_cluster", self.dev_cluster);
        map.insert("admitted", self.admitted);
        map
    }
}

impl Default for RejectionGaugeSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

impl RejectionGaugeSnapshot {
    pub fn new() -> Self {
        Self {
            total_evaluated: 0,
            low_liquidity: 0,
            low_conviction: 0,
            count_low_conviction: 0,
            hawkes_wash: 0,
            low_dispersion: 0,
            dev_cluster: 0,
            admitted: 0,
            pct_low_liquidity: 0.0,
            pct_low_conviction: 0.0,
            pct_hawkes_wash: 0.0,
            pct_low_dispersion: 0.0,
            pct_dev_cluster: 0.0,
            pct_admitted: 0.0,
        }
    }
}

/// Backward-compatible type alias for RejectionGaugeSnapshot
pub type RejectionGaugeTelemetry = RejectionGaugeSnapshot;

/// A single evaluated candidate audit entry in Stage 2 Deep Audit Log
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage2AuditEntry {
    /// Timestamp in HH:MM:SS format
    pub timestamp: String,
    /// Token ticker symbol
    pub symbol: String,
    /// Truncated token mint address (e.g. "8nFy...PhJ8")
    pub mint: String,
    /// Pool liquidity in USD
    pub liquidity_usd: f64,
    /// Key evaluated metric value (e.g. "HHI: 0.15", "Disp: 0.82", "BR: 0.45", "Liq: $14.2k")
    pub key_metric: String,
    /// Final verdict string: "ADMITTED" or specific "REJECT: <REASON>"
    pub verdict: String,
    /// True if candidate cleared all checks and was admitted to execution slot
    pub is_admitted: bool,
}

/// In-memory fixed ring buffer with a strict maximum capacity
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage2AuditRingBuffer {
    pub capacity: usize,
    pub entries: Vec<Stage2AuditEntry>,
}

impl Default for Stage2AuditRingBuffer {
    fn default() -> Self {
        Self::new(STAGE2_AUDIT_LOG_CAPACITY)
    }
}

impl Stage2AuditRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: Vec::with_capacity(capacity.max(1)),
        }
    }

    /// Pushes a new audit entry to the front of the ring buffer, evicting the oldest if capacity is reached
    pub fn push(&mut self, entry: Stage2AuditEntry) {
        self.entries.insert(0, entry);
        if self.entries.len() > self.capacity {
            self.entries.truncate(self.capacity);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn as_slice(&self) -> &[Stage2AuditEntry] {
        &self.entries
    }
}

/// Formats a full 32-44 character Base58 Solana public key into a clean truncated string
pub fn truncate_mint_address(mint: &str) -> String {
    if mint.len() > 10 {
        format!("{}...{}", &mint[..4], &mint[mint.len() - 4..])
    } else {
        mint.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rejection_gauge_recording_and_percentages() {
        let mut tracker = RollingRejectionTracker::new(ROLLING_WINDOW_CAPACITY);
        assert_eq!(tracker.len(), 0);

        // Record 60 low liquidity, 20 hawkes wash, 10 low dispersion, 10 dev cluster, 5 admitted
        for _ in 0..60 {
            tracker.record(RejectionCategory::LowLiquidity);
        }
        for _ in 0..20 {
            tracker.record(RejectionCategory::HawkesWash);
        }
        for _ in 0..10 {
            tracker.record(RejectionCategory::LowDispersion);
        }
        for _ in 0..10 {
            tracker.record(RejectionCategory::DevCluster);
        }
        for _ in 0..5 {
            tracker.record(RejectionCategory::Admitted);
        }

        let gauge = tracker.snapshot();

        assert_eq!(gauge.total_evaluated, 105);
        assert_eq!(gauge.low_liquidity, 60);
        assert_eq!(gauge.hawkes_wash, 20);
        assert_eq!(gauge.low_dispersion, 10);
        assert_eq!(gauge.dev_cluster, 10);
        assert_eq!(gauge.admitted, 5);

        // Reconciled against total_evaluated = 105
        let expected_liq: f64 = (((60.0f64 / 105.0f64) * 100.0f64 * 10.0f64).round()) / 10.0f64;
        let expected_hawkes: f64 = (((20.0f64 / 105.0f64) * 100.0f64 * 10.0f64).round()) / 10.0f64;
        let expected_disp: f64 = (((10.0f64 / 105.0f64) * 100.0f64 * 10.0f64).round()) / 10.0f64;
        let expected_dev: f64 = (((10.0f64 / 105.0f64) * 100.0f64 * 10.0f64).round()) / 10.0f64;
        let expected_admitted: f64 = (((5.0f64 / 105.0f64) * 100.0f64 * 10.0f64).round()) / 10.0f64;

        assert_eq!(gauge.pct_low_liquidity, expected_liq);
        assert_eq!(gauge.pct_hawkes_wash, expected_hawkes);
        assert_eq!(gauge.pct_low_dispersion, expected_disp);
        assert_eq!(gauge.pct_dev_cluster, expected_dev);
        assert_eq!(gauge.pct_admitted, expected_admitted);
    }

    #[test]
    fn test_rejection_gauge_mathematical_reconciliation() {
        let mut tracker = RollingRejectionTracker::new(ROLLING_WINDOW_CAPACITY);

        // Disperse 100 rejections across all 5 categories
        for _ in 0..30 {
            tracker.record(RejectionCategory::LowLiquidity);
        }
        for _ in 0..25 {
            tracker.record(RejectionCategory::LowConviction);
        }
        for _ in 0..20 {
            tracker.record(RejectionCategory::HawkesWash);
        }
        for _ in 0..15 {
            tracker.record(RejectionCategory::LowDispersion);
        }
        for _ in 0..10 {
            tracker.record(RejectionCategory::DevCluster);
        }

        let gauge = tracker.snapshot();

        assert_eq!(gauge.total_evaluated, 100);
        assert_eq!(gauge.low_liquidity, 30);
        assert_eq!(gauge.count_low_conviction, 25);
        assert_eq!(gauge.low_conviction, 25);
        assert_eq!(gauge.hawkes_wash, 20);
        assert_eq!(gauge.low_dispersion, 15);
        assert_eq!(gauge.dev_cluster, 10);

        assert_eq!(gauge.pct_low_liquidity, 30.0);
        assert_eq!(gauge.pct_low_conviction, 25.0);
        assert_eq!(gauge.pct_hawkes_wash, 20.0);
        assert_eq!(gauge.pct_low_dispersion, 15.0);
        assert_eq!(gauge.pct_dev_cluster, 10.0);

        let sum_pct = gauge.pct_low_liquidity
            + gauge.pct_low_conviction
            + gauge.pct_hawkes_wash
            + gauge.pct_low_dispersion
            + gauge.pct_dev_cluster;

        assert!((sum_pct - 100.0).abs() <= 0.1, "Percentages must sum to 100.0% within +/-0.1%");

        // Verify no category inflates to 100% when other categories are non-zero
        assert!(gauge.pct_low_liquidity < 100.0);
        assert!(gauge.pct_low_conviction < 100.0);
        assert!(gauge.pct_hawkes_wash < 100.0);
        assert!(gauge.pct_low_dispersion < 100.0);
        assert!(gauge.pct_dev_cluster < 100.0);
    }

    #[test]
    fn test_stage2_ring_buffer_capacity_limit() {
        let mut buffer = Stage2AuditRingBuffer::new(STAGE2_AUDIT_LOG_CAPACITY);
        assert_eq!(buffer.len(), 0);

        for i in 1..=12 {
            buffer.push(Stage2AuditEntry {
                timestamp: format!("12:00:{:02}", i),
                symbol: format!("SYM{}", i),
                mint: format!("mint_{}", i),
                liquidity_usd: 25_000.0 + (i as f64 * 100.0),
                key_metric: format!("HHI: 0.{:02}", i),
                verdict: if i % 2 == 0 { "ADMITTED".to_string() } else { "REJECT: LOW_LIQUIDITY".to_string() },
                is_admitted: i % 2 == 0,
            });
        }

        assert_eq!(buffer.len(), STAGE2_AUDIT_LOG_CAPACITY);
        // Newest entry should be at index 0 (SYM12)
        assert_eq!(buffer.entries[0].symbol, "SYM12");
        // Oldest retained entry should be SYM5 (since 12 down to 5 = 8 entries)
        assert_eq!(buffer.entries[7].symbol, "SYM5");
    }

    #[test]
    fn test_truncate_mint_address() {
        let long_mint = "8nFy4Hw9VAy3NQjwNVVbfNi1HM96VKodNMf5zFt1PhJ8";
        assert_eq!(truncate_mint_address(long_mint), "8nFy...PhJ8");

        let short_mint = "SOL";
        assert_eq!(truncate_mint_address(short_mint), "SOL");
    }

    #[test]
    fn test_rolling_rejection_window_caps_at_1000() {
        let mut tracker = RollingRejectionTracker::new(ROLLING_WINDOW_CAPACITY);
        assert_eq!(tracker.len(), 0);

        for _ in 0..2500 {
            tracker.record(RejectionCategory::LowLiquidity);
        }

        assert_eq!(tracker.len(), ROLLING_WINDOW_CAPACITY);
        let snap = tracker.snapshot();
        assert_eq!(snap.total_evaluated, 1000);
        assert_eq!(snap.low_liquidity, 1000);
        assert_eq!(snap.pct_low_liquidity, 100.0);
    }

    #[test]
    fn test_rolling_rejection_window_evicts_oldest_category() {
        let mut tracker = RollingRejectionTracker::new(ROLLING_WINDOW_CAPACITY);

        // Fill with 1,000 LowLiquidity
        for _ in 0..1000 {
            tracker.record(RejectionCategory::LowLiquidity);
        }
        assert_eq!(tracker.len(), 1000);
        let snap1 = tracker.snapshot();
        assert_eq!(snap1.low_liquidity, 1000);
        assert_eq!(snap1.hawkes_wash, 0);
        assert_eq!(snap1.pct_low_liquidity, 100.0);

        // Push 500 HawkesWash entries (should evict 500 LowLiquidity from the front)
        for _ in 0..500 {
            tracker.record(RejectionCategory::HawkesWash);
        }
        assert_eq!(tracker.len(), 1000);

        let snap2 = tracker.snapshot();
        assert_eq!(snap2.total_evaluated, 1000);
        assert_eq!(snap2.low_liquidity, 500);
        assert_eq!(snap2.hawkes_wash, 500);
        assert_eq!(snap2.pct_low_liquidity, 50.0);
        assert_eq!(snap2.pct_hawkes_wash, 50.0);
        assert_eq!(snap2.pct_low_dispersion, 0.0);
        assert_eq!(snap2.pct_dev_cluster, 0.0);
        assert_eq!(snap2.pct_low_conviction, 0.0);
        assert_eq!(snap2.pct_admitted, 0.0);
    }

    #[test]
    fn test_rejection_gauge_snapshot_dto() {
        let snap = RejectionGaugeSnapshot::new();
        assert_eq!(snap.total_evaluated, 0);
        assert_eq!(snap.low_liquidity, 0);
        assert_eq!(snap.low_conviction, 0);
        assert_eq!(snap.count_low_conviction, 0);
        assert_eq!(snap.pct_low_liquidity, 0.0);
    }

    #[test]
    fn test_concurrent_rolling_rejection_tracker() {
        let tracker = ConcurrentRollingRejectionTracker::default();
        tracker.record(RejectionCategory::LowLiquidity);
        tracker.record(RejectionCategory::LowConviction);
        let snap = tracker.snapshot();
        assert_eq!(snap.total_evaluated, 2);
        assert_eq!(snap.low_liquidity, 1);
        assert_eq!(snap.count_low_conviction, 1);
    }
}

