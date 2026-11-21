mod analyze;
mod card;
mod fix;
mod inventory;
mod paths;
mod report;
mod scope;
mod transcript;
mod util;

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use serde_json::json;

use crate::fix::PlanOptions;
use crate::paths::Paths;

/// Find the dead weight in your Claude Code setup: what every request
/// carries before you type, and what you never use.
#[derive(Parser)]
#[command(name = "deadweight", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// How many days of session history count as "used"
    #[arg(long, global = true, default_value_t = 30, value_name = "N")]
    days: u64,

    /// Claude Code config directory [default: $CLAUDE_CONFIG_DIR or ~/.claude]
    #[arg(long, global = true, value_name = "DIR")]
    config_dir: Option<PathBuf>,

    /// Print machine-readable JSON
    #[arg(long, global = true)]
    json: bool,

    /// List every source instead of the biggest ones
    #[arg(long, short)]
    all: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Disable unused plugins, hide unused skills and connectors (backed up, undoable)
    Fix {
        /// Apply without asking
        #[arg(long, short)]
        yes: bool,
        /// Show what would change and stop
        #[arg(long)]
        dry_run: bool,
        /// Also disable unused plugins that run hooks
        #[arg(long)]
        include_hooked: bool,
        /// Never touch this plugin or skill, or `connectors` (repeatable)
        #[arg(long, value_name = "NAME")]
        keep: Vec<String>,
        /// Move unused skills into the backup folder instead of hiding them
        #[arg(long)]
        park_skills: bool,
        /// Run even with fewer than 5 sessions of history
        #[arg(long)]
        force: bool,
    },
    /// Load plugins and skills only in the projects that use them (backed up, undoable)
    Scope {
        /// Apply without asking
        #[arg(long, short)]
        yes: bool,
        /// Show what would change and stop
        #[arg(long)]
        dry_run: bool,
        /// Also scope plugins that run hooks
        #[arg(long)]
        include_hooked: bool,
        /// Never touch this plugin or skill (repeatable)
        #[arg(long, value_name = "NAME")]
        keep: Vec<String>,
        /// Run even with fewer than 5 sessions of history
        #[arg(long)]
        force: bool,
    },
    /// Save a shareable card with your totals (no plugin or skill names)
    Card {
        /// Where to write it; a `.svg` path gets the SVG instead of a PNG
        #[arg(
            long,
            short,
            value_name = "FILE",
            default_value = "deadweight-card.png"
        )]
        output: PathBuf,
    },
    /// Revert the most recent `fix` or `scope`
    Undo {
        /// Apply without asking
        #[arg(long, short)]
        yes: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            anstream::eprintln!("deadweight: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let paths = Paths::resolve(cli.config_dir.clone())?;
    match cli.command {
        None => audit(&paths, &cli),
        Some(Command::Fix {
            yes,
            dry_run,
            include_hooked,
            ref keep,
            park_skills,
            force,
        }) => {
            let opts = PlanOptions {
                include_hooked,
                keep: keep.clone(),
                park_skills,
            };
            run_fix(&paths, &cli, &opts, yes, dry_run, force)
        }
        Some(Command::Scope {
            yes,
            dry_run,
            include_hooked,
            ref keep,
            force,
        }) => {
            let opts = PlanOptions {
                include_hooked,
                keep: keep.clone(),
                park_skills: false,
            };
            run_scope(&paths, &cli, &opts, yes, dry_run, force)
        }
        Some(Command::Card { ref output }) => run_card(&paths, &cli, output),
        Some(Command::Undo { yes }) => run_undo(&paths, cli.json, yes),
    }
}

fn load(paths: &Paths, days: u64) -> Result<(inventory::Inventory, analyze::Analysis)> {
    if !paths.claude_dir.is_dir() {
        bail!(
            "no Claude Code config found at {} (set --config-dir or CLAUDE_CONFIG_DIR)",
            paths.claude_dir.display()
        );
    }
    let inv = inventory::Inventory::load(paths);
    let history = transcript::load(&paths.projects(), days);
    match analyze::analyze(&inv, &history, days) {
        Some(a) => Ok((inv, a)),
        None => bail!(
            "no Claude Code session transcripts found under {}. Start a session, then run deadweight again.",
            paths.projects().display()
        ),
    }
}

fn audit(paths: &Paths, cli: &Cli) -> Result<ExitCode> {
    let (inv, a) = load(paths, cli.days)?;
    let plan = fix::plan(&a, &inv, &PlanOptions::default());
    let scope = scope::plan(&a, paths, &PlanOptions::default());
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "analysis": a, "plan": plan, "scope": scope }))?
        );
    } else {
        report::print(&a, &plan, &scope, cli.all);
    }
    Ok(ExitCode::SUCCESS)
}

