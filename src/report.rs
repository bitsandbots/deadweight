//! Human-readable terminal report.

use anstream::println;
use anstyle::{AnsiColor, Style};

use crate::analyze::{Analysis, Kind, ScopeHint, ScopeKind, Source};
use crate::fix::Plan;
use crate::scope::ScopePlan;
use crate::util::{bar, fmt_tokens, plural, short_time};

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();
const RED: Style = AnsiColor::Red.on_default().bold();
const YELLOW: Style = AnsiColor::Yellow.on_default().bold();
const GREEN: Style = AnsiColor::Green.on_default().bold();
const CYAN: Style = AnsiColor::Cyan.on_default();

fn paint(style: Style, text: impl std::fmt::Display) -> String {
    format!("{style}{text}{style:#}")
}

pub fn print(a: &Analysis, plan: &Plan, scope: &ScopePlan, show_all: bool) {
    println!();
    println!(
        "  {}  {}",
        paint(BOLD, "deadweight"),
        paint(DIM, "what your Claude Code setup costs on every request")
    );
    let when = a
        .session_started
        .as_deref()
        .map(short_time)
        .unwrap_or_else(|| "unknown time".into());
    println!(
        "  {}",
        paint(
            DIM,
            format!(
                "latest session {when} · {} {} in the last {} days",
                a.sessions,
                plural(a.sessions, "session", "sessions"),
                a.window_days
            )
        )
    );
    println!();

    headline(a);
    categories(a);
    sources(a, show_all);
    problems(a);
    footer(a, plan, scope);
}

fn headline(a: &Analysis) {
    match a.measured_tokens {
        Some(m) => {
            println!(
                "  Before you type a word, every request carries {}",
                paint(RED, format!("{} tokens", fmt_tokens(m as f64)))
            );
            let mut note = "measured from your latest session".to_string();
            if let Some(med) = a.median_tokens {
                note.push_str(&format!(
                    "; median of recent sessions {}",
                    fmt_tokens(med as f64)
                ));
            }
            println!("  {}", paint(DIM, note));
        }
        None => println!(
            "  Your setup adds about {} tokens to every request",
            paint(RED, fmt_tokens(a.setup_tokens))
        ),
    }
    let builtin = match a.builtin_tools {
        Some(n) => format!("Claude Code itself (system prompt + {n} tools)"),
        None => "Claude Code itself".to_string(),
    };
    println!(
        "  {} from your setup · ~{} from {builtin}",
        paint(BOLD, format!("~{}", fmt_tokens(a.setup_tokens))),
        fmt_tokens(a.builtin_tokens)
    );
    println!();
}

fn categories(a: &Analysis) {
    println!("  {}", paint(BOLD, "WHAT YOUR SETUP ADDS"));
    let max = a.categories.first().map(|c| c.tokens).unwrap_or(0.0);
    for c in &a.categories {
        let label = format!("{} ({})", c.kind.label(), c.count);
        println!(
            "  {:<34} {:>7}  {}",
            label,
            format!("~{}", fmt_tokens(c.tokens)),
            paint(CYAN, bar(c.tokens, max, 24))
        );
    }
    println!();
}

fn sources(a: &Analysis, show_all: bool) {
    let list: Vec<_> = a
        .sources
        .iter()
        .filter(|s| !s.source.is_builtin() && s.tokens >= 1.0)
        .collect();
    let limit = if show_all { list.len() } else { 12 };
    println!("  {}", paint(BOLD, "BIGGEST SOURCES"));
    println!(
        "  {}",
        paint(
            DIM,
            format!(
                "{:<30} {:<13} {:>7}  {:>6}  {}",
                "name", "type", "tokens", "used", "contents"
            )
        )
    );
    for s in list.iter().take(limit) {
        let mut parts = Vec::new();
        if s.hook_tokens >= 1.0 {
            parts.push(format!("hooks {}", fmt_tokens(s.hook_tokens)));
        }
        if s.tools > 0 {
            parts.push(format!("{} {}", s.tools, plural(s.tools, "tool", "tools")));
        }
        if s.skills > 0 {
            parts.push(format!(
                "{} {}",
                s.skills,
                plural(s.skills, "skill", "skills")
            ));
        }
        if s.agents > 0 {
            parts.push(format!(
                "{} {}",
                s.agents,
                plural(s.agents, "agent", "agents")
            ));
        }
        let used = if matches!(s.source, Source::Memory(_)) || s.hook_tokens >= s.tokens {
            paint(DIM, format!("{:>6}", "n/a"))
        } else if s.uses == 0 {
            paint(RED, format!("{:>6}", "never"))
        } else {
            paint(GREEN, format!("{:>6}", format!("{}×", s.uses)))
        };
        println!(
            "  {:<30} {:<13} {:>7}  {}  {}",
            truncate(&s.label, 30),
            s.source.kind_label(),
            format!("~{}", fmt_tokens(s.tokens)),
            used,
            paint(DIM, parts.join(" · "))
        );
    }
    if list.len() > limit {
        println!(
            "  {}",
            paint(DIM, format!("… {} more (use --all)", list.len() - limit))
        );
    }
    println!();
}

