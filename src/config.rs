// DMNE QUANT // CONFIGURATION SUBSYSTEM
// Elimination of Magic Numbers via Typed Institutional Config Schema

use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
pub use crate::hawkes::{default_hawkes_min_sample_size, HawkesConfig};

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

    // Point Process Wash-Trading Filter
    pub hawkes: crate::hawkes::HawkesConfig,

    // Volatility-Scaled Dynamic Exit Bands
    pub dynamic_exits: DynamicExitsConfig,

    // Real-Time Cumulative Volume Delta (CVD) & Orderflow Pressure
    pub orderflow: OrderflowConfig,

    // Slot 0 Genesis Bundle Protection & Anti-Sniper Cluster Detection
    pub genesis_guard: GenesisGuardConfig,

    // Deep Funder Graph HHI / Sybil Cluster Filter
    pub funder_graph: crate::funder_graph::FunderGraphConfig,

    // Sandbox & Replay Configuration
    pub sandbox: SandboxConfig,

    // Admin Authorization Config
    pub admin: AdminConfig,
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
    #[serde(default = "default_min_entry_conviction_pct")]
    pub min_entry_conviction_pct: f64,
    #[serde(default = "default_min_entry_m5_txns")]
    pub min_entry_m5_txns: u32,
    #[serde(default = "default_min_entry_m5_volume_usd")]
    pub min_entry_m5_volume_usd: f64,
    #[serde(default = "default_min_entry_h1_price_change_pct")]
    pub min_entry_h1_price_change_pct: f64,
    #[serde(default = "default_max_entry_fdv_to_liquidity_ratio")]
    pub max_entry_fdv_to_liquidity_ratio: f64,
    #[serde(default = "default_early_abort_window_secs")]
    pub early_abort_window_secs: u64,
    #[serde(default = "default_early_abort_max_peak_pct")]
    pub early_abort_max_peak_pct: f64,
    #[serde(default = "default_early_abort_loss_pct")]
    pub early_abort_loss_pct: f64,
    #[serde(default = "default_entry_confirm_ticks")]
    pub entry_confirm_ticks: u32,
    #[serde(default = "default_entry_confirm_interval_secs")]
    pub entry_confirm_interval_secs: u64,
    #[serde(default = "default_min_confirm_tick_gain_pct")]
    pub min_confirm_tick_gain_pct: f64,
    #[serde(default = "default_max_confirm_tick_gain_pct")]
    pub max_confirm_tick_gain_pct: f64,
    #[serde(default = "default_min_confirm_liquidity_retention_ratio")]
    pub min_confirm_liquidity_retention_ratio: f64,
    #[serde(default = "default_rejected_candidate_cooldown_secs")]
    pub rejected_candidate_cooldown_secs: u64,
    #[serde(default = "default_kelly_fraction_nav_pct")]
    pub kelly_fraction_nav_pct: f64,
    #[serde(default = "default_min_kelly_fraction_nav_pct")]
    pub min_kelly_fraction_nav_pct: f64,
    #[serde(default = "default_base_kelly_fraction_nav_pct")]
    pub base_kelly_fraction_nav_pct: f64,
    #[serde(default = "default_max_kelly_fraction_nav_pct")]
    pub max_kelly_fraction_nav_pct: f64,
    #[serde(default = "default_high_conviction_buy_ratio_threshold")]
    pub high_conviction_buy_ratio_threshold: f64,
    #[serde(default = "default_max_active_slots")]
    pub max_active_slots: usize,
    #[serde(default = "default_dex_roundtrip_fee_pct")]
    pub dex_roundtrip_fee_pct: f64,
    #[serde(default = "default_jito_tip_lamports")]
    pub jito_tip_lamports: u64,
    #[serde(default = "default_emergency_jito_tip_lamports")]
    pub emergency_jito_tip_lamports: u64,
    #[serde(default = "default_min_entry_sol")]
    pub min_entry_sol: f64,
    #[serde(default = "default_max_entry_sol")]
    pub max_entry_sol: f64,
    #[serde(default = "default_min_gas_reserve_sol")]
    pub min_gas_reserve_sol: f64,
    #[serde(default = "default_base_position_size_sol")]
    pub base_position_size_sol: f64,
    #[serde(default = "default_base_position_pct_nav")]
    pub base_position_pct_nav: f64,
    #[serde(default = "default_max_position_pct_nav")]
    pub max_position_pct_nav: f64,
    #[serde(default = "default_admin_passphrase")]
    pub admin_passphrase: Option<String>,
    #[serde(default)]
    pub dynamic_exits: DynamicExitsConfig,
    #[serde(default)]
    pub orderflow: OrderflowConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    #[serde(default = "default_admin_passphrase")]
    pub admin_passphrase: Option<String>,
}

