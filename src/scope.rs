//! `deadweight scope`: load plugins and skills only in the projects that use them.
//!
//! A plugin or user skill you use in two projects still loads in all the
//! others, on every request. `scope` turns it off in your user settings and
//! back on in `.claude/settings.local.json` of each project that used it.
//! Local settings take precedence over user settings, so nothing changes in
//! those projects. Like `fix`, every change is recorded in a backup manifest
//! and `deadweight undo` reverts it.
//!
//! User-scope MCP servers are reported, not moved: their entries in
//! `~/.claude.json` can hold credentials, and Claude Code rewrites that file
//! while it runs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;

use crate::analyze::{Analysis, ScopeHint, ScopeKind};
use crate::fix::{
    ENABLED_KEY, GitExclude, HIDDEN, LocalChange, LocalFile, Manifest, OVERRIDES, PlanOptions,
    PluginChange, SettingChange, enabled_plugins, object_entry, read_settings, write_json_atomic,
    write_manifest,
};
use crate::inventory::read_json;
use crate::paths::{Paths, home_dir};
use crate::transcript::project_key;
use crate::util::timestamp_now;

/// What a project's own settings already say about something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Local {
    /// Nothing: deadweight writes it.
    Unset,
    /// Already on there; nothing to write.
    On,
    /// Turned off there on purpose; leave that project alone.
    Off,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeMove {
    pub kind: ScopeKind,
    /// Plugin key (`name@marketplace`) or user skill name.
    pub name: String,
    pub label: String,
    pub tokens: f64,
    /// Projects it stays on in, through their `.claude/settings.local.json`.
    pub projects: Vec<PathBuf>,
    /// Of those, the ones whose settings already turn it on.
    pub already_on: Vec<PathBuf>,
    /// Projects that used it but need nothing: the folder is gone, or the
    /// project's own settings turn it off.
    pub ignored: Vec<String>,
    pub idle_projects: usize,
    pub idle_sessions: u32,
    pub sessions: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScopePlan {
    pub moves: Vec<ScopeMove>,
    /// Plugins left alone because they run hooks (`--include-hooked`).
    pub hooked: Vec<ScopeMove>,
    /// User-scope MCP servers to move by hand.
    pub mcp: Vec<ScopeHint>,
    /// Tokens per request saved in the projects that never use them.
    pub tokens: f64,
}

impl ScopePlan {
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }
}

pub fn plan(a: &Analysis, paths: &Paths, opts: &PlanOptions) -> ScopePlan {
    let kept = |label: &str, name: &str| {
        opts.keep
            .iter()
            .any(|k| k.eq_ignore_ascii_case(label) || k.eq_ignore_ascii_case(name))
    };
    let mut p = ScopePlan::default();
    for h in &a.scope_hints {
        if kept(&h.label, &h.name) {
            continue;
        }
        if h.kind == ScopeKind::Mcp {
            p.mcp.push(h.clone());
            continue;
        }
        let mut m = ScopeMove {
            kind: h.kind,
            name: h.name.clone(),
            label: h.label.clone(),
            tokens: h.tokens,
            projects: Vec::new(),
            already_on: Vec::new(),
            ignored: Vec::new(),
            idle_projects: h.idle_projects,
            idle_sessions: h.idle_sessions,
            sessions: h.sessions,
        };
        // Turning it off in user settings is only safe if every project
        // that used it can keep it.
        let mut blocked = false;
        for (project, _) in &h.used_in {
            let dir = PathBuf::from(project);
            if !dir.is_dir() {
                m.ignored.push(project.clone());
                continue;
            }
            if !writable_project(&dir, paths) {
                blocked = true;
                break;
            }
            match local_state(&dir, h.kind, &h.name) {
                Local::Off => m.ignored.push(project.clone()),
                Local::On => {
                    m.already_on.push(dir.clone());
                    m.projects.push(dir);
                }
                Local::Unset => m.projects.push(dir),
            }
        }
        if blocked || m.projects.is_empty() {
            continue;
        }
        if h.work_hooks && !opts.include_hooked {
            p.hooked.push(m);
        } else {
            p.moves.push(m);
        }
    }
    p.tokens = p.moves.iter().map(|m| m.tokens).sum();
    p
}

