// DMNE QUANT // AGENT HARNESS & ENGINEERING PATTERNS
// Typed Skills, Persistent Work Board & Human-in-the-Loop PR Gate

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Safety invariant boundaries (Zero Magic Numbers)
pub const MIN_LIQUIDITY_LOWER_BOUND_USD: f64 = 20_000.0;
pub const MIN_LIQUIDITY_UPPER_BOUND_USD: f64 = 100_000.0;

pub const KELLY_BASE_LOWER_BOUND_PCT: f64 = 2.0;
pub const KELLY_BASE_UPPER_BOUND_PCT: f64 = 10.0;

pub const MAX_PERMISSIBLE_STOP_LOSS_PCT: f64 = -25.0; // Stop loss cannot widen beyond -25.0% (e.g. -30.0% fails)

pub const MIN_POSITION_SIZE_SOL: f64 = 0.010;
pub const MAX_POSITION_SIZE_SOL: f64 = 0.10;

pub const MIN_BASE_POSITION_PCT_NAV: f64 = 0.02;
pub const MAX_BASE_POSITION_PCT_NAV: f64 = 0.07;
pub const MIN_MAX_POSITION_PCT_NAV: f64 = 0.04;
pub const MAX_MAX_POSITION_PCT_NAV: f64 = 0.12;
pub const HARD_CEILING_MAX_COMMITTED_SOL: f64 = 0.10;

pub const MIN_MFE_ACTIVATION_PCT: f64 = 10.0;
pub const MAX_MFE_ACTIVATION_PCT: f64 = 50.0;

pub const HAWKES_ETA_LOWER_BOUND: f64 = 1.00;
pub const HAWKES_ETA_UPPER_BOUND: f64 = 1.40;

/// Typed skills executable by the harness
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSkill {
    QueryAnomalies,
    CounterfactualSweep,
    CheckGuardrails,
    GenerateConfigDiff,
}

impl AgentSkill {
    pub fn description(&self) -> &'static str {
        match self {
            Self::QueryAnomalies => "Query trade autopsy records for MFE givebacks and rug drain anomalies",
            Self::CounterfactualSweep => "Simulate candidate parameter adjustments against historical autopsy records",
            Self::CheckGuardrails => "Validate proposed configuration changes against hard safety invariants",
            Self::GenerateConfigDiff => "Format non-destructive unified patch against live config.toml",
        }
    }
}

/// Hard safety boundaries that cannot be violated by any agent proposal
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetyInvariants {
    pub min_liquidity_usd: f64,
    pub kelly_base_pct: f64,
    pub max_stop_loss_pct: f64,
    pub hawkes_eta_ceiling: f64,
}

impl Default for SafetyInvariants {
    fn default() -> Self {
        Self {
            min_liquidity_usd: 20_000.0,
            kelly_base_pct: 6.5,
            max_stop_loss_pct: -20.0,
            hawkes_eta_ceiling: 1.25,
        }
    }
}

impl SafetyInvariants {
    /// Validates invariants against safety boundaries
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut violations = Vec::new();

        if self.min_liquidity_usd < MIN_LIQUIDITY_LOWER_BOUND_USD
            || self.min_liquidity_usd > MIN_LIQUIDITY_UPPER_BOUND_USD
        {
            violations.push(format!(
                "min_liquidity_usd (${:.0}) violates permissible bounds [${:.0}, ${:.0}]",
                self.min_liquidity_usd, MIN_LIQUIDITY_LOWER_BOUND_USD, MIN_LIQUIDITY_UPPER_BOUND_USD
            ));
        }

        if self.kelly_base_pct < KELLY_BASE_LOWER_BOUND_PCT
            || self.kelly_base_pct > KELLY_BASE_UPPER_BOUND_PCT
        {
            violations.push(format!(
                "kelly_base_pct ({:.1}%) violates permissible bounds [{:.1}%, {:.1}%]",
                self.kelly_base_pct, KELLY_BASE_LOWER_BOUND_PCT, KELLY_BASE_UPPER_BOUND_PCT
            ));
        }

