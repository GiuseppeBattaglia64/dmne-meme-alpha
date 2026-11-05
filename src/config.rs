// DMNE QUANT // CONFIGURATION SUBSYSTEM
// Elimination of Magic Numbers via Typed Institutional Config Schema

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    // Dynamic Pre-Trade Gate Constraints
    pub initial_capital_usd: f64,
    pub max_price_impact_ratio: f64,
    pub hhi_concentration_ceiling: f64,
    pub min_dead_lp_ratio: f64,
    pub min_liquidity_usd: f64,

    // Dynamic Sizing Engine
    pub kelly_nav_fraction: f64,
    pub min_entry_sol: f64,
    pub max_entry_sol: f64,
    pub sol_reference_price_usd: f64,

    // Dynamic In-Flight Cruise Control Supervisor
    pub rolling_sample_window: usize,
    pub volatility_trail_multiplier: f64,
    pub z_score_flash_threshold: f64,
    pub breakeven_arm_peak_pct: f64,
    pub breakeven_floor_pct: f64,
    pub stagnation_ttl_seconds: i64,
    pub hard_stop_loss_pct: f64,

    // Asymmetric Multi-Tranche Exits
    pub tranche_1_scale_ratio: f64,
    pub tranche_1_target_pct: f64,
    pub tranche_2_scale_ratio: f64,
    pub tranche_2_target_pct: f64,

    // Endpoint Discovery
    pub jito_block_engine_url: String,
    pub default_rpc_url: String,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            initial_capital_usd: 100.0,          // $100.00 USD Initial Account Capital
            max_price_impact_ratio: 0.012,       // 1.2% maximum pool price impact
            hhi_concentration_ceiling: 0.280,     // Herfindahl-Hirschman Index limit for top holders
            min_dead_lp_ratio: 0.950,             // 95% burned or locked LP tokens for AMM pools
            min_liquidity_usd: 8000.0,            // Dynamic fallback liquidity threshold
            kelly_nav_fraction: 0.075,            // 7.5% NAV Half-Kelly risk allocation
            min_entry_sol: 0.012,                 // Minimum entry size (covers rent & priority fees)
            max_entry_sol: 0.045,                 // Maximum entry size cap
            sol_reference_price_usd: 180.0,       // Floating SOL USD valuation reference
            rolling_sample_window: 15,            // 15-tick rolling window for volatility calculation
            volatility_trail_multiplier: 2.20,    // Multiplier (k) applied to rolling standard deviation
            z_score_flash_threshold: -3.0,        // Emergency circuit breaker triggered at -3.0 sigma
            breakeven_arm_peak_pct: 22.0,         // Minimum peak gain to arm breakeven floor guard
            breakeven_floor_pct: 3.0,             // Minimum guaranteed profit floor
            stagnation_ttl_seconds: 900,          // 15-minute capital recycling window
            hard_stop_loss_pct: -30.0,            // Hard stop loss cutoff
            tranche_1_scale_ratio: 0.50,          // 50% de-risking tranche
            tranche_1_target_pct: 100.0,          // De-risk at +100% (2x)
            tranche_2_scale_ratio: 0.25,          // 25% profit harvest tranche
            tranche_2_target_pct: 250.0,          // Harvest at +250% (3.5x)
            jito_block_engine_url: "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/bundles".to_string(),
            default_rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
        }
    }
}

impl EngineConfig {
    pub fn calculate_initial_bankroll_sol(&self) -> f64 {
        if self.sol_reference_price_usd > 0.0 {
            self.initial_capital_usd / self.sol_reference_price_usd
        } else {
            0.5556
        }
    }
}