pub fn default_admin_passphrase() -> Option<String> {
    Some("dmne-alpha-desk-2026".to_string())
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            admin_passphrase: default_admin_passphrase(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicExitsConfig {
    #[serde(default = "default_dynamic_exits_enabled")]
    pub enabled: bool,
    #[serde(default = "default_vol_multiplier")]
    pub vol_multiplier: f64,
    #[serde(default = "default_min_trail_pct")]
    pub min_trail_pct: f64,
    #[serde(default = "default_max_trail_pct")]
    pub max_trail_pct: f64,
    #[serde(default = "default_mfe_activation_pct")]
    pub mfe_activation_pct: f64,
    #[serde(default = "default_min_locked_floor_pct")]
    pub min_locked_floor_pct: f64,
    #[serde(default = "default_vol_window_size")]
    pub vol_window_size: usize,
}

fn default_dynamic_exits_enabled() -> bool { true }
fn default_vol_multiplier() -> f64 { 1.65 }
fn default_min_trail_pct() -> f64 { 8.0 }
fn default_max_trail_pct() -> f64 { 22.0 }
fn default_mfe_activation_pct() -> f64 { 30.0 }
fn default_min_locked_floor_pct() -> f64 { 12.0 }
fn default_vol_window_size() -> usize { 30 }

impl Default for DynamicExitsConfig {
    fn default() -> Self {
        Self {
            enabled: default_dynamic_exits_enabled(),
            vol_multiplier: default_vol_multiplier(),
            min_trail_pct: default_min_trail_pct(),
            max_trail_pct: default_max_trail_pct(),
            mfe_activation_pct: default_mfe_activation_pct(),
            min_locked_floor_pct: default_min_locked_floor_pct(),
            vol_window_size: default_vol_window_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderflowConfig {
    #[serde(default = "default_orderflow_enabled")]
    pub enabled: bool,
    #[serde(default = "default_cvd_window_secs")]
    pub cvd_window_secs: f64,
    #[serde(default = "default_min_entry_cvd_ratio")]
    pub min_entry_cvd_ratio: f64,
    #[serde(default = "default_bearish_absorption_threshold")]
    pub bearish_absorption_threshold: f64,
    #[serde(default = "default_tighten_exit_on_absorption")]
    pub tighten_exit_on_absorption: bool,
}

fn default_orderflow_enabled() -> bool { true }
fn default_cvd_window_secs() -> f64 { 60.0 }
fn default_min_entry_cvd_ratio() -> f64 { 0.15 }
fn default_bearish_absorption_threshold() -> f64 { -0.30 }
fn default_tighten_exit_on_absorption() -> bool { true }

impl Default for OrderflowConfig {
    fn default() -> Self {
        Self {
            enabled: default_orderflow_enabled(),
            cvd_window_secs: default_cvd_window_secs(),
            min_entry_cvd_ratio: default_min_entry_cvd_ratio(),
            bearish_absorption_threshold: default_bearish_absorption_threshold(),
            tighten_exit_on_absorption: default_tighten_exit_on_absorption(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisGuardConfig {
    #[serde(default = "default_genesis_guard_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_genesis_supply_pct")]
    pub max_genesis_supply_pct: f64,
    #[serde(default = "default_max_slot0_bundled_txs")]
    pub max_slot0_bundled_txs: u32,
    #[serde(default = "default_reject_shared_tip_accounts")]
    pub reject_shared_tip_accounts: bool,
}

fn default_genesis_guard_enabled() -> bool { true }
fn default_max_genesis_supply_pct() -> f64 { 25.0 }
fn default_max_slot0_bundled_txs() -> u32 { 3 }
fn default_reject_shared_tip_accounts() -> bool { true }

impl Default for GenesisGuardConfig {
    fn default() -> Self {
        Self {
            enabled: default_genesis_guard_enabled(),
            max_genesis_supply_pct: default_max_genesis_supply_pct(),
            max_slot0_bundled_txs: default_max_slot0_bundled_txs(),
            reject_shared_tip_accounts: default_reject_shared_tip_accounts(),
        }
    }
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
fn default_min_entry_liquidity_usd() -> f64 { 20000.0 }
fn default_min_entry_price_change_pct() -> f64 { 1.5 }
fn default_max_entry_price_change_pct() -> f64 { 85.0 }
fn default_min_entry_buy_sell_ratio() -> f64 { 1.25 }
fn default_min_entry_conviction_pct() -> f64 { 55.0 }
fn default_min_entry_m5_txns() -> u32 { 15 }
fn default_min_entry_m5_volume_usd() -> f64 { 2500.0 }
fn default_min_entry_h1_price_change_pct() -> f64 { -10.0 }
fn default_max_entry_fdv_to_liquidity_ratio() -> f64 { 15.0 }
fn default_early_abort_window_secs() -> u64 { 30 }
fn default_early_abort_max_peak_pct() -> f64 { 1.0 }
fn default_early_abort_loss_pct() -> f64 { -10.0 }
fn default_entry_confirm_ticks() -> u32 { 2 }
fn default_entry_confirm_interval_secs() -> u64 { 6 }
fn default_min_confirm_tick_gain_pct() -> f64 { 0.0 }
fn default_max_confirm_tick_gain_pct() -> f64 { 25.0 }
fn default_min_confirm_liquidity_retention_ratio() -> f64 { 0.98 }
fn default_rejected_candidate_cooldown_secs() -> u64 { 300 }
fn default_kelly_fraction_nav_pct() -> f64 { 7.5 }
fn default_min_kelly_fraction_nav_pct() -> f64 { 4.5 }
fn default_base_kelly_fraction_nav_pct() -> f64 { 6.5 }
fn default_max_kelly_fraction_nav_pct() -> f64 { 8.5 }
fn default_high_conviction_buy_ratio_threshold() -> f64 { 2.0 }
fn default_max_active_slots() -> usize { 4 }
fn default_dex_roundtrip_fee_pct() -> f64 { 0.60 }
fn default_jito_tip_lamports() -> u64 { 10000 }
fn default_emergency_jito_tip_lamports() -> u64 { 75000 }
fn default_min_entry_sol() -> f64 { 0.010 }
fn default_max_entry_sol() -> f64 { 0.018 }
fn default_min_gas_reserve_sol() -> f64 { 0.030 }
fn default_base_position_size_sol() -> f64 { 0.018 }
fn default_base_position_pct_nav() -> f64 { 0.025 }
fn default_max_position_pct_nav() -> f64 { 0.030 }

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            stop_loss_pct: -20.0,
            max_slippage_loss_pct: -25.0,
            flash_dump_tick_pct: -15.0,
            flash_dump_max_pnl_pct: 0.0,
            breakeven_arm_pct: 24.0,
            breakeven_floor_pct: 8.0,
            tp1_trigger_pct: 45.0,
            tp1_sell_fraction: 0.40,
            tp2_trigger_pct: 90.0,
            tp2_sell_fraction: 0.35,
            runner_trailing_drop_pct: 18.0,
            cruise_baseline_trail_mult: 2.20,
            min_runner_trailing_drop_pct: 12.0,
            max_runner_trailing_drop_pct: 25.0,
            stagnation_ttl_secs: 160,
            cruise_baseline_ttl_secs: 240,
            min_stagnation_ttl_secs: 120,
            max_stagnation_ttl_secs: 360,
            stagnation_max_peak_pct: 12.0,
            stagnation_band_min_pct: -8.0,
            stagnation_band_max_pct: 5.0,
            reentry_cooldown_secs: 3600,
            toxic_exit_permanent_ban: true,
            min_entry_liquidity_usd: 20000.0,
            min_entry_price_change_pct: 1.5,
            max_entry_price_change_pct: 85.0,
            min_entry_buy_sell_ratio: 1.25,
            min_entry_conviction_pct: 55.0,
            min_entry_m5_txns: 15,
            min_entry_m5_volume_usd: 2500.0,
            min_entry_h1_price_change_pct: -10.0,
            max_entry_fdv_to_liquidity_ratio: 15.0,
            early_abort_window_secs: 30,
            early_abort_max_peak_pct: 1.0,
            early_abort_loss_pct: -10.0,
            entry_confirm_ticks: 2,
            entry_confirm_interval_secs: 6,
            min_confirm_tick_gain_pct: 0.0,
            max_confirm_tick_gain_pct: 25.0,
            min_confirm_liquidity_retention_ratio: 0.98,
            rejected_candidate_cooldown_secs: 300,
            kelly_fraction_nav_pct: 7.5,
            min_kelly_fraction_nav_pct: 4.5,
            base_kelly_fraction_nav_pct: 6.5,
            max_kelly_fraction_nav_pct: 8.5,
            high_conviction_buy_ratio_threshold: 2.0,
            max_active_slots: 4,
            dex_roundtrip_fee_pct: 0.60,
            jito_tip_lamports: 10000,
            emergency_jito_tip_lamports: 75000,
            min_entry_sol: 0.010,
            max_entry_sol: default_max_entry_sol(),
            min_gas_reserve_sol: 0.030,
            base_position_size_sol: default_base_position_size_sol(),
            base_position_pct_nav: default_base_position_pct_nav(),
            max_position_pct_nav: default_max_position_pct_nav(),
            admin_passphrase: default_admin_passphrase(),
            dynamic_exits: DynamicExitsConfig::default(),
            orderflow: OrderflowConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    #[serde(default = "default_hawkes_mu_baseline")]
    pub hawkes_mu_baseline: f64,
    #[serde(default = "default_hawkes_alpha_jump")]
    pub hawkes_alpha_jump: f64,
    #[serde(default = "default_hawkes_beta_decay")]
    pub hawkes_beta_decay: f64,
    #[serde(default = "default_hawkes_min_branching_ratio")]
    pub hawkes_min_branching_ratio: f64,
    #[serde(default = "default_hawkes_max_branching_ratio")]
    pub hawkes_max_branching_ratio: f64,

    #[serde(default = "default_max_slot0_bundle_supply_ratio")]
    pub max_slot0_bundle_supply_ratio: f64,
    #[serde(default = "default_min_holder_shannon_entropy")]
    pub min_holder_shannon_entropy: f64,
    #[serde(default = "default_max_shared_funder_cluster_ratio")]
    pub max_shared_funder_cluster_ratio: f64,

    #[serde(default = "default_vpin_bucket_size_sol")]
    pub vpin_bucket_size_sol: f64,
    #[serde(default = "default_vpin_max_entry_toxicity")]
    pub vpin_max_entry_toxicity: f64,
    #[serde(default = "default_vpin_emergency_exit_toxicity")]
    pub vpin_emergency_exit_toxicity: f64,

    #[serde(default = "default_bayes_prior_alpha")]
    pub bayes_prior_alpha: f64,
    #[serde(default = "default_bayes_prior_beta")]
    pub bayes_prior_beta: f64,
    #[serde(default = "default_bayes_min_wallet_score")]
    pub bayes_min_wallet_score: f64,

    #[serde(default = "default_graduation_target_fdv_usd")]
    pub graduation_target_fdv_usd: f64,
    #[serde(default = "default_graduation_danger_zone_min_pct")]
    pub graduation_danger_zone_min_pct: f64,
    #[serde(default = "default_graduation_danger_zone_max_pct")]
    pub graduation_danger_zone_max_pct: f64,
    #[serde(default = "default_max_amm_price_impact_pct")]
    pub max_amm_price_impact_pct: f64,

    #[serde(default = "default_rpc_backfill_timeout_ms")]
    pub rpc_backfill_timeout_ms: u64,
    #[serde(default = "default_cache_path")]
    pub cache_path: String,
    #[serde(default = "default_report_path")]
    pub report_path: String,
}

fn default_hawkes_mu_baseline() -> f64 { 0.05 }
fn default_hawkes_alpha_jump() -> f64 { 0.45 }
fn default_hawkes_beta_decay() -> f64 { 0.60 }
fn default_hawkes_min_branching_ratio() -> f64 { 0.55 }
fn default_hawkes_max_branching_ratio() -> f64 { 0.98 }

fn default_max_slot0_bundle_supply_ratio() -> f64 { 0.35 }
fn default_min_holder_shannon_entropy() -> f64 { 1.25 }
fn default_max_shared_funder_cluster_ratio() -> f64 { 0.30 }

fn default_vpin_bucket_size_sol() -> f64 { 10.0 }
fn default_vpin_max_entry_toxicity() -> f64 { 0.62 }
fn default_vpin_emergency_exit_toxicity() -> f64 { 0.78 }

fn default_bayes_prior_alpha() -> f64 { 2.0 }
fn default_bayes_prior_beta() -> f64 { 3.0 }
fn default_bayes_min_wallet_score() -> f64 { 0.42 }

fn default_graduation_target_fdv_usd() -> f64 { 69000.0 }
fn default_graduation_danger_zone_min_pct() -> f64 { 88.0 }
fn default_graduation_danger_zone_max_pct() -> f64 { 102.0 }
fn default_max_amm_price_impact_pct() -> f64 { 2.5 }

fn default_rpc_backfill_timeout_ms() -> u64 { 2500 }
fn default_cache_path() -> String { "logs/sandbox_onchain_cache.json".to_string() }
fn default_report_path() -> String { "logs/sandbox_ablation_report.csv".to_string() }

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            hawkes_mu_baseline: default_hawkes_mu_baseline(),
            hawkes_alpha_jump: default_hawkes_alpha_jump(),
            hawkes_beta_decay: default_hawkes_beta_decay(),
            hawkes_min_branching_ratio: default_hawkes_min_branching_ratio(),
            hawkes_max_branching_ratio: default_hawkes_max_branching_ratio(),
            max_slot0_bundle_supply_ratio: default_max_slot0_bundle_supply_ratio(),
            min_holder_shannon_entropy: default_min_holder_shannon_entropy(),
            max_shared_funder_cluster_ratio: default_max_shared_funder_cluster_ratio(),
            vpin_bucket_size_sol: default_vpin_bucket_size_sol(),
            vpin_max_entry_toxicity: default_vpin_max_entry_toxicity(),
            vpin_emergency_exit_toxicity: default_vpin_emergency_exit_toxicity(),
            bayes_prior_alpha: default_bayes_prior_alpha(),
            bayes_prior_beta: default_bayes_prior_beta(),
            bayes_min_wallet_score: default_bayes_min_wallet_score(),
            graduation_target_fdv_usd: default_graduation_target_fdv_usd(),
            graduation_danger_zone_min_pct: default_graduation_danger_zone_min_pct(),
            graduation_danger_zone_max_pct: default_graduation_danger_zone_max_pct(),
            max_amm_price_impact_pct: default_max_amm_price_impact_pct(),
            rpc_backfill_timeout_ms: default_rpc_backfill_timeout_ms(),
            cache_path: default_cache_path(),
            report_path: default_report_path(),
        }
    }
}

#[allow(dead_code)]
impl SandboxConfig {
    pub fn load() -> Self {
        let path = "/home/ubuntu/dmne-meme-alpha/config.toml";
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(toml_file) = toml::from_str::<TomlConfigFile>(&content) {
                if let Some(sb) = toml_file.sandbox {
                    return sb;
                }
            }
        }
        Self::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlConfigFile {
    pub risk: Option<RiskConfig>,
    pub sandbox: Option<SandboxConfig>,
    pub hawkes: Option<crate::hawkes::HawkesConfig>,
    pub dynamic_exits: Option<DynamicExitsConfig>,
    pub orderflow: Option<OrderflowConfig>,
    pub genesis_guard: Option<GenesisGuardConfig>,
    pub funder_graph: Option<crate::funder_graph::FunderGraphConfig>,
    pub admin: Option<AdminConfig>,
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
            min_entry_sol: 0.010,                 // Minimum entry size (covers rent & priority fees)
            max_entry_sol: 0.018,                 // Maximum entry size cap
            sol_reference_price_usd: 180.0,       // Floating SOL USD valuation reference
            rolling_sample_window: 15,            // 15-tick rolling window for volatility calculation
            volatility_trail_multiplier: 2.20,    // Multiplier (k) applied to rolling standard deviation
            z_score_flash_threshold: -3.0,        // Emergency circuit breaker triggered at -3.0 sigma
            breakeven_arm_peak_pct: 24.0,         // Minimum peak gain to arm breakeven floor guard
            breakeven_floor_pct: 8.0,            // Minimum guaranteed profit floor
            stagnation_ttl_seconds: 160,          // Capital recycling window
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
            hawkes: crate::hawkes::HawkesConfig::default(),
            dynamic_exits: DynamicExitsConfig::default(),
            orderflow: OrderflowConfig::default(),
            genesis_guard: GenesisGuardConfig::default(),
            funder_graph: crate::funder_graph::FunderGraphConfig::default(),
            sandbox: SandboxConfig::default(),
            admin: AdminConfig::default(),
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
                    if let Some(ref pass) = r.admin_passphrase {
                        cfg.admin.admin_passphrase = Some(pass.clone());
                    }
                    cfg.risk = r;
                }
                if let Some(h) = toml_file.hawkes {
                    println!("[CONFIG] Loaded institutional [hawkes] envelope from config.toml");
                    cfg.hawkes = h;
                }
                if let Some(de) = toml_file.dynamic_exits {
                    println!("[CONFIG] Loaded institutional [dynamic_exits] envelope from config.toml");
                    cfg.risk.dynamic_exits = de.clone();
                    cfg.dynamic_exits = de;
                }
                if let Some(of) = toml_file.orderflow {
                    println!("[CONFIG] Loaded institutional [orderflow] envelope from config.toml");
                    cfg.risk.orderflow = of.clone();
                    cfg.orderflow = of;
                }
                if let Some(gg) = toml_file.genesis_guard {
                    println!("[CONFIG] Loaded institutional [genesis_guard] envelope from config.toml");
                    cfg.genesis_guard = gg;
                }
                if let Some(fg) = toml_file.funder_graph {
                    println!("[CONFIG] Loaded institutional [funder_graph] envelope from config.toml");
                    cfg.funder_graph = fg;
                }
                if let Some(sb) = toml_file.sandbox {
                    cfg.sandbox = sb;
                }
                if let Some(adm) = toml_file.admin {
                    println!("[CONFIG] Loaded institutional [admin] envelope from config.toml");
                    cfg.admin = adm;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_config_defaults() {
        let cfg = RiskConfig::default();
        assert_eq!(cfg.emergency_jito_tip_lamports, 75_000);
        assert_eq!(cfg.min_kelly_fraction_nav_pct, 4.5);
        assert_eq!(cfg.base_kelly_fraction_nav_pct, 6.5);
        assert_eq!(cfg.max_kelly_fraction_nav_pct, 8.5);
        assert_eq!(cfg.high_conviction_buy_ratio_threshold, 2.0);
        assert_eq!(cfg.jito_tip_lamports, 10_000);
        assert_eq!(cfg.min_entry_sol, 0.010);
        assert_eq!(cfg.max_entry_sol, 0.018);
        assert_eq!(cfg.min_gas_reserve_sol, 0.030);
        assert_eq!(cfg.base_position_pct_nav, 0.025);
        assert_eq!(cfg.max_position_pct_nav, 0.030);
    }

    #[test]
    fn test_risk_config_deserialization_defaults() {
        let toml_str = r#"
            stop_loss_pct = -20.0
            breakeven_arm_pct = 24.0
            breakeven_floor_pct = 8.0
            tp1_trigger_pct = 45.0
            tp1_sell_fraction = 0.40
            tp2_trigger_pct = 90.0
            tp2_sell_fraction = 0.35
            runner_trailing_drop_pct = 18.0
            stagnation_ttl_secs = 160
            stagnation_max_peak_pct = 12.0
            stagnation_band_min_pct = -8.0
            stagnation_band_max_pct = 5.0
        "#;
        let cfg: RiskConfig = toml::from_str(toml_str).expect("Failed to deserialize minimal toml");
        assert_eq!(cfg.emergency_jito_tip_lamports, 75_000);
        assert_eq!(cfg.min_kelly_fraction_nav_pct, 4.5);
        assert_eq!(cfg.base_kelly_fraction_nav_pct, 6.5);
        assert_eq!(cfg.max_kelly_fraction_nav_pct, 8.5);
        assert_eq!(cfg.high_conviction_buy_ratio_threshold, 2.0);
        assert_eq!(cfg.min_entry_sol, 0.010);
        assert_eq!(cfg.max_entry_sol, 0.018);
        assert_eq!(cfg.min_gas_reserve_sol, 0.030);
        assert_eq!(cfg.base_position_pct_nav, 0.025);
        assert_eq!(cfg.max_position_pct_nav, 0.030);
    }

    #[test]
    fn test_orderflow_config_defaults_and_deserialization() {
        let default_cfg = OrderflowConfig::default();
        assert!(default_cfg.enabled);
        assert_eq!(default_cfg.cvd_window_secs, 60.0);
        assert_eq!(default_cfg.min_entry_cvd_ratio, 0.15);
        assert_eq!(default_cfg.bearish_absorption_threshold, -0.30);
        assert!(default_cfg.tighten_exit_on_absorption);

        let toml_str = r#"
            enabled = true
            cvd_window_secs = 45.0
            min_entry_cvd_ratio = 0.20
            bearish_absorption_threshold = -0.35
            tighten_exit_on_absorption = false
        "#;
        let parsed: OrderflowConfig = toml::from_str(toml_str).expect("Failed to deserialize OrderflowConfig");
        assert!(parsed.enabled);
        assert_eq!(parsed.cvd_window_secs, 45.0);
        assert_eq!(parsed.min_entry_cvd_ratio, 0.20);
        assert_eq!(parsed.bearish_absorption_threshold, -0.35);
        assert!(!parsed.tighten_exit_on_absorption);
    }

    #[test]
    fn test_genesis_guard_config_defaults_and_deserialization() {
        let default_cfg = GenesisGuardConfig::default();
        assert!(default_cfg.enabled);
        assert_eq!(default_cfg.max_genesis_supply_pct, 25.0);
        assert_eq!(default_cfg.max_slot0_bundled_txs, 3);
        assert!(default_cfg.reject_shared_tip_accounts);

        let toml_str = r#"
            enabled = true
            max_genesis_supply_pct = 20.0
            max_slot0_bundled_txs = 2
            reject_shared_tip_accounts = false
        "#;
        let parsed: GenesisGuardConfig = toml::from_str(toml_str).expect("Failed to deserialize GenesisGuardConfig");
        assert!(parsed.enabled);
        assert_eq!(parsed.max_genesis_supply_pct, 20.0);
        assert_eq!(parsed.max_slot0_bundled_txs, 2);
        assert!(!parsed.reject_shared_tip_accounts);
    }

    #[test]
    fn test_admin_config_defaults_and_deserialization() {
        let default_admin = AdminConfig::default();
        assert_eq!(default_admin.admin_passphrase.as_deref(), Some("dmne-alpha-desk-2026"));

        let toml_str = r#"
            admin_passphrase = "custom-secret-key-999"
        "#;
        let parsed: AdminConfig = toml::from_str(toml_str).expect("Failed to deserialize AdminConfig");
        assert_eq!(parsed.admin_passphrase.as_deref(), Some("custom-secret-key-999"));

        let empty_toml = "";
        let parsed_empty: AdminConfig = toml::from_str(empty_toml).expect("Failed to deserialize empty AdminConfig");
        assert_eq!(parsed_empty.admin_passphrase.as_deref(), Some("dmne-alpha-desk-2026"));
    }
}

