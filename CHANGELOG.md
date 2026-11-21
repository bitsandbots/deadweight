# Changelog

## 0.5.0

- New: `deadweight scope`. Plugins and user skills you use in only a few projects are turned off in your user settings and kept on in each of those projects' `.claude/settings.local.json`, so they stop loading everywhere else. Backed up and reverted by `deadweight undo` like `fix`; the plain audit and `--json` (`scope`) show what it would do.
- Usage is now counted per project for plugins and skills too, not only MCP servers. The same folder recorded as `C:\x`, `C:/x/` or `/c/x` (Git Bash), or in a different letter case on Windows, counts as one project.
- For `scope`, plugin hooks are told apart: hooks that only add context at session start or react to the plugin's own skills don't hold a plugin back; hooks on every edit, shell command or stop still need `--include-hooked`. (`fix` still skips every unused plugin with hooks, since a hook can be the whole point of a plugin you never call.)
- A `settings.local.json` that `scope` creates is kept out of git (`.git/info/exclude`) unless git already ignores it, as Claude Code does for the files it creates.
- `--json`: `analysis.scope_hints` entries now have `kind`, `name`, `label`, `idle_projects`, `idle_sessions`, `sessions` and `work_hooks`; `server` is replaced by `name`.

## 0.4.0

- New: `deadweight card` saves a shareable PNG of your numbers: the total every request carries, your setup's share of it, unused skills, the biggest category, and what `fix` would save. Totals only: no plugin, skill or server names. `--output FILE.svg` writes the SVG instead. Fonts are bundled, so it looks the same everywhere.

## 0.3.0

- `fix` now hides unused user skills with Claude Code's own `skillOverrides` setting (`"user-invocable-only"`) instead of moving their folders. Claude stops seeing them, `/name` still runs them, and nothing on disk moves. `--park-skills` keeps the old behaviour.
- `fix` turns off claude.ai connectors in Claude Code (`disableClaudeAiConnectors`) when none of them was used in the window. `--keep connectors` skips it. If you use some of them, the report tells you which ones to switch off in `/mcp`.
- When skill descriptions are dropped, the report says how to make room: hide unused skills, or raise `skillListingBudgetFraction`, with what the extra room costs per request.
- `undo` restores all of the above, and still reads backups made by 0.1 and 0.2.

## 0.2.0

- New: user-scope MCP servers that are only used in one or two projects are listed with those projects, so you can move them out of the global config. Usage is counted per project from the session transcripts. Suggested by a reader on dev.to.

## 0.1.0

First release.

- Measures what every Claude Code request carries before you type, from the API usage recorded in your latest session.
- Attributes skill listings, deferred tool names, MCP instructions, agent listings and hook injections to the plugin, skill, MCP server or connector that added them.
- Counts real usage of skills, MCP tools, agents and slash commands across all sessions in the window, including subagents.
- Detects skills listed without a description because the skill listing is full, failed and never-authenticated MCP servers, duplicate skill names, and very large skill descriptions.
- `fix` disables unused plugins and parks unused user skills, with a backup and manifest; `undo` reverts it.
- `--json` output and a Claude Code plugin with a `/deadweight` command.
