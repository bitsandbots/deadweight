---
description: Audit what this setup costs per request
allowed-tools: Bash(deadweight:*)
argument-hint: "[fix|scope|undo|card]"
---

Run `deadweight --json` with the Bash tool. If the command is missing, tell the user to install it with `cargo install deadweight` and stop.

From the JSON, report briefly:

1. `analysis.measured_tokens`: what every request carries before the user types, and how much of it is their setup (`analysis.setup_tokens`).
2. The five biggest entries in `analysis.sources` that are not built-in, with their `uses` count.
3. Anything under `skills_without_description`, `failed_mcp`, `unauthenticated` and `duplicates`.
4. What `plan` would remove and how many tokens it saves, plus anything in `analysis.manual`.
5. What `scope.moves` would load only in the projects that use it, and the tokens per request it saves elsewhere (`scope.tokens`).

If the user passed `fix`, first run `deadweight fix --dry-run`, show the plan, and only run `deadweight fix --yes` after they confirm. If they passed `scope`, first run `deadweight scope --dry-run`, show the plan, and only run `deadweight scope --yes` after they confirm. If they passed `undo`, run `deadweight undo --yes`. If they passed `card`, run `deadweight card` and tell them where the PNG was saved. Remind them to restart Claude Code afterwards.

Arguments: $ARGUMENTS