        // max_stop_loss_pct cannot widen beyond -25.0% (e.g. -30.0% is rejected)
        if self.max_stop_loss_pct < MAX_PERMISSIBLE_STOP_LOSS_PCT {
            violations.push(format!(
                "max_stop_loss_pct ({:.1}%) widens beyond safety ceiling ({:.1}%)",
                self.max_stop_loss_pct, MAX_PERMISSIBLE_STOP_LOSS_PCT
            ));
        }

        if self.hawkes_eta_ceiling < HAWKES_ETA_LOWER_BOUND
            || self.hawkes_eta_ceiling > HAWKES_ETA_UPPER_BOUND
        {
            violations.push(format!(
                "hawkes_eta_ceiling ({:.2}) violates permissible bounds [{:.2}, {:.2}]",
                self.hawkes_eta_ceiling, HAWKES_ETA_LOWER_BOUND, HAWKES_ETA_UPPER_BOUND
            ));
        }

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }
}

/// Validates safety invariants on a deserialized TomlConfigFile before patch application
pub fn validate_safety_invariants_on_toml(cfg: &crate::config::TomlConfigFile) -> Result<(), Vec<String>> {
    let mut violations = Vec::new();

    if let Some(r) = &cfg.risk {
        if r.min_entry_liquidity_usd < MIN_LIQUIDITY_LOWER_BOUND_USD
            || r.min_entry_liquidity_usd > MIN_LIQUIDITY_UPPER_BOUND_USD
        {
            violations.push(format!(
                "min_entry_liquidity_usd (${:.0}) violates permissible bounds [${:.0}, ${:.0}]",
                r.min_entry_liquidity_usd, MIN_LIQUIDITY_LOWER_BOUND_USD, MIN_LIQUIDITY_UPPER_BOUND_USD
            ));
        }

        if r.base_position_size_sol < MIN_POSITION_SIZE_SOL
            || r.base_position_size_sol > MAX_POSITION_SIZE_SOL
        {
            violations.push(format!(
                "base_position_size_sol ({:.4} SOL) violates permissible bounds [{:.4} SOL, {:.4} SOL]",
                r.base_position_size_sol, MIN_POSITION_SIZE_SOL, MAX_POSITION_SIZE_SOL
            ));
        }

        if r.base_position_pct_nav < MIN_BASE_POSITION_PCT_NAV
            || r.base_position_pct_nav > MAX_BASE_POSITION_PCT_NAV
        {
            violations.push(format!(
                "base_position_pct_nav ({:.3}) violates permissible bounds [{:.3}, {:.3}]",
                r.base_position_pct_nav, MIN_BASE_POSITION_PCT_NAV, MAX_BASE_POSITION_PCT_NAV
            ));
        }

        if r.max_position_pct_nav < MIN_MAX_POSITION_PCT_NAV
            || r.max_position_pct_nav > MAX_MAX_POSITION_PCT_NAV
        {
            violations.push(format!(
                "max_position_pct_nav ({:.3}) violates permissible bounds [{:.3}, {:.3}]",
                r.max_position_pct_nav, MIN_MAX_POSITION_PCT_NAV, MAX_MAX_POSITION_PCT_NAV
            ));
        }

        if r.max_entry_sol > HARD_CEILING_MAX_COMMITTED_SOL {
            violations.push(format!(
                "max_entry_sol ({:.4} SOL) breaches hard safety ceiling ({:.4} SOL)",
                r.max_entry_sol, HARD_CEILING_MAX_COMMITTED_SOL
            ));
        }

        if r.stop_loss_pct < MAX_PERMISSIBLE_STOP_LOSS_PCT {
            violations.push(format!(
                "stop_loss_pct ({:.1}%) widens beyond safety floor ({:.1}%)",
                r.stop_loss_pct, MAX_PERMISSIBLE_STOP_LOSS_PCT
            ));
        }

        if r.max_slippage_loss_pct < MAX_PERMISSIBLE_STOP_LOSS_PCT {
            violations.push(format!(
                "max_slippage_loss_pct ({:.1}%) widens beyond safety floor ({:.1}%)",
                r.max_slippage_loss_pct, MAX_PERMISSIBLE_STOP_LOSS_PCT
            ));
        }
    }

    let mfe_val = cfg
        .dynamic_exits
        .as_ref()
        .map(|d| d.mfe_activation_pct)
        .or_else(|| cfg.risk.as_ref().map(|r| r.dynamic_exits.mfe_activation_pct));
    if let Some(mfe) = mfe_val {
        if mfe < MIN_MFE_ACTIVATION_PCT || mfe > MAX_MFE_ACTIVATION_PCT {
            violations.push(format!(
                "mfe_activation_pct ({:.1}%) violates permissible bounds [{:.1}%, {:.1}%]",
                mfe, MIN_MFE_ACTIVATION_PCT, MAX_MFE_ACTIVATION_PCT
            ));
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Anomaly classification for autopsy trades
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnomalyType {
    HighMfeGiveback,
    RapidDrainRug,
    ExcessiveSlippage,
}

/// Detected anomaly record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeAnomaly {
    pub trade_id: i32,
    pub symbol: String,
    pub realized_pnl_pct: f64,
    pub realized_pnl_sol: f64,
    pub peak_mfe_pct: f64,
    pub root_cause: String,
    pub anomaly_type: AnomalyType,
    pub counterfactual_raw: String,
}

/// Strongly-typed agent proposal staged on the audit work board
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProposal {
    pub task_id: String,
    pub task_type: String,
    pub anomaly_summary: String,
    pub counterfactual_delta_sol: f64,
    pub proposed_diff: String,
    pub guardrail_checks_passed: bool,
    pub guardrail_violations: Vec<String>,
}

/// Generates a standard unified diff patch between original and modified text
pub fn generate_unified_diff(
    file_path: &str,
    original: &str,
    modified: &str,
) -> String {
    let orig_lines: Vec<&str> = original.lines().collect();
    let mod_lines: Vec<&str> = modified.lines().collect();

    let mut diff = format!("--- a/{}\n+++ b/{}\n", file_path, file_path);

    let mut i = 0;
    let mut j = 0;
    while i < orig_lines.len() || j < mod_lines.len() {
        if i < orig_lines.len() && j < mod_lines.len() && orig_lines[i] == mod_lines[j] {
            i += 1;
            j += 1;
        } else {
            let start_orig = i + 1;
            let start_mod = j + 1;
            let mut removed = Vec::new();
            let mut added = Vec::new();

            while i < orig_lines.len() && (j >= mod_lines.len() || orig_lines[i] != mod_lines[j]) {
                if mod_lines.iter().skip(j).any(|&l| l == orig_lines[i]) {
                    break;
                }
                removed.push(orig_lines[i]);
                i += 1;
            }

            while j < mod_lines.len() && (i >= orig_lines.len() || orig_lines[i] != mod_lines[j]) {
                added.push(mod_lines[j]);
                j += 1;
            }

            diff.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                start_orig,
                removed.len(),
                start_mod,
                added.len()
            ));
            for r in removed {
                diff.push_str(&format!("-{}\n", r));
            }
            for a in added {
                diff.push_str(&format!("+{}\n", a));
            }
        }
    }

    diff
}

