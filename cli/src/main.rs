use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use bob_core::config::BobConfig;
use bob_core::db_migrations::apply_migrations;
use bob_core::db_policy::load_db_policy_rules;
use bob_core::fs_cache::FsIndex;
use bob_core::fs_watch::watch_and_persist;
use bob_core::ollama::generate as ollama_generate;
use bob_core::permissions::{PermissionEngine, PermissionRequest};
use clap::{Parser, Subcommand};
use serde_json::json;

#[derive(Parser, Debug)]
#[command(name = "bob-core")]
#[command(about = "BoB core utilities for local assistant infrastructure")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    BuildCache {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        exclude: Vec<String>,
    },
    Lookup {
        #[arg(long)]
        cache: Option<PathBuf>,
        #[arg(long)]
        path: PathBuf,
        #[arg(long, default_value_t = 10_000)]
        iterations: u32,
    },
    WatchCache {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        exclude: Vec<String>,
        #[arg(long)]
        max_events: Option<u64>,
        #[arg(long, default_value_t = 30_000)]
        idle_timeout_ms: u64,
    },
    SyncPath {
        #[arg(long)]
        cache: Option<PathBuf>,
        #[arg(long)]
        path: PathBuf,
    },
    CheckPermission {
        #[arg(long)]
        tool: String,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        command: Option<String>,
    },
    DbMigrate {
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    SyncPolicyDb {
        #[arg(long)]
        scope: Option<String>,
        #[arg(long, default_value_t = false)]
        persist: bool,
    },
    ChatOllama {
        #[arg(long)]
        model: String,
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        system: Option<String>,
    },
    ShowConfig,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = BobConfig::from_env();
    let mut permission_engine =
        PermissionEngine::load_or_default(&cfg.permission_policy_path, &cfg.audit_log_path)?;
    if cfg.policy_sync_from_db {
        sync_policy_from_db(&cfg, &mut permission_engine, None, false)?;
    }

    match cli.command {
        Commands::BuildCache {
            root,
            output,
            exclude,
        } => {
            authorize(
                &permission_engine,
                "cache.build",
                Some(root.to_string_lossy().to_string()),
                None,
            )?;
            let output = output.unwrap_or_else(|| cfg.fs_cache_path.clone());
            let index = FsIndex::build(&root, &exclude)?;
            index.save(&output)?;
            println!(
                "cache written: {} entries -> {}",
                index.total_entries(),
                output.display()
            );
        }
        Commands::Lookup {
            cache,
            path,
            iterations,
        } => {
            if iterations == 0 {
                anyhow::bail!("iterations must be > 0");
            }
            authorize(
                &permission_engine,
                "cache.lookup",
                Some(path.to_string_lossy().to_string()),
                None,
            )?;
            let cache = cache.unwrap_or_else(|| cfg.fs_cache_path.clone());
            let index = FsIndex::load(&cache)?;
            let lookup_path = std::fs::canonicalize(&path).unwrap_or(path);
            let lookup_path_str = lookup_path.to_string_lossy().to_string();

            let result = index.lookup(&lookup_path_str);
            println!("{}", serde_json::to_string_pretty(&result)?);

            let started = Instant::now();
            for _ in 0..iterations {
                let _ = index.lookup(&lookup_path_str);
            }
            let elapsed_ns = started.elapsed().as_nanos();
            let avg_ns = elapsed_ns / u128::from(iterations);
            println!("avg lookup: {avg_ns} ns over {iterations} iterations");
        }
        Commands::WatchCache {
            root,
            output,
            exclude,
            max_events,
            idle_timeout_ms,
        } => {
            authorize(
                &permission_engine,
                "cache.watch",
                Some(root.to_string_lossy().to_string()),
                None,
            )?;
            let output = output.unwrap_or_else(|| cfg.fs_cache_path.clone());
            let summary = watch_and_persist(&root, &output, &exclude, max_events, idle_timeout_ms)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        Commands::SyncPath { cache, path } => {
            authorize(
                &permission_engine,
                "cache.sync_path",
                Some(path.to_string_lossy().to_string()),
                None,
            )?;
            let cache = cache.unwrap_or_else(|| cfg.fs_cache_path.clone());
            let mut index = FsIndex::load(&cache)?;
            index.apply_path_change(&path)?;
            index.save(&cache)?;
            let payload = json!({
                "cache": cache.display().to_string(),
                "path": path.display().to_string(),
                "entries": index.total_entries(),
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        Commands::CheckPermission {
            tool,
            path,
            command,
        } => {
            let decision = permission_engine.authorize_and_audit(&PermissionRequest {
                tool,
                path: path.map(|p| p.to_string_lossy().to_string()),
                command,
            })?;
            println!("{}", serde_json::to_string_pretty(&decision)?);
        }
        Commands::DbMigrate { dir } => {
            authorize(&permission_engine, "db.migrate", None, None)?;
            let migration_dir = dir.unwrap_or(cfg.migrations_dir.clone());
            let summary = apply_migrations(&cfg.postgres_url, &migration_dir)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        Commands::SyncPolicyDb { scope, persist } => {
            authorize(&permission_engine, "policy.sync_db", None, None)?;
            let summary = sync_policy_from_db(&cfg, &mut permission_engine, scope, persist)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        Commands::ChatOllama {
            model,
            prompt,
            system,
        } => {
            authorize(&permission_engine, "model.chat", None, None)?;
            let response = ollama_generate(&cfg.ollama_url, &model, &prompt, system, None)?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Commands::ShowConfig => {
            authorize(&permission_engine, "config.show", None, None)?;
            println!("{}", serde_json::to_string_pretty(&cfg)?);
        }
    }

    Ok(())
}

fn authorize(
    engine: &PermissionEngine,
    tool: &str,
    path: Option<String>,
    command: Option<String>,
) -> Result<()> {
    let request = PermissionRequest {
        tool: tool.to_string(),
        path,
        command,
    };
    let decision = engine.authorize_and_audit(&request)?;
    if decision.allowed {
        return Ok(());
    }
    anyhow::bail!("permission denied: {}", decision.reason)
}

fn sync_policy_from_db(
    cfg: &BobConfig,
    permission_engine: &mut PermissionEngine,
    scope_override: Option<String>,
    persist: bool,
) -> Result<serde_json::Value> {
    let scope = scope_override.unwrap_or_else(|| cfg.policy_scope.clone());
    let (db_rules, summary) = load_db_policy_rules(&cfg.postgres_url, &scope)?;
    permission_engine.apply_db_rules(db_rules);
    if persist {
        permission_engine.persist_policy()?;
    }
    Ok(json!({
        "scope": summary.scope,
        "tool_rules": summary.tool_rules,
        "path_rules": summary.path_rules,
        "command_rules": summary.command_rules,
        "persisted": persist
    }))
}
