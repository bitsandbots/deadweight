# Contributing to deadweight

Thanks for helping. deadweight is small (about 2,600 lines of Rust), so you can read the whole thing in an evening.

## Where to start

- Issues labelled [`good first issue`](https://github.com/msertdev/deadweight/labels/good%20first%20issue) are scoped, have a pointer to the code involved, and don't need deep knowledge of Claude Code internals.
- [`help wanted`](https://github.com/msertdev/deadweight/labels/help%20wanted) issues are bigger or need a decision. Comment first so we can agree on the shape.
- Found a misattribution in your own setup? That's one of the most useful reports you can send. See [Reporting a wrong number](#reporting-a-wrong-number).

Comment on an issue before you start so nobody duplicates work. If you go quiet for two weeks, someone else may pick it up.

## Building and testing

You need Rust 1.85 or newer.

```sh
cargo build
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

CI runs the last three on Linux, macOS and Windows. A PR needs all three green.

Run it against your own setup with `cargo run`, or against a copy with `cargo run -- --config-dir /path/to/claude`.

## How the code is laid out

| file | what it does |
| --- | --- |
| `src/main.rs` | CLI (clap), wires the steps together |
| `src/paths.rs` | finds `~/.claude`, `CLAUDE_CONFIG_DIR`, `.claude.json` |
| `src/inventory.rs` | what is installed: plugins, skills, agents, MCP server names, hooks |
| `src/transcript.rs` | reads `~/.claude/projects/**/*.jsonl` in parallel: the latest startup snapshot and usage counts |
| `src/analyze.rs` | turns inventory + transcripts into sources, token estimates and diagnostics |
| `src/report.rs` | the human-readable output |
| `src/fix.rs` | `fix` and `undo`: plan, back up, apply, revert |
| `tests/cli.rs` | end-to-end tests against a fake `~/.claude` built in a temp dir |

New behaviour should come with a test in `tests/cli.rs`. The `Fixture` there builds a synthetic config directory with plugins, skills, an MCP server and session transcripts; extend it rather than reading real files.

## Ground rules

These are what make deadweight safe to run on someone's machine. PRs that break them won't be merged.

1. **No network.** deadweight never makes a request.
2. **No secrets.** From `.claude.json` only server *names* are read, never commands, env vars or headers. There is a test for this; keep it passing.
3. **Read-only by default.** Only `fix` and `undo` write, and both ask first.
4. **Nothing is deleted.** Every change `fix` makes is recorded in a backup manifest and reversible with `undo`.
5. **Measured beats guessed.** If a number is an estimate, the output should say so.
6. **Pure Rust.** No shell, Python or JS scripts in the repo; plugin files are Markdown and JSON only.

## Reporting a wrong number

Run `deadweight --json`, delete anything private (project paths, plugin or server names you'd rather not share), and paste the relevant part into an issue along with what you expected. Your Claude Code version (`claude --version`) helps too.

## Pull requests

- Keep a PR to one change. Small PRs get reviewed the same day, usually.
- Add a line to `CHANGELOG.md` under an `Unreleased` heading.
- Use a short imperative commit title (`feat: ...`, `fix: ...`, `docs: ...`).

By contributing you agree that your work is released under the MIT license.