fn run_card(paths: &Paths, cli: &Cli, output: &std::path::Path) -> Result<ExitCode> {
    let (inv, a) = load(paths, cli.days)?;
    let plan = fix::plan(&a, &inv, &PlanOptions::default());
    let card = card::Card::new(&a, &plan);
    card.write(output)?;
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "path": output, "card": card }))?
        );
    } else {
        report::success(&format!("Saved {}", output.display()));
        report::note("It shows totals only: no plugin, skill or server names. Share away.");
    }
    Ok(ExitCode::SUCCESS)
}

fn run_fix(
    paths: &Paths,
    cli: &Cli,
    opts: &PlanOptions,
    yes: bool,
    dry_run: bool,
    force: bool,
) -> Result<ExitCode> {
    let (inv, a) = load(paths, cli.days)?;
    if a.sessions < 5 && !force {
        bail!(
            "only {} session(s) in the last {} days; too little history to call anything unused (use --force to override)",
            a.sessions,
            cli.days
        );
    }
    let plan = fix::plan(&a, &inv, opts);
    if plan.is_empty() {
        if cli.json {
            println!("{}", json!({ "applied": false, "plan": plan }));
        } else {
            report::note("Nothing to fix: everything listed was used recently.");
        }
        return Ok(ExitCode::SUCCESS);
    }
    if dry_run {
        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "applied": false, "plan": plan }))?
            );
        } else {
            report::print_plan(&plan);
        }
        return Ok(ExitCode::SUCCESS);
    }
    if !cli.json {
        report::print_plan(&plan);
    }
    if !yes && !confirm("Apply these changes?")? {
        report::note("Nothing changed.");
        return Ok(ExitCode::SUCCESS);
    }
    let dir = fix::apply(&plan, paths)?;
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "applied": true, "backup": dir, "plan": plan }))?
        );
    } else {
        report::success(&format!("Done. Backup: {}", dir.display()));
        report::note(
            "Restart Claude Code to load the slimmer setup. Revert with `deadweight undo`.",
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn run_scope(
    paths: &Paths,
    cli: &Cli,
    opts: &PlanOptions,
    yes: bool,
    dry_run: bool,
    force: bool,
) -> Result<ExitCode> {
    let (_, a) = load(paths, cli.days)?;
    if a.sessions < 5 && !force {
        bail!(
            "only {} session(s) in the last {} days; too little history to tell where things are used (use --force to override)",
            a.sessions,
            cli.days
        );
    }
    let plan = scope::plan(&a, paths, opts);
    if plan.is_empty() || dry_run {
        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "applied": false, "plan": plan }))?
            );
        } else if plan.is_empty() && plan.hooked.is_empty() && plan.mcp.is_empty() {
            report::note(
                "Nothing to scope: everything you use is used in most of your projects, or nowhere yet.",
            );
        } else {
            report::print_scope_plan(&plan);
        }
        return Ok(ExitCode::SUCCESS);
    }
    if !cli.json {
        report::print_scope_plan(&plan);
    }
    if !yes && !confirm("Apply these changes?")? {
        report::note("Nothing changed.");
        return Ok(ExitCode::SUCCESS);
    }
    let dir = scope::apply(&plan, paths)?;
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "applied": true, "backup": dir, "plan": plan }))?
        );
    } else {
        report::success(&format!("Done. Backup: {}", dir.display()));
        report::note(
            "Restart Claude Code to pick it up. Revert with `deadweight undo`. Started using one in a new project? Run `claude plugin enable <name> --scope local` there.",
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn run_undo(paths: &Paths, json_out: bool, yes: bool) -> Result<ExitCode> {
    let Some((dir, manifest)) = fix::latest_backup(paths)? else {
        if json_out {
            println!("{}", json!({ "restored": false }));
        } else {
            report::note("No deadweight changes to undo.");
        }
        return Ok(ExitCode::SUCCESS);
    };
    if !json_out {
        report::note(&format!(
            "Backup {}: {} plugin(s), {} skill(s), {} project settings file(s)",
            dir.display(),
            manifest.plugins.len(),
            manifest.skills.len() + manifest.skill_overrides.len(),
            manifest.local_files.len()
        ));
    }
    if !yes && !confirm("Restore it?")? {
        report::note("Nothing changed.");
        return Ok(ExitCode::SUCCESS);
    }
    let m = fix::undo(paths, &dir, manifest)?;
    if json_out {
        println!(
            "{}",
            json!({
                "restored": true,
                "backup": dir,
                "plugins": m.plugins.len(),
                "skills": m.skills.len() + m.skill_overrides.len(),
                "project_files": m.local_files.len()
            })
        );
    } else {
        report::success("Restored. Restart Claude Code to pick it up.");
    }
    Ok(ExitCode::SUCCESS)
}

fn confirm(question: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("not running in a terminal; pass --yes to apply without asking");
    }
    print!("\n  {question} [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}
