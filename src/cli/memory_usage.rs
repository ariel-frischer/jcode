//! Memory CLI dispatch and offline accounting presentation. Usage never loads graphs.
use crate::memory_usage::{
    summary::{UsageReport, report_in_dir},
    types::{MemoryRequestObservation, TokenSubtotal, validate_accounting_identifier},
};
use crate::{memory, storage};
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::{ffi::OsString, fmt::Write as _, io::Write as _};

pub enum MemorySubcommand {
    List {
        scope: String,
        tag: Option<String>,
    },
    Search {
        query: String,
        semantic: bool,
    },
    Export {
        output: String,
        scope: String,
    },
    Import {
        input: String,
        scope: String,
        overwrite: bool,
    },
    Stats,
    ClearTest,
    Usage {
        session: Option<String>,
        calls: bool,
        json: bool,
    },
}

/// The typed Clap command is authoritative. The cheap token check avoids parsing
/// ordinary invocations twice; a prompt/model named `memory` cannot dispatch usage.
pub(super) fn try_run_offline(args: impl IntoIterator<Item = OsString>) -> Option<Result<()>> {
    let args: Vec<_> = args.into_iter().collect();
    if !args.iter().any(|arg| arg == "memory") {
        return None;
    }
    let parsed = match super::args::Args::try_parse_from(&args) {
        Ok(parsed) => parsed,
        Err(error) => {
            if args
                .iter()
                .any(|arg| arg == "usage" || arg == "--help" || arg == "-h" || arg == "help")
            {
                error.exit();
            }
            return None;
        }
    };
    if let Some(super::args::Command::Memory(MemoryCommand::Usage {
        session,
        calls,
        json,
    })) = parsed.command
    {
        return Some((|| {
            if let Some(cwd) = parsed.cwd {
                std::env::set_current_dir(cwd)
                    .map_err(|_| anyhow::anyhow!("usage working directory unavailable"))?;
            }
            run(session.as_deref(), calls, json)
        })());
    }
    None
}

pub(super) fn run(session: Option<&str>, calls: bool, json: bool) -> Result<()> {
    if let Some(session) = session {
        validate_accounting_identifier(session).map_err(|_| anyhow::anyhow!(
            "invalid session selector: use 1-128 ASCII identifier bytes, without paths, spaces or '..'"
        ))?;
    }
    // Canonical precedence, without cache initialization, migration or raw TOML
    // error output (the parser's error may contain sensitive config source).
    let controls = crate::config::Config::load_strict()
        .map_err(|_| {
            anyhow::anyhow!("usage configuration unavailable or invalid; check config.toml")
        })?
        .lifecycle_observability
        .effective_status();
    let base =
        crate::storage::jcode_dir().map_err(|_| anyhow::anyhow!("usage data root unavailable"))?;
    let report = report_in_dir(&base, session, controls, calls)?;
    let output = if json {
        serde_json::to_string_pretty(&report)?
    } else {
        render_text(&report)?
    };
    writeln!(std::io::stdout().lock(), "{output}")?;
    Ok(())
}

fn render_text(report: &UsageReport) -> Result<String> {
    let mut text = String::from("Memory usage: retained observations only, not lifetime totals.\n");
    writeln!(text, "{}", report.pricing_policy)?;
    writeln!(
        text,
        "Controls: enabled={} persistence={} structured_logs={}",
        report.controls.enabled,
        report.controls.persist_session_events,
        report.controls.emit_structured_logs
    )?;
    writeln!(text, "Coverage: {}", label(&report.coverage)?)?;
    for warning in &report.storage_warnings {
        writeln!(text, "Warning: {}", label(warning)?)?;
    }
    for warning in &report.warnings {
        writeln!(text, "Warning: {}", label(warning)?)?;
    }
    if report.sessions.is_empty() {
        writeln!(
            text,
            "No retained observations. Historical consumption is unknown."
        )?;
    }
    for session in &report.sessions {
        writeln!(
            text,
            "\nSession {}: {} observed calls, coverage={}",
            session.session_id.as_deref().unwrap_or("unattributed"),
            session.calls,
            label(&session.coverage)?
        )?;
        writeln!(
            text,
            "  Retained window: {} to {}",
            label(&session.window.first_recorded_at)?,
            label(&session.window.last_recorded_at)?
        )?;
        for (name, tokens) in [
            ("Input", session.tokens.input_tokens),
            ("Cached input (subset)", session.tokens.cached_input_tokens),
            (
                "Cache creation (subset)",
                session.tokens.cache_creation_tokens,
            ),
            ("Output", session.tokens.output_tokens),
            ("Reasoning (output subset)", session.tokens.reasoning_tokens),
        ] {
            write_subtotal(&mut text, name, tokens)?;
        }
        writeln!(
            text,
            "  API-equivalent cost known subtotal={}, unknown cost calls={}",
            usd(session.known_cost_subtotal_nano_usd),
            session.unknown_cost_calls
        )?;
    }
    if let Some(calls) = &report.calls {
        for call in calls {
            write_call(&mut text, call)?;
        }
    }
    Ok(text)
}