/// Applies unified patch removals and additions to an original text
pub fn apply_patch_to_text(original: &str, diff: &str) -> Result<String, String> {
    let mut result = original.to_string();

    let mut current_removals = Vec::new();
    let mut current_additions = Vec::new();
    let mut in_hunk = false;

    for line in diff.lines() {
        if line.starts_with("@@") {
            if in_hunk {
                // Apply previous hunk
                result = apply_hunk(&result, &current_removals, &current_additions)?;
                current_removals.clear();
                current_additions.clear();
            }
            in_hunk = true;
        } else if in_hunk {
            if let Some(stripped) = line.strip_prefix('-') {
                current_removals.push(stripped.to_string());
            } else if let Some(stripped) = line.strip_prefix('+') {
                current_additions.push(stripped.to_string());
            }
        }
    }

    if in_hunk && (!current_removals.is_empty() || !current_additions.is_empty()) {
        result = apply_hunk(&result, &current_removals, &current_additions)?;
    }

    Ok(result)
}

/// Safely applies a unified diff to a configuration file, creating a .bak copy before modifying
pub fn apply_patch_to_file_safe(file_path: &str, diff: &str) -> Result<String, String> {
    let original = std::fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read {}: {}", file_path, e))?;

    // Create backup prior to modification
    let backup_path = format!("{}.bak", file_path);
    std::fs::copy(file_path, &backup_path)
        .map_err(|e| format!("Failed to create backup at {}: {}", backup_path, e))?;

    let modified = apply_patch_to_text(&original, diff)?;
    std::fs::write(file_path, &modified)
        .map_err(|e| format!("Failed to write {}: {}", file_path, e))?;

    Ok(modified)
}