fn problems(a: &Analysis) {
    let mut lines: Vec<String> = Vec::new();
    let bare = a.skills_without_description.len();
    if bare > 0 {
        lines.push(format!(
            "{} {} of {} skills are listed {}: the skill listing is full",
            paint(RED, "✗"),
            bare,
            a.skills_listed,
            paint(BOLD, "without a description"),
        ));
        lines.push(paint(
            DIM,
            format!(
                "    (~{} characters). Claude sees only their names, so it can't tell when to use them.",
                fmt_tokens(a.skill_listing_chars as f64)
            ),
        ));
        if !a.used_skills_without_description.is_empty() {
            lines.push(paint(
                DIM,
                format!(
                    "    Including {} you actually use: {}",
                    a.used_skills_without_description.len(),
                    preview(&a.used_skills_without_description, 5)
                ),
            ));
        }
        if let Some(b) = &a.skill_budget {
            lines.push(format!(
                "    {} hide what you don't use ({}), or set {} in settings.json",
                paint(DIM, "Make room:"),
                paint(BOLD, "deadweight fix"),
                paint(
                    CYAN,
                    format!("\"skillListingBudgetFraction\": {}", b.suggested)
                ),
            ));
            lines.push(paint(
                DIM,
                format!(
                    "    ({} {}; the extra room costs up to ~{} tokens per request).",
                    if b.configured { "now" } else { "default" },
                    b.fraction,
                    fmt_tokens(b.extra_tokens)
                ),
            ));
        }
    }
    if !a.failed_mcp.is_empty() {
        lines.push(format!(
            "{} {} MCP {} failed to connect at startup",
            paint(RED, "✗"),
            a.failed_mcp.len(),
            plural(a.failed_mcp.len(), "server", "servers")
        ));
        for f in &a.failed_mcp {
            lines.push(paint(DIM, format!("    {} ({})", f.name, f.code)));
        }
    }
    if !a.unauthenticated.is_empty() {
        let tools: usize = a.unauthenticated.iter().map(|u| u.tools).sum();
        lines.push(format!(
            "{} {} MCP {} never signed in, still listing {} auth-only tools",
            paint(YELLOW, "!"),
            a.unauthenticated.len(),
            plural(a.unauthenticated.len(), "server", "servers"),
            tools
        ));
        let names: Vec<String> = a.unauthenticated.iter().map(|u| u.server.clone()).collect();
        lines.push(paint(DIM, format!("    {}", preview(&names, 6))));
    }
    if !a.duplicates.is_empty() {
        lines.push(format!(
            "{} {} skill {} listed more than once",
            paint(YELLOW, "!"),
            a.duplicates.len(),
            plural(a.duplicates.len(), "name is", "names are")
        ));
        for d in a.duplicates.iter().take(6) {
            lines.push(paint(
                DIM,
                format!("    {}: {}", d.name, d.entries.join(", ")),
            ));
        }
    }
    if a.skills_listed > 0 {
        let unused_tokens: f64 = a
            .items
            .iter()
            .filter(|i| i.kind == Kind::Skill && i.uses == 0 && !i.source.is_builtin())
            .map(|i| i.tokens)
            .sum();
        lines.push(format!(
            "{} {} of {} listed skills were not used in {} days (~{} per request)",
            paint(YELLOW, "!"),
            a.skills_unused,
            a.skills_listed,
            a.window_days,
            fmt_tokens(unused_tokens)
        ));
    }
    for h in a.heavy_skills.iter().take(3) {
        lines.push(format!(
            "{} {} alone costs ~{} tokens per request (used {}× in {} days)",
            paint(YELLOW, "!"),
            paint(BOLD, &h.name),
            fmt_tokens(h.tokens),
            h.uses,
            a.window_days
        ));
    }
    if lines.is_empty() {
        return;
    }
    println!("  {}", paint(BOLD, "PROBLEMS"));
    for l in lines {
        println!("  {l}");
    }
    println!();
}

