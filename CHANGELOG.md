# Changelog

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
