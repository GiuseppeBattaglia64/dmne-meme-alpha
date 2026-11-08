// DMNE QUANT // CLI AGENT HARNESS & PR GATE
// Subcommands: plan | propose | apply --task-id <ID>

use dmne_meme_alpha::db::{
    create_agent_task, establish_connection, fetch_agent_task_by_id,
    fetch_harness_autopsies, init_agent_harness_tables, list_pending_tasks,
    resolve_agent_task, DEFAULT_DB_URL,
};
use dmne_meme_alpha::harness::{apply_patch_to_text, AgentSkill, HarnessEngine};
use std::env;
use std::fs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let subcommand = args[1].as_str();

    let client = establish_connection(DEFAULT_DB_URL).await?;
    init_agent_harness_tables(&client).await?;

    match subcommand {
        "plan" => {
            println!("=================================================================");
            println!("  DMNE MEME ALPHA // COLD-PATH AGENT HARNESS [PLAN]              ");
            println!("=================================================================");

            let records = fetch_harness_autopsies(&client, 50).await?;
            let (anomalies, summary) = HarnessEngine::query_anomalies(&records);

            println!("\n[1] AUTOPSY ANOMALY SCAN:");
            println!("  Total Audited Trades Evaluated: {}", records.len());
            println!("  Anomalies Identified:          {}", anomalies.len());
            println!("  Summary: {}\n", summary);

            println!("  Top Detected Anomalies:");
            for (idx, a) in anomalies.iter().take(5).enumerate() {
                println!(
                    "    {:02}. Trade #{:<4} | {:<8} | PnL: {:>6.2}% ({:>8.5} SOL) | Peak MFE: {:>5.1}% | Cause: {}",
                    idx + 1,
                    a.trade_id,
                    a.symbol,
                    a.realized_pnl_pct,
                    a.realized_pnl_sol,
                    a.peak_mfe_pct,
                    a.root_cause
                );
            }

            println!("\n[2] AVAILABLE TYPED SKILLS:");
            let skills = [
                AgentSkill::QueryAnomalies,
                AgentSkill::CounterfactualSweep,
                AgentSkill::CheckGuardrails,
                AgentSkill::GenerateConfigDiff,
            ];
            for skill in &skills {
                println!("  - {:<22} : {}", format!("{:?}", skill), skill.description());
            }

            println!("\n[3] PENDING WORK BOARD TASKS:");
            let pending = list_pending_tasks(&client).await?;
            if pending.is_empty() {
                println!("  No pending audit tasks on work board.");
            } else {
                for (task_id, task_type, anomaly_sum, delta_sol, _, passed) in pending {
                    println!(
                        "  * [{}] {} | Delta: {:+.5} SOL | Guardrails: {} | {}",
                        task_id,
                        task_type,
                        delta_sol,
                        if passed { "PASSED" } else { "FAILED" },
                        anomaly_sum
                    );
                }
            }

            println!("\nTo stage a candidate configuration proposal, execute:");
            println!("  cargo run --bin dmne_agent_harness -- propose\n");
        }

        "propose" => {
            println!("=================================================================");
            println!("  DMNE MEME ALPHA // COLD-PATH AGENT HARNESS [PROPOSE]           ");
            println!("=================================================================");

            let config_path = "config.toml";
            let original_config = fs::read_to_string(config_path)
                .map_err(|e| format!("Failed to read {}: {}", config_path, e))?;

            let records = fetch_harness_autopsies(&client, 50).await?;

            let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
            let task_id = format!("TASK-AUTOPSY-{}", timestamp);

            // Execute the sequential skill pipeline
            let proposed_liquidity_usd = 35_000.0;
            let proposal = HarnessEngine::run_proposal_pipeline(
                &records,
                &original_config,
                &task_id,
                proposed_liquidity_usd,
            );

            // Persist the staged task to PostgreSQL agent_audit_tasks
            create_agent_task(
                &client,
                &proposal.task_id,
                &proposal.task_type,
                &proposal.anomaly_summary,
                proposal.counterfactual_delta_sol,
                &proposal.proposed_diff,
                proposal.guardrail_checks_passed,
            )
            .await?;

            println!("\n[STAGED PROPOSAL DETAILS]");
            println!("  Task ID:                 {}", proposal.task_id);
            println!("  Task Type:               {}", proposal.task_type);
            println!("  Status:                  PENDING");
            println!("  Anomaly Summary:         {}", proposal.anomaly_summary);
            println!(
                "  Counterfactual Delta:    {:+.6} SOL (avoided losses across anomalies)",
                proposal.counterfactual_delta_sol
            );
            println!(
                "  Guardrail Invariants:    {}",
                if proposal.guardrail_checks_passed {
                    "PASSED (All hard safety bounds verified)"
                } else {
                    "FAILED (Safety invariants violated)"
                }
            );

            if !proposal.guardrail_violations.is_empty() {
                println!("  Violations:");
                for v in &proposal.guardrail_violations {
                    println!("    - {}", v);
                }
            }

            println!("\n[PROPOSED UNIFIED PATCH]");
            println!("{}", proposal.proposed_diff);

            println!("=================================================================");
            println!("  HUMAN-IN-THE-LOOP PR GATE:                                     ");
            println!("  The proposal is staged in PostgreSQL and awaiting confirmation.");
            println!("  To review and apply this patch to config.toml, execute:        ");
            println!("    cargo run --bin dmne_agent_harness -- apply --task-id {}", proposal.task_id);
            println!("=================================================================\n");
        }

        "apply" => {
            println!("=================================================================");
            println!("  DMNE MEME ALPHA // COLD-PATH AGENT HARNESS [APPLY]             ");
            println!("=================================================================");

            let mut task_id_arg = None;
            let mut iter = args.iter().skip(2);
            while let Some(arg) = iter.next() {
                if arg == "--task-id" {
                    task_id_arg = iter.next().cloned();
                } else if !arg.starts_with("--") && task_id_arg.is_none() {
                    task_id_arg = Some(arg.clone());
                }
            }

            let task_id = match task_id_arg {
                Some(id) => id,
                None => {
                    eprintln!("Error: Missing required argument --task-id <ID>");
                    eprintln!("Usage: cargo run --bin dmne_agent_harness -- apply --task-id <ID>");
                    std::process::exit(1);
                }
            };

            let task = fetch_agent_task_by_id(&client, &task_id).await?;
            let (_, task_type, status, anomaly_summary, delta_sol, proposed_diff, guardrail_passed) =
                match task {
                    Some(t) => t,
                    None => {
                        eprintln!("Error: Task ID '{}' not found in agent_audit_tasks", task_id);
                        std::process::exit(1);
                    }
                };

            if status == "APPLIED" {
                println!("Notice: Task '{}' is already in APPLIED state.", task_id);
                return Ok(());
            }

            if !guardrail_passed {
                eprintln!(
                    "Error: Task '{}' failed safety guardrail checks. Cannot apply unsafe proposal.",
                    task_id
                );
                std::process::exit(1);
            }

            println!("Applying staged proposal '{}' (Type: {})...", task_id, task_type);
            println!("Anomaly Summary: {}", anomaly_summary);
            println!("Expected Delta:  {:+.6} SOL", delta_sol);

            let config_path = "config.toml";
            let current_config = fs::read_to_string(config_path)
                .map_err(|e| format!("Failed to read {}: {}", config_path, e))?;

            let updated_config = apply_patch_to_text(&current_config, &proposed_diff)
                .map_err(|e| format!("Failed to apply patch: {}", e))?;

            fs::write(config_path, &updated_config)
                .map_err(|e| format!("Failed to write {}: {}", config_path, e))?;

            resolve_agent_task(&client, &task_id, "APPLIED").await?;

            println!("\nSUCCESS: Staged patch applied cleanly to {}", config_path);
            println!("Database status updated to APPLIED.");
            println!("-----------------------------------------------------------------");
            println!("STRICT GUARDRAIL NOTICE:");
            println!("Live daemon 'dmne-alpha.service' was NOT restarted (cold-path safety).");
            println!("When ready to activate new configuration in live execution:");
            println!("  sudo systemctl restart dmne-alpha.service");
            println!("-----------------------------------------------------------------\n");
        }

        _ => {
            eprintln!("Unknown subcommand: {}", subcommand);
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_usage() {
    println!("Usage: dmne_agent_harness <subcommand> [options]");
    println!("\nSubcommands:");
    println!("  plan                  Query recent autopsies, identify anomalies, and list skills");
    println!("  propose               Execute skill pipeline and stage candidate proposal in DB");
    println!("  apply --task-id <ID>  Apply validated proposal to config.toml and mark APPLIED");
}