fn footer(a: &Analysis, plan: &Plan, scope: &ScopePlan) {
    if a.sessions < 5 {
        println!(
            "  {}",
            paint(
                YELLOW,
                format!(
                    "Only {} {} of history: usage numbers are thin. Come back after a few more sessions.",
                    a.sessions,
                    plural(a.sessions, "session", "sessions")
                )
            )
        );
    }
    if !plan.is_empty() {
        let mut what = Vec::new();
        if !plan.plugins.is_empty() {
            what.push(format!(
                "disable {} unused {}",
                plan.plugins.len(),
                plural(plan.plugins.len(), "plugin", "plugins")
            ));
        }
        if !plan.skills.is_empty() {
            what.push(format!(
                "{} {} unused {}",
                if plan.park_skills { "park" } else { "hide" },
                plan.skills.len(),
                plural(plan.skills.len(), "skill", "skills")
            ));
        }
        if let Some(c) = &plan.connectors {
            what.push(format!(
                "turn off {} unused claude.ai {}",
                c.names.len(),
                plural(c.names.len(), "connector", "connectors")
            ));
        }
        let last = what.pop().unwrap_or_default();
        let what = if what.is_empty() {
            last
        } else {
            format!("{} and {last}", what.join(", "))
        };
        println!(
            "  Run {} to {} and save {} per request.",
            paint(BOLD, "deadweight fix"),
            what,
            paint(GREEN, format!("~{} tokens", fmt_tokens(plan.tokens)))
        );
    }
    if !plan.hooked.is_empty() {
        let t: f64 = plan.hooked.iter().map(|p| p.tokens).sum();
        let names: Vec<String> = plan.hooked.iter().map(|p| p.label.clone()).collect();
        println!(
            "  {}",
            paint(
                DIM,
                format!(
                    "{} more unused {} {} hooks ({}, ~{}); review, or add --include-hooked.",
                    names.len(),
                    plural(names.len(), "plugin", "plugins"),
                    plural(names.len(), "runs", "run"),
                    preview(&names, 4),
                    fmt_tokens(t)
                )
            )
        );
    }
    if !a.manual.is_empty() {
        println!();
        println!("  {}", paint(BOLD, "UNUSED, BUT YOURS TO REMOVE"));
        for m in a.manual.iter().take(6) {
            println!(
                "  {:<34} ~{:<6} {}",
                truncate(&m.what, 34),
                fmt_tokens(m.tokens),
                paint(CYAN, &m.how)
            );
        }
    }
    scope_hints(a, scope);
    println!();
    let basis = if a.measured_tokens.is_some() {
        "The total is measured; the breakdown is estimated from text size."
    } else {
        "Figures are estimated from text size."
    };
    println!("  {}", paint(DIM, basis));
    println!();
}

fn scope_hints(a: &Analysis, scope: &ScopePlan) {
    if a.scope_hints.is_empty() {
        return;
    }
    println!();
    println!(
        "  {}",
        paint(BOLD, "LOADED EVERYWHERE, USED IN A FEW PROJECTS")
    );
    for h in a.scope_hints.iter().take(8) {
        println!(
            "  {:<34} ~{:<6} {}",
            truncate(&format!("{} {}", h.kind.label(), h.label), 34),
            fmt_tokens(h.tokens),
            paint(DIM, idle_note(h))
        );
        let used: Vec<String> = h.used_in.iter().map(|(p, _)| short_dir(p)).collect();
        println!(
            "    {}",
            paint(DIM, format!("used in {}", preview(&used, 4)))
        );
        if h.kind == ScopeKind::Mcp {
            mcp_move(h);
        }
    }
    let n = scope.moves.len();
    if n > 0 {
        println!(
            "  Run {} to load {} only where {} used: ~{} less per request everywhere else.",
            paint(BOLD, "deadweight scope"),
            if n == 1 {
                "it".to_owned()
            } else {
                format!("these {n}")
            },
            plural(n, "it's", "they're"),
            paint(GREEN, fmt_tokens(scope.tokens))
        );
    }
    if !scope.hooked.is_empty() {
        let names: Vec<String> = scope.hooked.iter().map(|m| m.label.clone()).collect();
        println!(
            "  {}",
            paint(
                DIM,
                format!(
                    "Not included: {} ({} hooks that act on your work in any project).",
                    preview(&names, 4),
                    plural(names.len(), "it has", "they have")
                )
            )
        );
    }
}

/// "idle in 10 of 13 projects (412 of 540 sessions)"
fn idle_note(h: &ScopeHint) -> String {
    format!(
        "idle in {} of {} projects ({} of {} sessions)",
        h.idle_projects, h.projects_total, h.idle_sessions, h.sessions
    )
}