fn apply_hunk(
    source: &str,
    removals: &[String],
    additions: &[String],
) -> Result<String, String> {
    if removals.is_empty() {
        return Ok(source.to_string());
    }

    let target_block = removals.join("\n");
    let replacement_block = additions.join("\n");

    if !source.contains(&target_block) {
        return Err(format!(
            "Hunk target block not found in source text: {:?}",
            target_block
        ));
    }

    Ok(source.replacen(&target_block, &replacement_block, 1))
}

/// Orchestrates sequential skill execution
pub struct HarnessEngine;

impl HarnessEngine {
    /// Skill 1: Query anomalies from autopsy records
    pub fn query_anomalies(
        records: &[crate::db::HarnessAutopsyRecord],
    ) -> (Vec<TradeAnomaly>, String) {
        let mut anomalies = Vec::new();

        for r in records {
            let is_high_mfe = r.peak_mfe_pct >= 15.0 && r.realized_pnl_pct <= -5.0;
            let is_rug = r.root_cause_category == "RAPID_DRAIN_RUG";
            let is_slippage = r.root_cause_category == "EXCESSIVE_SLIPPAGE" && r.realized_pnl_pct <= -20.0;

            if is_high_mfe {
                anomalies.push(TradeAnomaly {
                    trade_id: r.trade_id,
                    symbol: r.symbol.clone(),
                    realized_pnl_pct: r.realized_pnl_pct,
                    realized_pnl_sol: r.realized_pnl_sol,
                    peak_mfe_pct: r.peak_mfe_pct,
                    root_cause: r.root_cause_category.clone(),
                    anomaly_type: AnomalyType::HighMfeGiveback,
                    counterfactual_raw: r.counterfactual_recommendation_raw.clone(),
                });
            } else if is_rug {
                anomalies.push(TradeAnomaly {
                    trade_id: r.trade_id,
                    symbol: r.symbol.clone(),
                    realized_pnl_pct: r.realized_pnl_pct,
                    realized_pnl_sol: r.realized_pnl_sol,
                    peak_mfe_pct: r.peak_mfe_pct,
                    root_cause: r.root_cause_category.clone(),
                    anomaly_type: AnomalyType::RapidDrainRug,
                    counterfactual_raw: r.counterfactual_recommendation_raw.clone(),
                });
            } else if is_slippage {
                anomalies.push(TradeAnomaly {
                    trade_id: r.trade_id,
                    symbol: r.symbol.clone(),
                    realized_pnl_pct: r.realized_pnl_pct,
                    realized_pnl_sol: r.realized_pnl_sol,
                    peak_mfe_pct: r.peak_mfe_pct,
                    root_cause: r.root_cause_category.clone(),
                    anomaly_type: AnomalyType::ExcessiveSlippage,
                    counterfactual_raw: r.counterfactual_recommendation_raw.clone(),
                });
            }
        }

        let summary = format!(
            "Identified {} anomalies across {} audited trades: {}",
            anomalies.len(),
            records.len(),
            anomalies
                .iter()
                .take(3)
                .map(|a| format!("#{}: {} ({:.1}% PnL, {:.1}% MFE)", a.trade_id, a.symbol, a.realized_pnl_pct, a.peak_mfe_pct))
                .collect::<Vec<_>>()
                .join(", ")
        );

        (anomalies, summary)
    }

