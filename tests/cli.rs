//! End-to-end tests against a synthetic Claude Code config directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(sessions: usize) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("deadweight-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let claude = root.join("claude");
        let work = root.join("work");
        fs::create_dir_all(&work).unwrap();

        write(
            &claude.join("settings.json"),
            &json!({
                "theme": "dark",
                "enabledPlugins": { "bloat@mkt": true, "handy@mkt": true, "hooky@mkt": true }
            }),
        );

        let cache = claude.join("plugins").join("cache").join("mkt");
        let mut installed = serde_json::Map::new();
        for name in ["bloat", "handy", "hooky"] {
            let dir = cache.join(name);
            fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
            write(
                &dir.join(".claude-plugin").join("plugin.json"),
                &json!({ "name": name }),
            );
            installed.insert(
                format!("{name}@mkt"),
                json!([{ "scope": "user", "installPath": dir }]),
            );
        }
        write(
            &cache.join("hooky").join("hooks").join("hooks.json"),
            &json!({ "hooks": { "SessionStart": [ { "hooks": [ { "type": "command", "command": "hooky-start" } ] } ] } }),
        );
        write(
            &claude.join("plugins").join("installed_plugins.json"),
            &json!({ "version": 2, "plugins": installed }),
        );

        for skill in ["old-skill", "used-skill"] {
            let dir = claude.join("skills").join(skill);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("SKILL.md"), format!("---\nname: {skill}\n---\n")).unwrap();
        }

        write(
            &claude.join(".claude.json"),
            &json!({ "mcpServers": { "idle-srv": { "command": "x", "env": { "TOKEN": "s3cr3t-value" } } } }),
        );

        let project = claude.join("projects").join("demo");
        fs::create_dir_all(&project).unwrap();
        let start = [
            attachment(json!({
                "type": "hook_success", "hookEvent": "SessionStart",
                "command": "hooky-start", "content": "x".repeat(720), "stdout": ""
            })),
            attachment(json!({
                "type": "deferred_tools_delta",
                "addedNames": ["CronCreate", "mcp__plugin_bloat_api__fetch", "mcp__plugin_bloat_api__push", "mcp__idle-srv__query", "mcp__claude_ai_Gmail__search_threads"],
                "failedMcpServers": [{ "name": "plugin:bloat:broken", "errorCode": "ENOTFOUND", "error": "getaddrinfo ENOTFOUND" }]
            })),
            attachment(json!({
                "type": "skill_listing", "isInitial": true, "skillCount": 5,
                "names": ["bloat:report", "handy:helper", "old-skill", "used-skill", "loop"],
                "content": format!(
                    "- bloat:report: {}\n- handy:helper: helps\n- old-skill: an old skill\n- used-skill\n- loop: built in",
                    "y".repeat(1800)
                )
            })),
            json!({ "type": "assistant", "message": { "role": "assistant", "content": [],
                "usage": { "input_tokens": 3, "cache_creation_input_tokens": 4000, "cache_read_input_tokens": 997 } } }),
        ];
        let usage = [
            tool_use("Skill", json!({ "skill": "handy:helper" })),
            tool_use("Skill", json!({ "skill": "used-skill" })),
        ];
        for i in 0..sessions {
            let mut lines: Vec<String> = start.iter().map(Value::to_string).collect();
            if i == 0 {
                lines.extend(usage.iter().map(Value::to_string));
            }
            fs::write(
                project.join(format!("session-{i}.jsonl")),
                lines.join("\n") + "\n",
            )
            .unwrap();
        }
        Fixture { root }
    }

    fn claude(&self) -> PathBuf {
        self.root.join("claude")
    }

    fn run(&self, args: &[&str]) -> (bool, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_deadweight"))
            .args(args)
            .arg("--config-dir")
            .arg(self.claude())
            .current_dir(self.root.join("work"))
            .env("NO_COLOR", "1")
            .env_remove("CLAUDE_CONFIG_DIR")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        (out.status.success(), text)
    }

    fn json(&self, args: &[&str]) -> Value {
        let mut all = args.to_vec();
        all.push("--json");
        let (ok, text) = self.run(&all);
        assert!(ok, "deadweight failed: {text}");
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("bad json ({e}): {text}"))
    }

    fn settings(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.claude().join("settings.json")).unwrap())
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write(path: &Path, v: &Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

fn attachment(a: Value) -> Value {
    json!({ "type": "attachment", "timestamp": "2026-11-12T10:00:00.000Z", "attachment": a })
}

fn tool_use(name: &str, input: Value) -> Value {
    json!({ "type": "assistant", "message": { "role": "assistant",
        "content": [ { "type": "tool_use", "id": "t", "name": name, "input": input } ] } })
}

fn labels(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| p["key"].as_str().or(p["name"].as_str()).unwrap().to_owned())
        .collect()
}