fn mcp_move(h: &ScopeHint) {
    let target = match h.used_in.as_slice() {
        [(p, _)] => format!("projects > \"{p}\" > mcpServers"),
        _ => "each of those projects' mcpServers".to_owned(),
    };
    println!(
        "    {}",
        paint(
            CYAN,
            format!(
                "in .claude.json, move \"{}\" from the top-level mcpServers into {target}",
                h.name
            )
        )
    );
}

/// The last folder of a project path, which is how people name projects.
fn short_dir(p: &str) -> String {
    p.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(p)
        .to_owned()
}

pub fn print_scope_plan(plan: &ScopePlan) {
    println!();
    if !plan.moves.is_empty() {
        println!(
            "  {}",
            paint(BOLD, "Load these only in the projects that use them")
        );
        println!(
            "  {}",
            paint(
                DIM,
                "off in ~/.claude/settings.json, on in each project's .claude/settings.local.json"
            )
        );
        for m in &plan.moves {
            println!(
                "    {:<32} ~{:<6} {}",
                truncate(&format!("{} {}", m.kind.label(), m.label), 32),
                fmt_tokens(m.tokens),
                paint(
                    DIM,
                    format!(
                        "saved in {} {} ({} of {} sessions)",
                        m.idle_projects,
                        plural(m.idle_projects, "project", "projects"),
                        m.idle_sessions,
                        m.sessions
                    )
                )
            );
            for p in &m.projects {
                let note = if m.already_on.contains(p) {
                    " (already on there)"
                } else {
                    ""
                };
                println!("      {} {}{note}", paint(GREEN, "on"), p.display());
            }
        }
        println!(
            "\n  Saves {} per request in the projects that never use them. Undo any time with {}.",
            paint(GREEN, format!("~{} tokens", fmt_tokens(plan.tokens))),
            paint(BOLD, "deadweight undo")
        );
    }
    if !plan.hooked.is_empty() {
        let names: Vec<String> = plan.hooked.iter().map(|m| m.label.clone()).collect();
        println!(
            "  {}",
            paint(
                DIM,
                format!(
                    "Left alone because {} hooks that may matter everywhere: {}. Add --include-hooked to scope {} too.",
                    plural(names.len(), "it runs", "they run"),
                    preview(&names, 4),
                    plural(names.len(), "it", "them")
                )
            )
        );
    }
    if !plan.mcp.is_empty() {
        println!();
        println!(
            "  {}",
            paint(
                BOLD,
                "MCP servers to move yourself (.claude.json can hold credentials)"
            )
        );
        for h in &plan.mcp {
            println!(
                "  {:<34} ~{:<6} {}",
                truncate(&format!("mcp server {}", h.label), 34),
                fmt_tokens(h.tokens),
                paint(DIM, idle_note(h))
            );
            mcp_move(h);
        }
    }
}

pub fn print_plan(plan: &Plan) {
    println!();
    if !plan.plugins.is_empty() {
        println!(
            "  {}",
            paint(
                BOLD,
                "Disable these plugins (settings.json → enabledPlugins: false)"
            )
        );
        for p in &plan.plugins {
            println!("    {:<56} ~{}", p.key, fmt_tokens(p.tokens));
        }
    }
    if !plan.skills.is_empty() {
        let heading = if plan.park_skills {
            "Move these skills into the backup folder".to_owned()
        } else {
            format!(
                "Hide these skills from Claude (settings.json → skillOverrides: \"{}\"; /name still works)",
                crate::fix::HIDDEN
            )
        };
        println!("  {}", paint(BOLD, heading));
        for s in &plan.skills {
            println!("    {:<56} ~{}", s.name, fmt_tokens(s.tokens));
        }
    }
    if let Some(c) = &plan.connectors {
        println!(
            "  {}",
            paint(
                BOLD,
                "Turn off claude.ai connectors in Claude Code (settings.json → disableClaudeAiConnectors: true)"
            )
        );
        println!(
            "    {:<56} ~{}",
            truncate(&c.names.join(", "), 56),
            fmt_tokens(c.tokens)
        );
    }
    println!(
        "\n  Saves {} per request. Undo any time with {}.",
        paint(GREEN, format!("~{} tokens", fmt_tokens(plan.tokens))),
        paint(BOLD, "deadweight undo")
    );
}

pub fn success(msg: &str) {
    println!("  {} {msg}", paint(GREEN, "✓"));
}

pub fn note(msg: &str) {
    println!("  {}", paint(DIM, msg));
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut t: String = s.chars().take(max - 1).collect();
        t.push('…');
        t
    }
}

fn preview(names: &[String], n: usize) -> String {
    let mut s = names.iter().take(n).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > n {
        s.push_str(&format!(", +{} more", names.len() - n));
    }
    s
}