/// A directory that can take a project settings file without clashing with
/// your user-level Claude Code config.
fn writable_project(dir: &Path, paths: &Paths) -> bool {
    let key = norm(dir);
    let claude = norm(&paths.claude_dir);
    if key == claude || key.starts_with(&format!("{claude}/")) {
        return false;
    }
    // `~/.claude/settings.local.json` would sit next to your user settings.
    if let Some(home) = home_dir() {
        if key == norm(&home) {
            return false;
        }
    }
    dir.parent().is_some()
}

fn norm(p: &Path) -> String {
    let s = project_key(&p.to_string_lossy());
    let s = s.trim_end_matches('/').to_owned();
    if cfg!(windows) { s.to_lowercase() } else { s }
}

fn settings_files(dir: &Path) -> [PathBuf; 2] {
    let d = dir.join(".claude");
    [d.join("settings.local.json"), d.join("settings.json")]
}

fn local_state(dir: &Path, kind: ScopeKind, name: &str) -> Local {
    let section = section(kind);
    // settings.local.json wins over settings.json, so check it first.
    for file in settings_files(dir) {
        let Some(v) = read_json(&file).and_then(|s| s.get(section)?.get(name).cloned()) else {
            continue;
        };
        let on = match kind {
            ScopeKind::Plugin => v.as_bool() == Some(true),
            _ => v.as_str() == Some("on"),
        };
        return if on { Local::On } else { Local::Off };
    }
    Local::Unset
}

fn section(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Plugin => ENABLED_KEY,
        _ => OVERRIDES,
    }
}

fn on_value(kind: ScopeKind) -> Value {
    match kind {
        ScopeKind::Plugin => Value::Bool(true),
        _ => Value::String("on".into()),
    }
}

/// Apply a scope plan. Returns the backup directory.
pub fn apply(plan: &ScopePlan, paths: &Paths) -> Result<PathBuf> {
    if plan.is_empty() {
        bail!("nothing to change");
    }
    let dir = paths.backups().join(timestamp_now());
    fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;

    let settings_path = paths.settings();
    let mut settings = read_settings(&settings_path)?;
    if settings_path.exists() {
        fs::copy(&settings_path, dir.join("settings.json"))
            .context("cannot back up settings.json")?;
    }

    let mut m = Manifest::new(3);
    for mv in &plan.moves {
        match mv.kind {
            ScopeKind::Plugin => m.plugins.push(PluginChange {
                key: mv.name.clone(),
                previous: enabled_plugins(&mut settings)?.get(&mv.name).cloned(),
            }),
            _ => m.skill_overrides.push(SettingChange {
                key: mv.name.clone(),
                previous: object_entry(&mut settings, OVERRIDES)?
                    .get(&mv.name)
                    .cloned(),
            }),
        }
    }

    // Every project file, read once, with the changes it gets.
    let mut files: Vec<(LocalFile, Value)> = Vec::new();
    for mv in &plan.moves {
        for project in mv.projects.iter().filter(|p| !mv.already_on.contains(p)) {
            let file = project.join(".claude").join("settings.local.json");
            let i = match files.iter().position(|(f, _)| f.file == file) {
                Some(i) => i,
                None => {
                    let existed = file.exists();
                    let contents = read_settings(&file)?;
                    if existed {
                        let copy = dir.join("local").join(format!("{}.json", files.len()));
                        fs::create_dir_all(copy.parent().unwrap_or(&dir))?;
                        fs::copy(&file, &copy)
                            .with_context(|| format!("cannot back up {}", file.display()))?;
                    }
                    files.push((
                        LocalFile {
                            created_dir: !project.join(".claude").exists(),
                            created: !existed,
                            file: file.clone(),
                        },
                        contents,
                    ));
                    files.len() - 1
                }
            };
            let section = section(mv.kind);
            let contents = &mut files[i].1;
            m.local.push(LocalChange {
                file: file.clone(),
                section: section.into(),
                key: mv.name.clone(),
                previous: object_entry(contents, section)?.get(&mv.name).cloned(),
            });
            object_entry(contents, section)?.insert(mv.name.clone(), on_value(mv.kind));
        }
    }
    m.local_files = files.iter().map(|(f, _)| clone_file(f)).collect();
    // A settings.local.json made by hand is not ignored by git the way one
    // Claude Code creates is, so keep the new ones out of commits.
    for (f, _) in files.iter().filter(|(f, _)| f.created) {
        let Some(project) = f.file.parent().and_then(Path::parent) else {
            continue;
        };
        if let Some(g) = git_exclude_for(project) {
            if !m.git_excludes.iter().any(|x| x.file == g.file) {
                m.git_excludes.push(g);
            }
        }
    }

    // The manifest goes first so a half-finished run can still be undone.
    write_manifest(&dir, &m)?;

    for g in &m.git_excludes {
        add_line(&g.file, &g.line)?;
    }
    // Turn things on where they are used before turning them off everywhere.
    for (f, contents) in &files {
        if let Some(parent) = f.file.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        write_json_atomic(&f.file, contents)?;
    }
    for c in &m.plugins {
        enabled_plugins(&mut settings)?.insert(c.key.clone(), Value::Bool(false));
    }
    for c in &m.skill_overrides {
        object_entry(&mut settings, OVERRIDES)?.insert(c.key.clone(), Value::String(HIDDEN.into()));
    }
    write_json_atomic(&settings_path, &settings)?;
    Ok(dir)
}

