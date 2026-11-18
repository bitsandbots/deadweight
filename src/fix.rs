//! `deadweight fix` / `deadweight undo`.
//!
//! Every change is reversible: before touching anything, deadweight writes a
//! manifest and a copy of `settings.json` to
//! `~/.claude/deadweight/backups/<timestamp>/`. Plugins are disabled (never
//! uninstalled), user skills are hidden from the model with `skillOverrides`
//! (or, with `--park-skills`, moved into the backup folder) and unused
//! claude.ai connectors are switched off with `disableClaudeAiConnectors`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::analyze::{Analysis, Source};
use crate::inventory::Inventory;
use crate::paths::Paths;
use crate::util::timestamp_now;

/// The `skillOverrides` value that hides a skill from the model but keeps `/name`.
pub const HIDDEN: &str = "user-invocable-only";
const OVERRIDES_KEY: &str = "skillOverrides";
const CONNECTORS_KEY: &str = "disableClaudeAiConnectors";

#[derive(Debug, Default, Clone)]
pub struct PlanOptions {
    pub include_hooked: bool,
    pub keep: Vec<String>,
    /// Move unused skills into the backup folder instead of hiding them.
    pub park_skills: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedPlugin {
    pub key: String,
    pub label: String,
    pub tokens: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedSkill {
    pub name: String,
    pub path: PathBuf,
    pub tokens: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedConnectors {
    pub names: Vec<String>,
    pub tokens: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Plan {
    pub plugins: Vec<PlannedPlugin>,
    pub skills: Vec<PlannedSkill>,
    /// `true`: skills are moved into the backup; `false`: hidden with `skillOverrides`.
    pub park_skills: bool,
    /// Every claude.ai connector, when none of them was used.
    pub connectors: Option<PlannedConnectors>,
    /// Unused plugins left alone because they run hooks.
    pub hooked: Vec<PlannedPlugin>,
    pub tokens: f64,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty() && self.skills.is_empty() && self.connectors.is_none()
    }
}

pub fn plan(a: &Analysis, inv: &Inventory, opts: &PlanOptions) -> Plan {
    let kept = |label: &str, key: &str| {
        opts.keep
            .iter()
            .any(|k| k.eq_ignore_ascii_case(label) || k.eq_ignore_ascii_case(key))
    };
    let mut p = Plan {
        park_skills: opts.park_skills,
        ..Plan::default()
    };
    for s in a.sources.iter().filter(|s| s.uses == 0 && s.tokens > 0.0) {
        match &s.source {
            Source::Plugin(key) => {
                let enabled = inv
                    .plugins
                    .iter()
                    .any(|pl| &pl.key == key && pl.enabled_by_user);
                if !enabled || kept(&s.label, key) {
                    continue;
                }
                let planned = PlannedPlugin {
                    key: key.clone(),
                    label: s.label.clone(),
                    tokens: s.tokens,
                };
                if s.runs_hooks && !opts.include_hooked {
                    p.hooked.push(planned);
                } else {
                    p.plugins.push(planned);
                }
            }
            Source::UserSkill(name) => {
                if kept(name, name) || inv.skill_overrides.contains_key(name) {
                    continue;
                }
                if let Some(path) = inv.user_skills.get(name) {
                    p.skills.push(PlannedSkill {
                        name: name.clone(),
                        path: path.clone(),
                        tokens: s.tokens,
                    });
                }
            }
            _ => {}
        }
    }

    let connectors: Vec<_> = a
        .sources
        .iter()
        .filter(|s| matches!(s.source, Source::Connector(_)))
        .collect();
    if !connectors.is_empty()
        && connectors.iter().all(|s| s.uses == 0)
        && !inv.connectors_disabled
        && !kept("connectors", "claude.ai")
    {
        p.connectors = Some(PlannedConnectors {
            names: connectors.iter().map(|s| s.label.clone()).collect(),
            tokens: connectors.iter().map(|s| s.tokens).sum(),
        });
    }

    p.tokens = p.plugins.iter().map(|x| x.tokens).sum::<f64>()
        + p.skills.iter().map(|x| x.tokens).sum::<f64>()
        + p.connectors.as_ref().map_or(0.0, |c| c.tokens);
    p
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginChange {
    pub key: String,
    /// Value of `enabledPlugins[key]` before the change; `None` = absent.
    pub previous: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkillMove {
    pub name: String,
    pub from: PathBuf,
    pub to: PathBuf,
}

/// A `settings.json` value deadweight set, and what was there before.
#[derive(Debug, Serialize, Deserialize)]
pub struct SettingChange {
    pub key: String,
    /// `None` = absent.
    pub previous: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub created: String,
    pub tool_version: String,
    pub plugins: Vec<PluginChange>,
    pub skills: Vec<SkillMove>,
    /// `skillOverrides` entries (manifest version 2 and later).
    #[serde(default)]
    pub skill_overrides: Vec<SettingChange>,
    /// `disableClaudeAiConnectors` (manifest version 2 and later).
    #[serde(default)]
    pub connectors: Option<SettingChange>,
    pub restored: bool,
}

impl Manifest {
    fn touches_settings(&self) -> bool {
        !self.plugins.is_empty() || !self.skill_overrides.is_empty() || self.connectors.is_some()
    }
}

const MANIFEST: &str = "manifest.json";

/// Apply a plan. Returns the backup directory.
pub fn apply(plan: &Plan, paths: &Paths) -> Result<PathBuf> {
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

    let previous_override = |name: &str| {
        settings
            .get(OVERRIDES_KEY)
            .and_then(|o| o.get(name))
            .cloned()
    };
    let (moved, hidden): (Vec<&PlannedSkill>, Vec<&PlannedSkill>) =
        plan.skills.iter().partition(|_| plan.park_skills);
    let skill_overrides = hidden
        .iter()
        .map(|s| SettingChange {
            key: s.name.clone(),
            previous: previous_override(&s.name),
        })
        .collect();
    let connectors = plan.connectors.as_ref().map(|_| SettingChange {
        key: CONNECTORS_KEY.into(),
        previous: settings.get(CONNECTORS_KEY).cloned(),
    });
    let enabled = enabled_plugins(&mut settings)?;
    let manifest = Manifest {
        version: 2,
        created: timestamp_now(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
        plugins: plan
            .plugins
            .iter()
            .map(|p| PluginChange {
                key: p.key.clone(),
                previous: enabled.get(&p.key).cloned(),
            })
            .collect(),
        skills: moved
            .iter()
            .map(|s| SkillMove {
                name: s.name.clone(),
                from: s.path.clone(),
                to: dir.join("skills").join(&s.name),
            })
            .collect(),
        skill_overrides,
        connectors,
        restored: false,
    };
    // The manifest goes first so a half-finished run can still be undone.
    write_manifest(&dir, &manifest)?;

    for c in &manifest.plugins {
        enabled.insert(c.key.clone(), Value::Bool(false));
    }
    if !manifest.skill_overrides.is_empty() {
        let overrides = object_entry(&mut settings, OVERRIDES_KEY)?;
        for c in &manifest.skill_overrides {
            overrides.insert(c.key.clone(), Value::String(HIDDEN.into()));
        }
    }
    if manifest.connectors.is_some() {
        root(&mut settings)?.insert(CONNECTORS_KEY.into(), Value::Bool(true));
    }
    if manifest.touches_settings() {
        write_json_atomic(&settings_path, &settings)?;
    }

    if !manifest.skills.is_empty() {
        fs::create_dir_all(dir.join("skills"))?;
    }
    for m in &manifest.skills {
        fs::rename(&m.from, &m.to)
            .with_context(|| format!("cannot move {} to the backup", m.from.display()))?;
    }
    Ok(dir)
}

/// The newest backup that has not been restored yet.
pub fn latest_backup(paths: &Paths) -> Result<Option<(PathBuf, Manifest)>> {
    let Ok(entries) = fs::read_dir(paths.backups()) else {
        return Ok(None);
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(MANIFEST).is_file())
        .collect();
    dirs.sort();
    for dir in dirs.into_iter().rev() {
        let text = fs::read_to_string(dir.join(MANIFEST))?;
        let m: Manifest = serde_json::from_str(&text)
            .with_context(|| format!("unreadable manifest in {}", dir.display()))?;
        if !m.restored {
            return Ok(Some((dir, m)));
        }
    }
    Ok(None)
}

/// Revert one backup. Skips anything that was changed back by hand.
pub fn undo(paths: &Paths, dir: &Path, mut m: Manifest) -> Result<Manifest> {
    if m.touches_settings() {
        let settings_path = paths.settings();
        let mut settings = read_settings(&settings_path)?;
        if !m.plugins.is_empty() {
            let enabled = enabled_plugins(&mut settings)?;
            for c in &m.plugins {
                restore(enabled, &c.key, &c.previous);
            }
        }
        if !m.skill_overrides.is_empty() {
            let overrides = object_entry(&mut settings, OVERRIDES_KEY)?;
            for c in &m.skill_overrides {
                restore(overrides, &c.key, &c.previous);
            }
            if overrides.is_empty() {
                root(&mut settings)?.remove(OVERRIDES_KEY);
            }
        }
        if let Some(c) = &m.connectors {
            restore(root(&mut settings)?, CONNECTORS_KEY, &c.previous);
        }
        write_json_atomic(&settings_path, &settings)?;
    }
    for s in &m.skills {
        if s.to.exists() && !s.from.exists() {
            if let Some(parent) = s.from.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&s.to, &s.from)
                .with_context(|| format!("cannot restore {}", s.from.display()))?;
        }
    }
    m.restored = true;
    write_manifest(dir, &m)?;
    Ok(m)
}

fn restore(map: &mut Map<String, Value>, key: &str, previous: &Option<Value>) {
    match previous {
        Some(v) => {
            map.insert(key.to_owned(), v.clone());
        }
        None => {
            map.remove(key);
        }
    }
}

fn read_settings(path: &Path) -> Result<Value> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .with_context(|| format!("{} is not valid JSON; not touching it", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn root(settings: &mut Value) -> Result<&mut Map<String, Value>> {
    settings
        .as_object_mut()
        .context("settings.json is not a JSON object")
}

fn object_entry<'a>(settings: &'a mut Value, key: &str) -> Result<&'a mut Map<String, Value>> {
    root(settings)?
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .with_context(|| format!("{key} in settings.json is not an object"))
}

fn enabled_plugins(settings: &mut Value) -> Result<&mut Map<String, Value>> {
    object_entry(settings, "enabledPlugins")
}

fn write_manifest(dir: &Path, m: &Manifest) -> Result<()> {
    write_json_atomic(&dir.join(MANIFEST), &serde_json::to_value(m)?)
}

fn write_json_atomic(path: &Path, v: &Value) -> Result<()> {
    let mut text = serde_json::to_string_pretty(v)?;
    text.push('\n');
    let tmp = path.with_extension("json.deadweight-tmp");
    fs::write(&tmp, text).with_context(|| format!("cannot write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}