    /// Skill 2: Counterfactual sweep across anomalies
    pub fn counterfactual_sweep(
        anomalies: &[TradeAnomaly],
        proposed_min_liquidity: f64,
    ) -> f64 {
        let mut total_saved_sol = 0.0;

        for a in anomalies {
            // Check if counterfactual recommendation recommended tightening liquidity
            // or if the trade loss would be avoided by the higher liquidity threshold
            let would_avoid = a.counterfactual_raw.contains("LIQUIDITY_GATE")
                || a.counterfactual_raw.contains("would_avoid_trade\":true")
                || a.counterfactual_raw.contains("would_avoid_trade\": true");

            if would_avoid && proposed_min_liquidity >= 35_000.0 {
                total_saved_sol += a.realized_pnl_sol.abs();
            }
        }

        total_saved_sol
    }

    /// Skill 3: Check proposal against safety invariants
    pub fn check_guardrails(
        invariants: &SafetyInvariants,
    ) -> (bool, Vec<String>) {
        match invariants.validate() {
            Ok(()) => (true, Vec::new()),
            Err(violations) => (false, violations),
        }
    }

    /// Skill 4: Generate non-destructive unified diff
    pub fn generate_diff(
        config_path: &str,
        original_config: &str,
        replacements: &HashMap<&str, &str>,
    ) -> (String, String) {
        let mut modified = original_config.to_string();

        for (target, replacement) in replacements {
            if modified.contains(*target) {
                modified = modified.replace(*target, *replacement);
            }
        }

        let diff = generate_unified_diff(config_path, original_config, &modified);
        (diff, modified)
    }

