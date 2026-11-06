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
    pub wallet_keypair_path: String,
    pub min_gas_reserve_sol: f64,
    pub min_liquidity_usd: f64,
    pub max_dev_holding_pct: f64,
    pub max_top10_concentration_pct: f64,
    pub min_symbol_entropy: f64,

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
    pub jito_tip_floor_url: String,
    pub cu_headroom_multiplier: f64,
    pub sim_initial_cu_limit: u32,
    pub min_sim_cu_threshold: u64,
    pub default_rpc_url: String,

    // Institutional Risk Envelope
    pub risk: RiskConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    pub stop_loss_pct: f64,
    #[serde(default = "default_max_slippage_loss_pct")]
    pub max_slippage_loss_pct: f64,
    #[serde(default = "default_flash_dump_tick_pct")]
    pub flash_dump_tick_pct: f64,
    #[serde(default = "default_flash_dump_max_pnl_pct")]
    pub flash_dump_max_pnl_pct: f64,

    pub breakeven_arm_pct: f64,
    pub breakeven_floor_pct: f64,

    pub tp1_trigger_pct: f64,
    pub tp1_sell_fraction: f64,

    pub tp2_trigger_pct: f64,
    pub tp2_sell_fraction: f64,

    pub runner_trailing_drop_pct: f64,
    #[serde(default = "default_cruise_baseline_trail_mult")]
    pub cruise_baseline_trail_mult: f64,
    #[serde(default = "default_min_runner_trailing_drop_pct")]
    pub min_runner_trailing_drop_pct: f64,
    #[serde(default = "default_max_runner_trailing_drop_pct")]
    pub max_runner_trailing_drop_pct: f64,

    pub stagnation_ttl_secs: u64,
    #[serde(default = "default_cruise_baseline_ttl_secs")]
    pub cruise_baseline_ttl_secs: u64,
    #[serde(default = "default_min_stagnation_ttl_secs")]
    pub min_stagnation_ttl_secs: u64,
    #[serde(default = "default_max_stagnation_ttl_secs")]
    pub max_stagnation_ttl_secs: u64,
    pub stagnation_max_peak_pct: f64,
    pub stagnation_band_min_pct: f64,
    pub stagnation_band_max_pct: f64,

    // Anti-Churn Quarantine & Entry Momentum Gates
    #[serde(default = "default_reentry_cooldown_secs")]
    pub reentry_cooldown_secs: u64,
    #[serde(default = "default_toxic_exit_permanent_ban")]
    pub toxic_exit_permanent_ban: bool,
    #[serde(default = "default_min_entry_liquidity_usd")]
    pub min_entry_liquidity_usd: f64,
    #[serde(default = "default_min_entry_price_change_pct")]
    pub min_entry_price_change_pct: f64,
    #[serde(default = "default_max_entry_price_change_pct")]
    pub max_entry_price_change_pct: f64,
    #[serde(default = "default_min_entry_buy_sell_ratio")]
    pub min_entry_buy_sell_ratio: f64,
    #[serde(default = "default_min_entry_m5_txns")]
    pub min_entry_m5_txns: u32,
    #[serde(default = "default_min_entry_m5_volume_usd")]
    pub min_entry_m5_volume_usd: f64,
    #[serde(default = "default_min_entry_h1_price_change_pct")]
    pub min_entry_h1_price_change_pct: f64,
    #[serde(default = "default_max_entry_fdv_to_liquidity_ratio")]
    pub max_entry_fdv_to_liquidity_ratio: f64,
    #[serde(default = "default_kelly_fraction_nav_pct")]
    pub kelly_fraction_nav_pct: f64,
    #[serde(default = "default_max_active_slots")]
    pub max_active_slots: usize,
    #[serde(default = "default_dex_roundtrip_fee_pct")]
    pub dex_roundtrip_fee_pct: f64,
    #[serde(default = "default_jito_tip_lamports")]
    pub jito_tip_lamports: u64,
}