const EXCLUDE_LINE: &str = "**/.claude/settings.local.json";

/// When `project` is in a git repository that would not ignore
/// `.claude/settings.local.json`, the repository's `info/exclude` file and
/// the line that fixes it. Without git, or outside a repository, nothing.
fn git_exclude_for(project: &Path) -> Option<GitExclude> {
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(project)
            .args(args)
            .output()
            .ok()
    };
    // 0: already ignored, 1: not ignored, anything else: not a repository.
    let checked = git(&["check-ignore", "-q", ".claude/settings.local.json"])?;
    if checked.status.code() != Some(1) {
        return None;
    }
    let out = git(&["rev-parse", "--git-path", "info/exclude"])?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8(out.stdout).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let file = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        project.join(raw)
    };
    let present = fs::read_to_string(&file)
        .map(|t| t.lines().any(|l| l.trim() == EXCLUDE_LINE))
        .unwrap_or(false);
    (!present).then(|| GitExclude {
        file,
        line: EXCLUDE_LINE.into(),
    })
}

fn add_line(file: &Path, line: &str) -> Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = fs::read_to_string(file).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(line);
    text.push('\n');
    fs::write(file, text).with_context(|| format!("cannot update {}", file.display()))
}

fn clone_file(f: &LocalFile) -> LocalFile {
    LocalFile {
        file: f.file.clone(),
        created: f.created,
        created_dir: f.created_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_config_dirs_are_not_projects() {
        let paths = Paths {
            claude_dir: PathBuf::from("/home/u/.claude"),
            global_config: PathBuf::from("/home/u/.claude.json"),
            cwd: PathBuf::from("/home/u/code"),
        };
        assert!(!writable_project(Path::new("/home/u/.claude"), &paths));
        assert!(!writable_project(
            Path::new("/home/u/.claude/plugins"),
            &paths
        ));
        assert!(writable_project(Path::new("/home/u/code/api"), &paths));
        assert!(writable_project(Path::new("/home/u/.claudette"), &paths));
        assert!(!writable_project(Path::new("/"), &paths));
    }
}