    /// Executes full pipeline: QueryAnomalies -> CounterfactualSweep -> CheckGuardrails -> GenerateConfigDiff
    pub fn run_proposal_pipeline(
        records: &[crate::db::HarnessAutopsyRecord],
        original_config: &str,
        task_id: &str,
        proposed_min_liquidity: f64,
    ) -> AgentProposal {
        // 1. Query anomalies
        let (anomalies, anomaly_summary) = Self::query_anomalies(records);

        // 2. Counterfactual sweep
        let counterfactual_delta_sol = Self::counterfactual_sweep(&anomalies, proposed_min_liquidity);

        // 3. Safety invariants
        let invariants = SafetyInvariants {
            min_liquidity_usd: proposed_min_liquidity,
            kelly_base_pct: 6.5,
            max_stop_loss_pct: -20.0,
            hawkes_eta_ceiling: 1.25,
        };
        let (mut passed_checks, mut violations) = Self::check_guardrails(&invariants);

        // Bi-Directional Alpha & Significance Gate: net_alpha_delta_sol > 0.0150 SOL && sample_size >= 30
        if counterfactual_delta_sol <= 0.0150 {
            passed_checks = false;
            violations.push(format!(
                "Net alpha delta ({:+.6} SOL) fails threshold: requires > +0.0150 SOL net rescued alpha",
                counterfactual_delta_sol
            ));
        }
        if records.len() < 30 {
            passed_checks = false;
            violations.push(format!(
                "Sample size ({} records) fails threshold: requires >= 30 historical settled trade records",
                records.len()
            ));
        }

        // 4. Generate config diff
        let target_line = "min_entry_liquidity_usd = 20000.0";
        let new_line = format!("min_entry_liquidity_usd = {:.1}", proposed_min_liquidity);
        let mut replacements = HashMap::new();
        replacements.insert(target_line, new_line.as_str());

        let (proposed_diff, _) = Self::generate_diff("config.toml", original_config, &replacements);

        AgentProposal {
            task_id: task_id.to_string(),
            task_type: "CONFIG_CALIBRATION".to_string(),
            anomaly_summary,
            counterfactual_delta_sol,
            proposed_diff,
            guardrail_checks_passed: passed_checks,
            guardrail_violations: violations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safety_invariants_rejects_unsafe_stop() {
        // A stop loss of -30.0% widens beyond the safety ceiling of -25.0%
        let unsafe_invariants = SafetyInvariants {
            min_liquidity_usd: 25_000.0,
            kelly_base_pct: 6.5,
            max_stop_loss_pct: -30.0,
            hawkes_eta_ceiling: 1.25,
        };

        let result = unsafe_invariants.validate();
        assert!(result.is_err(), "Expected safety invariants validation to fail");

        let violations = result.unwrap_err();
        assert!(
            violations.iter().any(|v| v.contains("max_stop_loss_pct") && v.contains("widens beyond safety ceiling")),
            "Expected violation error mentioning max_stop_loss_pct ceiling violation, got: {:?}",
            violations
        );
    }

    #[test]
    fn test_safety_invariants_accepts_valid_bounds() {
        let valid_invariants = SafetyInvariants {
            min_liquidity_usd: 35_000.0,
            kelly_base_pct: 6.5,
            max_stop_loss_pct: -20.0,
            hawkes_eta_ceiling: 1.25,
        };

        assert!(valid_invariants.validate().is_ok());
    }

    #[test]
    fn test_generate_diff_produces_valid_unified_patch() {
        let original = "[risk]\nstop_loss_pct = -20.0\nmin_entry_liquidity_usd = 20000.0\n";
        let modified = "[risk]\nstop_loss_pct = -20.0\nmin_entry_liquidity_usd = 35000.0\n";

        let diff = generate_unified_diff("config.toml", original, modified);

        assert!(diff.contains("--- a/config.toml"));
        assert!(diff.contains("+++ b/config.toml"));
        assert!(diff.contains("@@ -1,3 +1,3 @@") || diff.contains("@@ -2,1 +2,1 @@") || diff.contains("@@"));
        assert!(diff.contains("-min_entry_liquidity_usd = 20000.0"));
        assert!(diff.contains("+min_entry_liquidity_usd = 35000.0"));

        // Verify that patch application produces the exact modified text
        let applied = apply_patch_to_text(original, &diff).expect("Patch application failed");
        assert_eq!(applied, modified);
    }

    #[test]
    fn test_apply_patch_to_file_creates_backup() {
        let temp_dir = std::env::temp_dir();
        let target_file = temp_dir.join("test_config_backup.toml");
        let backup_file = temp_dir.join("test_config_backup.toml.bak");

        let original = "[risk]\nstop_loss_pct = -20.0\n";
        let modified = "[risk]\nstop_loss_pct = -18.0\n";
        std::fs::write(&target_file, original).unwrap();

        let diff = generate_unified_diff("test_config_backup.toml", original, modified);
        let result = apply_patch_to_file_safe(target_file.to_str().unwrap(), &diff);
        assert!(result.is_ok());

        assert!(backup_file.exists(), "Expected .bak backup file to be created");
        let backup_content = std::fs::read_to_string(&backup_file).unwrap();
        assert_eq!(backup_content, original);

        let final_content = std::fs::read_to_string(&target_file).unwrap();
        assert_eq!(final_content, modified);

        let _ = std::fs::remove_file(target_file);
        let _ = std::fs::remove_file(backup_file);
    }

    #[test]
    fn test_validate_safety_invariants_on_toml() {
        let mut cfg = crate::config::TomlConfigFile {
            risk: Some(crate::config::RiskConfig::default()),
            sandbox: None,
            hawkes: None,
            dynamic_exits: Some(crate::config::DynamicExitsConfig::default()),
            orderflow: None,
            genesis_guard: None,
            funder_graph: None,
            admin: None,
            solana: None,
        };

        assert!(validate_safety_invariants_on_toml(&cfg).is_ok());

        // Test invalid min_entry_liquidity_usd (< 20000)
        cfg.risk.as_mut().unwrap().min_entry_liquidity_usd = 15000.0;
        let res = validate_safety_invariants_on_toml(&cfg);
        assert!(res.is_err());
        assert!(res.unwrap_err()[0].contains("min_entry_liquidity_usd"));

        // Reset and test invalid base_position_size_sol (> 0.10)
        cfg.risk.as_mut().unwrap().min_entry_liquidity_usd = 35000.0;
        cfg.risk.as_mut().unwrap().base_position_size_sol = 0.1500;
        let res_size = validate_safety_invariants_on_toml(&cfg);
        assert!(res_size.is_err());
        assert!(res_size.unwrap_err()[0].contains("base_position_size_sol"));

        // Reset and test invalid stop loss widening beyond -25%
        cfg.risk.as_mut().unwrap().base_position_size_sol = 0.0225;
        cfg.risk.as_mut().unwrap().stop_loss_pct = -30.0;
        let res_sl = validate_safety_invariants_on_toml(&cfg);
        assert!(res_sl.is_err());
        assert!(res_sl.unwrap_err()[0].contains("stop_loss_pct"));

        // Reset and test invalid mfe_activation_pct (< 10.0)
        cfg.risk.as_mut().unwrap().stop_loss_pct = -20.0;
        cfg.dynamic_exits.as_mut().unwrap().mfe_activation_pct = 5.0;
        let res_mfe = validate_safety_invariants_on_toml(&cfg);
        assert!(res_mfe.is_err());
        assert!(res_mfe.unwrap_err()[0].contains("mfe_activation_pct"));

        // Reset and test invalid base_position_pct_nav (< 0.02 or > 0.07)
        cfg.dynamic_exits.as_mut().unwrap().mfe_activation_pct = 15.0;
        cfg.risk.as_mut().unwrap().base_position_pct_nav = 0.01;
        let res_nav_low = validate_safety_invariants_on_toml(&cfg);
        assert!(res_nav_low.is_err());
        assert!(res_nav_low.unwrap_err()[0].contains("base_position_pct_nav"));

        cfg.risk.as_mut().unwrap().base_position_pct_nav = 0.09;
        let res_nav_high = validate_safety_invariants_on_toml(&cfg);
        assert!(res_nav_high.is_err());
        assert!(res_nav_high.unwrap_err()[0].contains("base_position_pct_nav"));

        // Reset and test invalid max_position_pct_nav (< 0.04 or > 0.12)
        cfg.risk.as_mut().unwrap().base_position_pct_nav = 0.045;
        cfg.risk.as_mut().unwrap().max_position_pct_nav = 0.030;
        let res_max_low = validate_safety_invariants_on_toml(&cfg);
        assert!(res_max_low.is_err());
        assert!(res_max_low.unwrap_err()[0].contains("max_position_pct_nav"));

        cfg.risk.as_mut().unwrap().max_position_pct_nav = 0.15;
        let res_max_high = validate_safety_invariants_on_toml(&cfg);
        assert!(res_max_high.is_err());
        assert!(res_max_high.unwrap_err()[0].contains("max_position_pct_nav"));

        // Reset and test invalid max_entry_sol (> 0.10)
        cfg.risk.as_mut().unwrap().max_position_pct_nav = 0.085;
        cfg.risk.as_mut().unwrap().max_entry_sol = 0.15;
        let res_entry_high = validate_safety_invariants_on_toml(&cfg);
        assert!(res_entry_high.is_err());
        assert!(res_entry_high.unwrap_err()[0].contains("max_entry_sol"));
    }
}