fn default_flash_dump_tick_pct() -> f64 { -15.0 }
fn default_flash_dump_max_pnl_pct() -> f64 { 0.0 }
fn default_max_slippage_loss_pct() -> f64 { -25.0 }
fn default_cruise_baseline_trail_mult() -> f64 { 2.20 }
fn default_min_runner_trailing_drop_pct() -> f64 { 12.0 }
fn default_max_runner_trailing_drop_pct() -> f64 { 25.0 }
fn default_cruise_baseline_ttl_secs() -> u64 { 240 }
fn default_min_stagnation_ttl_secs() -> u64 { 120 }
fn default_max_stagnation_ttl_secs() -> u64 { 360 }
fn default_reentry_cooldown_secs() -> u64 { 3600 }
fn default_toxic_exit_permanent_ban() -> bool { true }
fn default_min_entry_liquidity_usd() -> f64 { 25000.0 }
fn default_min_entry_price_change_pct() -> f64 { 1.5 }
fn default_max_entry_price_change_pct() -> f64 { 85.0 }
fn default_min_entry_buy_sell_ratio() -> f64 { 1.20 }
fn default_min_entry_m5_txns() -> u32 { 15 }
fn default_min_entry_m5_volume_usd() -> f64 { 2500.0 }
fn default_min_entry_h1_price_change_pct() -> f64 { -10.0 }
fn default_max_entry_fdv_to_liquidity_ratio() -> f64 { 15.0 }
fn default_kelly_fraction_nav_pct() -> f64 { 7.5 }
fn default_max_active_slots() -> usize { 4 }
fn default_dex_roundtrip_fee_pct() -> f64 { 0.60 }
fn default_jito_tip_lamports() -> u64 { 10000 }

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            stop_loss_pct: -20.0,
            max_slippage_loss_pct: -25.0,
            flash_dump_tick_pct: -15.0,
            flash_dump_max_pnl_pct: 0.0,
            breakeven_arm_pct: 35.0,
            breakeven_floor_pct: 12.0,
            tp1_trigger_pct: 45.0,
            tp1_sell_fraction: 0.40,
            tp2_trigger_pct: 90.0,
            tp2_sell_fraction: 0.35,
            runner_trailing_drop_pct: 18.0,
            cruise_baseline_trail_mult: 2.20,
            min_runner_trailing_drop_pct: 12.0,
            max_runner_trailing_drop_pct: 25.0,
            stagnation_ttl_secs: 240,
            cruise_baseline_ttl_secs: 240,
            min_stagnation_ttl_secs: 120,
            max_stagnation_ttl_secs: 360,
            stagnation_max_peak_pct: 12.0,
            stagnation_band_min_pct: -8.0,
            stagnation_band_max_pct: 5.0,
            reentry_cooldown_secs: 3600,
            toxic_exit_permanent_ban: true,
            min_entry_liquidity_usd: 25000.0,
            min_entry_price_change_pct: 1.5,
            max_entry_price_change_pct: 85.0,
            min_entry_buy_sell_ratio: 1.20,
            min_entry_m5_txns: 15,
            min_entry_m5_volume_usd: 2500.0,
            min_entry_h1_price_change_pct: -10.0,
            max_entry_fdv_to_liquidity_ratio: 15.0,
            kelly_fraction_nav_pct: 7.5,
            max_active_slots: 4,
            dex_roundtrip_fee_pct: 0.60,
            jito_tip_lamports: 10000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TomlConfigFile {
    pub risk: Option<RiskConfig>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            initial_capital_usd: 100.0,          // $100.00 USD Initial Account Capital
            max_price_impact_ratio: 0.012,       // 1.2% maximum pool price impact
            hhi_concentration_ceiling: 0.280,     // Herfindahl-Hirschman Index limit for top holders
            min_dead_lp_ratio: 0.950,             // 95% burned or locked LP tokens for AMM pools
            wallet_keypair_path: "/home/ubuntu/.config/solana/dmne-live-wallet.json".to_string(),
            min_gas_reserve_sol: 0.0300,       // Never withdraw or trade below this reserve
            min_liquidity_usd: 25000.0,
            max_dev_holding_pct: 1.0,         // Reject if deployer holds >= 1.0%
            max_top10_concentration_pct: 18.0,
            min_symbol_entropy: 1.8,          // Reject repetitive ticker spam
            kelly_nav_fraction: 0.075,            // 7.5% NAV Half-Kelly risk allocation
            min_entry_sol: 0.012,                 // Minimum entry size (covers rent & priority fees)
            max_entry_sol: 0.045,                 // Maximum entry size cap
            sol_reference_price_usd: 180.0,       // Floating SOL USD valuation reference
            rolling_sample_window: 15,            // 15-tick rolling window for volatility calculation
            volatility_trail_multiplier: 2.20,    // Multiplier (k) applied to rolling standard deviation
            z_score_flash_threshold: -3.0,        // Emergency circuit breaker triggered at -3.0 sigma
            breakeven_arm_peak_pct: 35.0,         // Minimum peak gain to arm breakeven floor guard
            breakeven_floor_pct: 12.0,            // Minimum guaranteed profit floor
            stagnation_ttl_seconds: 240,          // Capital recycling window
            hard_stop_loss_pct: -20.0,            // Hard stop loss cutoff
            tranche_1_scale_ratio: 0.40,          // 40% TP1 harvest
            tranche_1_target_pct: 45.0,           // TP1 trigger at +45%
            tranche_2_scale_ratio: 0.35,          // 35% TP2 harvest
            tranche_2_target_pct: 90.0,           // TP2 trigger at +90%
            jito_block_engine_url: "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/bundles".to_string(),
            jito_tip_floor_url: "https://bundles.jito.wtf/api/v1/bundles/tip_floor".to_string(),
            cu_headroom_multiplier: 1.15,        // 15% CU buffer over unitsConsumed
            sim_initial_cu_limit: 200_000,       // Initial simulation envelope
            min_sim_cu_threshold: 15_000,        // Rejection floor for no-op transactions
            default_rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            risk: RiskConfig::default(),
        }
    }
}

impl EngineConfig {
    pub fn load() -> Self {
        let mut cfg = Self::default();
        let path = "/home/ubuntu/dmne-meme-alpha/config.toml";
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(toml_file) = toml::from_str::<TomlConfigFile>(&content) {
                if let Some(r) = toml_file.risk {
                    println!("[CONFIG] Loaded institutional [risk] envelope from config.toml");
                    cfg.breakeven_arm_peak_pct = r.breakeven_arm_pct;
                    cfg.breakeven_floor_pct = r.breakeven_floor_pct;
                    cfg.hard_stop_loss_pct = r.stop_loss_pct;
                    cfg.tranche_1_target_pct = r.tp1_trigger_pct;
                    cfg.tranche_1_scale_ratio = r.tp1_sell_fraction;
                    cfg.tranche_2_target_pct = r.tp2_trigger_pct;
                    cfg.tranche_2_scale_ratio = r.tp2_sell_fraction;
                    cfg.stagnation_ttl_seconds = r.stagnation_ttl_secs as i64;
                    cfg.risk = r;
                }
            }
        }
        cfg
    }

    pub fn calculate_initial_bankroll_sol(&self) -> f64 {
        if self.sol_reference_price_usd > 0.0 {
            self.initial_capital_usd / self.sol_reference_price_usd
        } else {
            0.5556
        }
    }
}