fn write_subtotal(text: &mut String, name: &str, tokens: TokenSubtotal) -> Result<()> {
    writeln!(
        text,
        "  {name}: known subtotal={}, unknown calls={}",
        tokens.known_subtotal, tokens.unknown_calls
    )?;
    Ok(())
}

fn write_call(text: &mut String, call: &MemoryRequestObservation) -> Result<()> {
    writeln!(
        text,
        "\nCall {} at {}: session={} operation={}:{}",
        call.request_id,
        call.recorded_at,
        call.context.session_id.as_deref().unwrap_or("unattributed"),
        label(&call.context.operation_kind)?,
        call.context.operation_id
    )?;
    writeln!(
        text,
        "  provider={} model={} effort={} auth={} outcome={} attempt_coverage={}",
        call.provider,
        call.model,
        label(&call.effort)?,
        label(&call.auth_class)?,
        label(&call.outcome)?,
        label(&call.attempt_coverage)?
    )?;
    writeln!(
        text,
        "  input={} cached_input={} cache_creation={} output={} reasoning={} total={} (subsets counted once)",
        label(&call.usage.input_tokens)?,
        label(&call.usage.cached_input_tokens)?,
        label(&call.usage.cache_creation_tokens)?,
        label(&call.usage.output_tokens)?,
        label(&call.usage.reasoning_tokens)?,
        label(&call.usage.total_tokens()?)?
    )?;
    writeln!(
        text,
        "  API-equivalent estimate={}, known subtotal={}, basis={}",
        call.pricing
            .estimate_nano_usd
            .map(usd)
            .unwrap_or_else(|| "unknown".into()),
        usd(call.pricing.known_subtotal_nano_usd),
        label(&call.pricing.basis)?
    )?;
    Ok(())
}

fn usd(nano_usd: u64) -> String {
    format!(
        "${}.{:09}",
        nano_usd / 1_000_000_000,
        nano_usd % 1_000_000_000
    )
}

fn label(value: &impl serde::Serialize) -> Result<String> {
    let serialized = serde_json::to_string(value)?;
    Ok(if serialized == "null" {
        "unknown".into()
    } else {
        serialized.trim_matches('"').into()
    })
}

#[derive(Subcommand, Debug)]
pub(crate) enum MemoryCommand {
    /// Inspect retained local request usage and API-equivalent costs (offline, not billed charges)
    Usage {
        /// Filter by the authentic originating session ID
        #[arg(long)]
        session: Option<String>,
        /// Include each retained request with model, operation and optional usage
        #[arg(long)]
        calls: bool,
        /// Emit deterministic versioned JSON with null unknown values
        #[arg(long)]
        json: bool,
    },

    /// List all stored memories
    List {
        /// Filter by scope (project, global, all)
        #[arg(short, long, default_value = "all")]
        scope: String,

        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,
    },

    /// Search memories by query
    Search {
        /// Search query
        query: String,

        /// Use semantic search (embedding-based) instead of keyword
        #[arg(short, long)]
        semantic: bool,
    },

    /// Export memories to a JSON file
    Export {
        /// Output file path
        output: String,

        /// Export scope (project, global, all)
        #[arg(short, long, default_value = "all")]
        scope: String,
    },

    /// Import memories from a JSON file
    Import {
        /// Input file path
        input: String,

        /// Import scope (project, global)
        #[arg(short, long, default_value = "project")]
        scope: String,

        /// Overwrite existing memories with same ID
        #[arg(long)]
        overwrite: bool,
    },

    /// Show memory statistics
    Stats,

    /// Clear test memory storage (used by debug sessions)
    ClearTest,
}

pub(super) fn map_subcommand(subcmd: MemoryCommand) -> MemorySubcommand {
    match subcmd {
        MemoryCommand::List { scope, tag } => MemorySubcommand::List { scope, tag },
        MemoryCommand::Search { query, semantic } => MemorySubcommand::Search { query, semantic },
        MemoryCommand::Export { output, scope } => MemorySubcommand::Export { output, scope },
        MemoryCommand::Import {
            input,
            scope,
            overwrite,
        } => MemorySubcommand::Import {
            input,
            scope,
            overwrite,
        },
        MemoryCommand::Usage {
            session,
            calls,
            json,
        } => MemorySubcommand::Usage {
            session,
            calls,
            json,
        },
        MemoryCommand::Stats => MemorySubcommand::Stats,
        MemoryCommand::ClearTest => MemorySubcommand::ClearTest,
    }
}

