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

use serde::{Deserialize, Serialize};

/// Maximum fixed capacity of the Stage 2 Deep Audit Log ring buffer
pub const STAGE2_AUDIT_LOG_CAPACITY: usize = 8;

/// Rejection classification categories for the Pre-Trade Rejection Gauge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RejectionCategory {
    /// Liquidity < minimum parameter threshold
    LowLiquidity,
    /// Hawkes process self-excitation supercritical or cyclic periodicity
    HawkesWash,
    /// Shannon entropy or order flow dispersion below cutoff
    LowDispersion,
    /// Dev cluster or holder HHI concentration ceiling exceeded
    DevCluster,
    /// Successfully passed all pre-flight checks and admitted to candidate queue
    Admitted,
    /// Other pre-flight check failures (e.g. momentum overextension, confirmation drop)
    Other,
}

impl RejectionCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            RejectionCategory::LowLiquidity => "LOW_LIQUIDITY",
            RejectionCategory::HawkesWash => "HAWKES_WASH",
            RejectionCategory::LowDispersion => "LOW_DISPERSION",
            RejectionCategory::DevCluster => "DEV_CLUSTER",
            RejectionCategory::Admitted => "ADMITTED",
            RejectionCategory::Other => "OTHER",
        }
    }
}

/// Telemetry tracking rolling pre-trade rejection vectors and relative distributions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RejectionGaugeTelemetry {
    pub low_liquidity: u64,
    pub hawkes_wash: u64,
    pub low_dispersion: u64,
    pub dev_cluster: u64,
    pub admitted: u64,
    pub other: u64,
    pub total_evaluated: u64,
    pub pct_low_liquidity: f64,
    pub pct_hawkes_wash: f64,
    pub pct_low_dispersion: f64,
    pub pct_dev_cluster: f64,
}

impl Default for RejectionGaugeTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl RejectionGaugeTelemetry {
    pub fn new() -> Self {
        Self {
            low_liquidity: 0,
            hawkes_wash: 0,
            low_dispersion: 0,
            dev_cluster: 0,
            admitted: 0,
            other: 0,
            total_evaluated: 0,
            pct_low_liquidity: 0.0,
            pct_hawkes_wash: 0.0,
            pct_low_dispersion: 0.0,
            pct_dev_cluster: 0.0,
        }
    }

    /// Records an evaluation decision and re-calculates normalized percentages
    pub fn record(&mut self, category: RejectionCategory) {
        self.total_evaluated += 1;
        match category {
            RejectionCategory::LowLiquidity => self.low_liquidity += 1,
            RejectionCategory::HawkesWash => self.hawkes_wash += 1,
            RejectionCategory::LowDispersion => self.low_dispersion += 1,
            RejectionCategory::DevCluster => self.dev_cluster += 1,
            RejectionCategory::Admitted => self.admitted += 1,
            RejectionCategory::Other => self.other += 1,
        }
        self.recompute_percentages();
    }

    /// Re-calculates percentages across the 4 primary rejection vectors
    pub fn recompute_percentages(&mut self) {
        let total_rejections = (self.low_liquidity + self.hawkes_wash + self.low_dispersion + self.dev_cluster) as f64;
        if total_rejections > 0.0 {
            self.pct_low_liquidity = ((self.low_liquidity as f64 / total_rejections) * 100.0 * 10.0).round() / 10.0;
            self.pct_hawkes_wash = ((self.hawkes_wash as f64 / total_rejections) * 100.0 * 10.0).round() / 10.0;
            self.pct_low_dispersion = ((self.low_dispersion as f64 / total_rejections) * 100.0 * 10.0).round() / 10.0;
            self.pct_dev_cluster = ((self.dev_cluster as f64 / total_rejections) * 100.0 * 10.0).round() / 10.0;
        } else {
            self.pct_low_liquidity = 0.0;
            self.pct_hawkes_wash = 0.0;
            self.pct_low_dispersion = 0.0;
            self.pct_dev_cluster = 0.0;
        }
    }
}

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
        let mut gauge = RejectionGaugeTelemetry::new();
        assert_eq!(gauge.total_evaluated, 0);

        // Record 60 low liquidity, 20 hawkes wash, 10 low dispersion, 10 dev cluster, 5 admitted
        for _ in 0..60 {
            gauge.record(RejectionCategory::LowLiquidity);
        }
        for _ in 0..20 {
            gauge.record(RejectionCategory::HawkesWash);
        }
        for _ in 0..10 {
            gauge.record(RejectionCategory::LowDispersion);
        }
        for _ in 0..10 {
            gauge.record(RejectionCategory::DevCluster);
        }
        for _ in 0..5 {
            gauge.record(RejectionCategory::Admitted);
        }

        assert_eq!(gauge.total_evaluated, 105);
        assert_eq!(gauge.low_liquidity, 60);
        assert_eq!(gauge.hawkes_wash, 20);
        assert_eq!(gauge.low_dispersion, 10);
        assert_eq!(gauge.dev_cluster, 10);
        assert_eq!(gauge.admitted, 5);

        // Normalized over 100 total rejections
        assert_eq!(gauge.pct_low_liquidity, 60.0);
        assert_eq!(gauge.pct_hawkes_wash, 20.0);
        assert_eq!(gauge.pct_low_dispersion, 10.0);
        assert_eq!(gauge.pct_dev_cluster, 10.0);
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
}