#[test]
fn audit_measures_and_attributes() {
    let f = Fixture::new(6);
    let v = f.json(&[]);
    let a = &v["analysis"];
    assert_eq!(a["measured_tokens"], 5000);
    assert_eq!(a["sessions"], 6);
    assert_eq!(a["failed_mcp"][0]["name"], "plugin:bloat:broken");
    assert_eq!(a["skills_without_description"], json!(["used-skill"]));
    assert_eq!(a["skill_budget"]["suggested"], 0.02);
    assert_eq!(a["skill_budget"]["configured"], false);

    let bloat = a["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["label"] == "bloat")
        .expect("bloat source");
    assert_eq!(bloat["uses"], 0);
    assert_eq!(bloat["skills"], 1);
    assert_eq!(bloat["tools"], 2);
    // The 1800-character description belongs to bloat, not to the line before.
    assert!(bloat["tokens"].as_f64().unwrap() > 450.0);

    let plan = &v["plan"];
    assert_eq!(labels(&plan["plugins"]), vec!["bloat@mkt"]);
    assert_eq!(labels(&plan["hooked"]), vec!["hooky@mkt"]);
    assert_eq!(labels(&plan["skills"]), vec!["old-skill"]);
    assert_eq!(plan["connectors"]["names"], json!(["claude.ai Gmail"]));
}

#[test]
fn human_report_runs_and_never_prints_secrets() {
    let f = Fixture::new(6);
    let (ok, text) = f.run(&[]);
    assert!(ok, "{text}");
    assert!(text.contains("every request carries 5.0k tokens"), "{text}");
    assert!(text.contains("deadweight fix"), "{text}");
    assert!(!text.contains("s3cr3t-value"));
    let (_, json_text) = f.run(&["--json"]);
    assert!(!json_text.contains("s3cr3t-value"));
}

#[test]
fn fix_then_undo_round_trips() {
    let f = Fixture::new(6);
    let skill = f.claude().join("skills").join("old-skill");

    let v = f.json(&["fix", "--yes"]);
    assert_eq!(v["applied"], true);
    let s = f.settings();
    assert_eq!(s["enabledPlugins"]["bloat@mkt"], false);
    assert_eq!(s["enabledPlugins"]["handy@mkt"], true);
    assert_eq!(s["enabledPlugins"]["hooky@mkt"], true);
    assert_eq!(s["theme"], "dark");
    // Skills are hidden from the model, not moved.
    assert_eq!(
        s["skillOverrides"],
        json!({ "old-skill": "user-invocable-only" })
    );
    assert!(skill.join("SKILL.md").exists());
    assert_eq!(s["disableClaudeAiConnectors"], true);

    let v = f.json(&["undo", "--yes"]);
    assert_eq!(v["restored"], true);
    let s = f.settings();
    assert_eq!(s["enabledPlugins"]["bloat@mkt"], true);
    assert!(s.get("skillOverrides").is_none(), "{s}");
    assert!(s.get("disableClaudeAiConnectors").is_none(), "{s}");
    assert!(skill.join("SKILL.md").exists());

    let v = f.json(&["undo", "--yes"]);
    assert_eq!(v["restored"], false);
}

#[test]
fn fix_respects_keep_and_include_hooked() {
    let f = Fixture::new(6);
    let v = f.json(&["fix", "--dry-run", "--include-hooked", "--keep", "bloat"]);
    assert_eq!(labels(&v["plan"]["plugins"]), vec!["hooky@mkt"]);
    assert_eq!(f.settings()["enabledPlugins"]["bloat@mkt"], true);
}

#[test]
fn fix_refuses_with_too_little_history() {
    let f = Fixture::new(2);
    let (ok, text) = f.run(&["fix", "--yes"]);
    assert!(!ok);
    assert!(text.contains("too little history"), "{text}");
    assert_eq!(f.settings()["enabledPlugins"]["bloat@mkt"], true);
}

#[test]
fn missing_config_dir_is_a_clear_error() {
    let f = Fixture::new(1);
    let out = Command::new(env!("CARGO_BIN_EXE_deadweight"))
        .args(["--config-dir"])
        .arg(f.root.join("nope"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no Claude Code config found"));
}

/// A session started in `cwd` that makes `calls` calls to the idle-srv MCP server.
fn project_session(f: &Fixture, dir: &str, cwd: &str, calls: usize) {
    let mut lines =
        vec![json!({ "type": "user", "cwd": cwd, "message": { "role": "user", "content": "hi" } })];
    for _ in 0..calls {
        let mut t = tool_use("mcp__idle-srv__query", json!({}));
        t["cwd"] = json!(cwd);
        lines.push(t);
    }
    let text: Vec<String> = lines.iter().map(Value::to_string).collect();
    let project = f.claude().join("projects").join(dir);
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("s.jsonl"), text.join("\n") + "\n").unwrap();
}

#[test]
fn user_mcp_used_in_one_project_gets_a_scope_hint() {
    let f = Fixture::new(6);
    project_session(&f, "api", r"C:\code\api", 3);
    project_session(&f, "web", "/home/u/web", 0);

    let v = f.json(&[]);
    let hints = &v["analysis"]["scope_hints"];
    assert_eq!(hints.as_array().unwrap().len(), 1, "{hints}");
    assert_eq!(hints[0]["name"], "idle-srv");
    assert_eq!(hints[0]["kind"], "mcp");
    assert_eq!(hints[0]["used_in"], json!([["C:/code/api", 3]]));
    assert_eq!(hints[0]["projects_total"], 2);

    let (ok, text) = f.run(&[]);
    assert!(ok, "{text}");
    assert!(text.contains("USED IN A FEW PROJECTS"), "{text}");
    assert!(
        text.contains(r#"projects > "C:/code/api" > mcpServers"#),
        "{text}"
    );
    assert!(!text.contains("s3cr3t-value"));
}

#[test]
fn user_mcp_used_everywhere_gets_no_scope_hint() {
    let f = Fixture::new(6);
    project_session(&f, "api", "/home/u/api", 2);
    project_session(&f, "web", "/home/u/web", 1);
    let v = f.json(&[]);
    assert_eq!(v["analysis"]["scope_hints"], json!([]));
}

#[test]
fn park_skills_moves_them_and_undo_brings_them_back() {
    let f = Fixture::new(6);
    let skill = f.claude().join("skills").join("old-skill");
    f.json(&["fix", "--yes", "--park-skills", "--keep", "connectors"]);
    assert!(!skill.exists());
    assert!(f.settings().get("skillOverrides").is_none());
    assert!(f.settings().get("disableClaudeAiConnectors").is_none());
    f.json(&["undo", "--yes"]);
    assert!(skill.join("SKILL.md").exists());
}

#[test]
fn existing_overrides_are_kept_and_restored() {
    let f = Fixture::new(6);
    let mut s = f.settings();
    s["skillOverrides"] = json!({ "some-other": "off" });
    s["disableClaudeAiConnectors"] = json!(false);
    write(&f.claude().join("settings.json"), &s);

    f.json(&["fix", "--yes"]);
    let s = f.settings();
    assert_eq!(s["skillOverrides"]["some-other"], "off");
    assert_eq!(s["skillOverrides"]["old-skill"], "user-invocable-only");
    assert_eq!(s["disableClaudeAiConnectors"], true);

    f.json(&["undo", "--yes"]);
    let s = f.settings();
    assert_eq!(s["skillOverrides"], json!({ "some-other": "off" }));
    assert_eq!(s["disableClaudeAiConnectors"], false);
}

#[test]
fn undo_reads_version_1_manifests() {
    let f = Fixture::new(6);
    let dir = f
        .claude()
        .join("deadweight")
        .join("backups")
        .join("20261112-100000");
    let parked = dir.join("skills").join("old-skill");
    fs::create_dir_all(&parked).unwrap();
    let skill = f.claude().join("skills").join("old-skill");
    fs::rename(&skill, &parked).unwrap();
    write(
        &dir.join("manifest.json"),
        &json!({
            "version": 1, "created": "20261112-100000", "tool_version": "0.2.0",
            "plugins": [{ "key": "bloat@mkt", "previous": true }],
            "skills": [{ "name": "old-skill", "from": skill, "to": parked }],
            "restored": false
        }),
    );
    let mut s = f.settings();
    s["enabledPlugins"]["bloat@mkt"] = json!(false);
    write(&f.claude().join("settings.json"), &s);

    let v = f.json(&["undo", "--yes"]);
    assert_eq!(v["restored"], true);
    assert!(skill.join("SKILL.md").exists());
    assert_eq!(f.settings()["enabledPlugins"]["bloat@mkt"], true);
}

#[test]
fn card_shows_totals_and_no_names() {
    let f = Fixture::new(6);
    let (ok, text) = f.run(&["card"]);
    assert!(ok, "{text}");
    let png = fs::read(f.root.join("work").join("deadweight-card.png")).unwrap();
    assert_eq!(&png[1..4], b"PNG");

    let v = f.json(&["card", "--output", "card.svg"]);
    assert_eq!(v["card"]["measured"], 5000.0);
    let svg = fs::read_to_string(f.root.join("work").join("card.svg")).unwrap();
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("5.0k"));
    for name in [
        "bloat",
        "handy",
        "hooky",
        "old-skill",
        "used-skill",
        "idle-srv",
        "Gmail",
    ] {
        assert!(!svg.contains(name), "card leaks {name}");
    }
}

/// A session started in `<root>/<name>` (created on disk) that makes these tool calls.
fn session_in(f: &Fixture, name: &str, calls: &[Value]) -> PathBuf {
    let dir = f.root.join(name);
    fs::create_dir_all(&dir).unwrap();
    let cwd = dir.to_string_lossy().to_string();
    let mut lines =
        vec![json!({ "type": "user", "cwd": cwd, "message": { "role": "user", "content": "hi" } })];
    lines.extend(calls.iter().cloned());
    let text: Vec<String> = lines.iter().map(Value::to_string).collect();
    let project = f.claude().join("projects").join(format!("p-{name}"));
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("s.jsonl"), text.join("\n") + "\n").unwrap();
    dir
}

/// handy and used-skill are used in `api` only; `web` and `docs` never use them.
fn scoped_fixture() -> (Fixture, PathBuf, PathBuf) {
    let f = Fixture::new(6);
    let api = session_in(
        &f,
        "api",
        &[
            tool_use("Skill", json!({ "skill": "handy:helper" })),
            tool_use("Skill", json!({ "skill": "used-skill" })),
        ],
    );
    let web = session_in(&f, "web", &[]);
    session_in(&f, "docs", &[]);
    (f, api, web)
}

fn local_settings(project: &Path) -> Option<Value> {
    let text = fs::read_to_string(project.join(".claude").join("settings.local.json")).ok()?;
    Some(serde_json::from_str(&text).unwrap())
}

fn names(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn scope_finds_what_is_used_in_few_projects() {
    let (f, _, _) = scoped_fixture();
    let v = f.json(&[]);
    let hints = &v["analysis"]["scope_hints"];
    let found = names(hints);
    assert!(found.contains(&"handy@mkt".to_owned()), "{found:?}");
    assert!(found.contains(&"used-skill".to_owned()), "{found:?}");
    let handy = hints
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "handy@mkt")
        .unwrap();
    assert_eq!(handy["kind"], "plugin");
    assert_eq!(handy["projects_total"], 3);
    assert_eq!(handy["idle_projects"], 2);
    assert_eq!(handy["idle_sessions"], 2);
    assert_eq!(handy["used_in"].as_array().unwrap().len(), 1);

    let (ok, text) = f.run(&[]);
    assert!(ok, "{text}");
    assert!(text.contains("USED IN A FEW PROJECTS"), "{text}");
    assert!(text.contains("deadweight scope"), "{text}");
}

#[test]
fn scope_dry_run_changes_nothing() {
    let (f, api, _) = scoped_fixture();
    let before = f.settings();
    let v = f.json(&["scope", "--dry-run"]);
    assert_eq!(v["applied"], false);
    assert_eq!(names(&v["plan"]["moves"]).len(), 2, "{v}");
    assert_eq!(f.settings(), before);
    assert!(local_settings(&api).is_none());
}

#[test]
fn scope_then_undo_round_trips() {
    let (f, api, web) = scoped_fixture();
    let v = f.json(&["scope", "--yes"]);
    assert_eq!(v["applied"], true, "{v}");

    let s = f.settings();
    assert_eq!(s["enabledPlugins"]["handy@mkt"], false);
    assert_eq!(s["enabledPlugins"]["bloat@mkt"], true);
    assert_eq!(s["skillOverrides"]["used-skill"], "user-invocable-only");
    assert_eq!(s["theme"], "dark");
    assert_eq!(
        local_settings(&api).unwrap(),
        json!({ "enabledPlugins": { "handy@mkt": true }, "skillOverrides": { "used-skill": "on" } })
    );
    assert!(!web.join(".claude").exists());

    let v = f.json(&["undo", "--yes"]);
    assert_eq!(v["restored"], true);
    let s = f.settings();
    assert_eq!(s["enabledPlugins"]["handy@mkt"], true);
    assert!(s.get("skillOverrides").is_none(), "{s}");
    assert!(!api.join(".claude").exists(), "created files are removed");
}

#[test]
fn scope_keeps_what_a_project_already_has() {
    let (f, api, _) = scoped_fixture();
    let original = json!({ "permissions": { "allow": ["Bash(ls)"] } });
    write(&api.join(".claude").join("settings.local.json"), &original);

    f.json(&["scope", "--yes"]);
    let local = local_settings(&api).unwrap();
    assert_eq!(local["permissions"], original["permissions"]);
    assert_eq!(local["enabledPlugins"]["handy@mkt"], true);

    f.json(&["undo", "--yes"]);
    assert_eq!(local_settings(&api).unwrap(), original);
}

#[test]
fn scope_respects_keep() {
    let (f, _, _) = scoped_fixture();
    let v = f.json(&["scope", "--dry-run", "--keep", "handy"]);
    assert_eq!(names(&v["plan"]["moves"]), vec!["used-skill"]);
}

/// hooky gets used in one more project, next to two that never use it.
fn hooky_in_one_project(f: &Fixture) {
    session_in(
        f,
        "api2",
        &[tool_use("Skill", json!({ "skill": "hooky:thing" }))],
    );
    session_in(f, "site", &[]);
}

#[test]
fn scope_treats_session_start_hooks_as_safe() {
    let (f, _, _) = scoped_fixture();
    hooky_in_one_project(&f);
    // The fixture's hooky only adds context at session start.
    let v = f.json(&["scope", "--dry-run"]);
    assert!(
        names(&v["plan"]["moves"]).contains(&"hooky@mkt".to_owned()),
        "{v}"
    );
}

#[test]
fn scope_leaves_plugins_with_work_hooks_alone() {
    let (f, _, _) = scoped_fixture();
    hooky_in_one_project(&f);
    // A hook on every edit acts on your work in any project.
    write(
        &f.claude()
            .join("plugins")
            .join("cache")
            .join("mkt")
            .join("hooky")
            .join("hooks")
            .join("hooks.json"),
        &json!({ "hooks": {
            "SessionStart": [ { "hooks": [ { "type": "command", "command": "hooky-start" } ] } ],
            "PostToolUse": [ { "matcher": "Edit|Write", "hooks": [ { "type": "command", "command": "hooky-lint" } ] } ]
        } }),
    );
    let v = f.json(&["scope", "--dry-run"]);
    assert_eq!(names(&v["plan"]["hooked"]), vec!["hooky@mkt"]);
    let v = f.json(&["scope", "--dry-run", "--include-hooked"]);
    assert!(
        names(&v["plan"]["moves"]).contains(&"hooky@mkt".to_owned()),
        "{v}"
    );
}

#[test]
fn scope_refuses_with_too_little_history() {
    let f = Fixture::new(1);
    session_in(
        &f,
        "api",
        &[tool_use("Skill", json!({ "skill": "handy:helper" }))],
    );
    session_in(&f, "web", &[]);
    let (ok, text) = f.run(&["scope", "--yes"]);
    assert!(!ok);
    assert!(text.contains("too little history"), "{text}");
    assert_eq!(f.settings()["enabledPlugins"]["handy@mkt"], true);
}

#[test]
fn scope_keeps_created_files_out_of_git() {
    let (f, api, _) = scoped_fixture();
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&api)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
    };
    if git(&["init", "-q"]).is_none() {
        eprintln!("git not available; skipping");
        return;
    }
    // Claude Code may already have added the pattern to the global excludes.
    let ignored_globally = git(&["check-ignore", "-q", ".claude/settings.local.json"]).is_some();
    f.json(&["scope", "--yes"]);
    let exclude = api.join(".git").join("info").join("exclude");
    let text = fs::read_to_string(&exclude).unwrap_or_default();
    assert_eq!(
        text.lines().any(|l| l == "**/.claude/settings.local.json"),
        !ignored_globally,
        "{text}"
    );
    let status = git(&["status", "--porcelain", "--untracked-files=all"]).unwrap();
    let status = String::from_utf8_lossy(&status.stdout).to_string();
    assert!(!status.contains("settings.local.json"), "{status}");

    f.json(&["undo", "--yes"]);
    let text = fs::read_to_string(&exclude).unwrap_or_default();
    assert!(!text.contains("settings.local.json"), "{text}");
}