pub fn run_memory_command(cmd: MemorySubcommand) -> Result<()> {
    let project_dir = match std::env::current_dir() {
        Ok(dir) => Some(dir),
        Err(_) => {
            eprintln!("Working directory unavailable; project memories cannot be resolved.");
            None
        }
    };
    run_memory_command_for_dir(cmd, project_dir)
}

pub(super) fn run_memory_command_for_dir(
    cmd: MemorySubcommand,
    project_dir: Option<std::path::PathBuf>,
) -> Result<()> {
    use memory::{MemoryEntry, MemoryManager};

    let project_dir = project_dir.filter(|dir| !dir.as_os_str().is_empty());
    // Match agent-side memory resolution: project memory is available only when
    // the caller supplied a concrete working directory.
    // Usage reporting must not initialize sidecars or load memory graphs.
    let manager = std::cell::LazyCell::new(|| match project_dir.as_ref() {
        Some(dir) => MemoryManager::new().with_project_dir(dir),
        _ => MemoryManager::new(),
    });

    match cmd {
        MemorySubcommand::Usage {
            session,
            calls,
            json,
        } => {
            super::memory_usage::run(session.as_deref(), calls, json)?;
        }
        MemorySubcommand::List { scope, tag } => {
            let mut all_memories: Vec<MemoryEntry> = Vec::new();

            if (scope == "all" || scope == "project")
                && let Ok(graph) = manager.load_project_graph()
            {
                all_memories.extend(graph.all_memories().cloned());
            }
            if (scope == "all" || scope == "global")
                && let Ok(graph) = manager.load_global_graph()
            {
                all_memories.extend(graph.all_memories().cloned());
            }

            if let Some(tag_filter) = tag {
                all_memories.retain(|m| m.tags.contains(&tag_filter));
            }

            all_memories.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

            if all_memories.is_empty() {
                println!("No memories found.");
            } else {
                println!("Found {} memories:\n", all_memories.len());
                for entry in &all_memories {
                    let tags_str = if entry.tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", entry.tags.join(", "))
                    };
                    let conf = entry.effective_confidence();
                    println!(
                        "- [{}] {}{}\n  id: {} (conf: {:.0}%, accessed: {}x)",
                        entry.category,
                        entry.content,
                        tags_str,
                        entry.id,
                        conf * 100.0,
                        entry.access_count
                    );
                    println!();
                }
            }
        }

        MemorySubcommand::Search { query, semantic } => {
            if semantic {
                match manager.find_similar(&query, 0.3, 20) {
                    Ok(results) => {
                        if results.is_empty() {
                            println!("No memories found matching '{}'", query);
                        } else {
                            println!(
                                "Found {} memories matching '{}' (semantic):\n",
                                results.len(),
                                query
                            );
                            for (entry, score) in results {
                                let tags_str = if entry.tags.is_empty() {
                                    String::new()
                                } else {
                                    format!(" [{}]", entry.tags.join(", "))
                                };
                                println!(
                                    "- [{}] {}{}\n  id: {} (score: {:.0}%)",
                                    entry.category,
                                    entry.content,
                                    tags_str,
                                    entry.id,
                                    score * 100.0
                                );
                                println!();
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Search failed: {}", e);
                    }
                }
            } else {
                match manager.search(&query) {
                    Ok(results) => {
                        if results.is_empty() {
                            println!("No memories found matching '{}'", query);
                        } else {
                            println!(
                                "Found {} memories matching '{}' (keyword):\n",
                                results.len(),
                                query
                            );
                            for entry in results {
                                let tags_str = if entry.tags.is_empty() {
                                    String::new()
                                } else {
                                    format!(" [{}]", entry.tags.join(", "))
                                };
                                println!(
                                    "- [{}] {}{}\n  id: {}",
                                    entry.category, entry.content, tags_str, entry.id
                                );
                                println!();
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Search failed: {}", e);
                    }
                }
            }
        }

        MemorySubcommand::Export { output, scope } => {
            let mut all_memories: Vec<memory::MemoryEntry> = Vec::new();

            if (scope == "all" || scope == "project")
                && let Ok(graph) = manager.load_project_graph()
            {
                all_memories.extend(graph.all_memories().cloned());
            }
            if (scope == "all" || scope == "global")
                && let Ok(graph) = manager.load_global_graph()
            {
                all_memories.extend(graph.all_memories().cloned());
            }

            let json = serde_json::to_string_pretty(&all_memories)?;
            std::fs::write(&output, json)?;
            println!("Exported {} memories to {}", all_memories.len(), output);
        }

        MemorySubcommand::Import {
            input,
            scope,
            overwrite,
        } => {
            if scope != "global" && project_dir.is_none() {
                anyhow::bail!("cannot import project memories without a project directory");
            }
            let content = std::fs::read_to_string(&input)?;
            let memories: Vec<memory::MemoryEntry> = serde_json::from_str(&content)?;

            let mut imported = 0;
            let mut skipped = 0;

            for entry in memories {
                let entry_id = entry.id.clone();
                let result = if scope == "global" {
                    if !overwrite
                        && let Ok(graph) = manager.load_global_graph()
                        && graph.get_memory(&entry.id).is_some()
                    {
                        skipped += 1;
                        continue;
                    }
                    manager.remember_global(entry)
                } else {
                    if !overwrite
                        && let Ok(graph) = manager.load_project_graph()
                        && graph.get_memory(&entry.id).is_some()
                    {
                        skipped += 1;
                        continue;
                    }
                    manager.remember_project(entry)
                };

                result.map_err(|error| {
                    anyhow::anyhow!(
                        "failed to durably import memory {entry_id} into {scope} scope: {error}"
                    )
                })?;
                imported += 1;
            }

            println!("Imported {} memories ({} skipped)", imported, skipped);
        }

        MemorySubcommand::Stats => {
            let mut project_count = 0;
            let mut global_count = 0;
            let mut total_tags = std::collections::HashSet::new();
            let mut categories: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();

            if let Ok(graph) = manager.load_project_graph() {
                project_count = graph.memory_count();
                for entry in graph.all_memories() {
                    for tag in &entry.tags {
                        total_tags.insert(tag.clone());
                    }
                    *categories.entry(entry.category.to_string()).or_default() += 1;
                }
            }

            if let Ok(graph) = manager.load_global_graph() {
                global_count = graph.memory_count();
                for entry in graph.all_memories() {
                    for tag in &entry.tags {
                        total_tags.insert(tag.clone());
                    }
                    *categories.entry(entry.category.to_string()).or_default() += 1;
                }
            }

            println!("Memory Statistics:");
            println!("  Project memories: {}", project_count);
            println!("  Global memories:  {}", global_count);
            println!("  Total:            {}", project_count + global_count);
            println!("  Unique tags:      {}", total_tags.len());
            println!("\nBy category:");
            for (cat, count) in &categories {
                println!("  {}: {}", cat, count);
            }
        }

        MemorySubcommand::ClearTest => {
            let test_dir = storage::jcode_dir()?.join("memory").join("test");
            if test_dir.exists() {
                let count = std::fs::read_dir(&test_dir)?.count();
                std::fs::remove_dir_all(&test_dir)?;
                println!("Cleared test memory storage ({} files)", count);
            } else {
                println!("Test memory storage is already empty");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::cli::args::{Args, Command, MemoryCommand};
    use clap::Parser;

    #[test]
    fn offline_dispatch_does_not_capture_other_commands_or_model_names() {
        for args in [
            vec!["jcode", "memory", "stats"],
            vec!["jcode", "--model", "memory", "memory", "stats"],
            vec!["jcode", "--model", "usage", "memory", "list"],
            vec!["jcode", "memory", "search", "usage"],
        ] {
            assert!(super::try_run_offline(args.into_iter().map(Into::into)).is_none());
        }
    }

    #[test]
    fn usage_arguments_parse_and_reject_invalid_combinations() {
        let args = Args::try_parse_from([
            "jcode",
            "memory",
            "usage",
            "--session",
            "session-a",
            "--calls",
            "--json",
        ])
        .unwrap();
        assert!(
            matches!(args.command, Some(Command::Memory(MemoryCommand::Usage { session: Some(session), calls: true, json: true })) if session == "session-a")
        );
        let args = Args::try_parse_from(["jcode", "memory", "usage"]).unwrap();
        assert!(matches!(
            args.command,
            Some(Command::Memory(MemoryCommand::Usage {
                session: None,
                calls: false,
                json: false
            }))
        ));
        for args in [
            vec!["jcode", "memory", "usage", "--session"],
            vec!["jcode", "memory", "usage", "--calls", "true"],
            vec!["jcode", "memory", "usage", "--unknown"],
        ] {
            assert!(Args::try_parse_from(args).is_err());
        }
    }
}
