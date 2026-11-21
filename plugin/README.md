# deadweight

Every plugin, skill, MCP server and hook you install adds text to every request Claude Code sends, before you type a word. This plugin adds a `/deadweight` command that asks Claude to audit that overhead with the [deadweight](https://github.com/msertdev/deadweight) command-line tool and explain it to you.

## Install

```text
/plugin install deadweight --marketplace msertdev/deadweight
```

## What `/deadweight` shows

- What every request carries before you type, measured from the API usage recorded in your latest session, and how much of it is your setup
- The biggest sources, traced to the plugin, skill, MCP server or connector that added them, with how often you used each one in the last 30 days
- Skills listed without a description because the skill listing is full, MCP servers that failed or never signed in, and duplicate skill names
- What `deadweight fix` and `deadweight scope` would change, and how many tokens per request that saves

## Arguments

- `/deadweight`: the audit
- `/deadweight fix`: shows the plan, and turns off what you never use after you confirm
- `/deadweight scope`: shows the plan, and keeps plugins and skills that you only use in a few projects on just there after you confirm
- `/deadweight undo`: reverts the latest change
- `/deadweight card`: saves a PNG with your totals to share (no plugin or skill names on it)

Every change is backed up first and `/deadweight undo` puts it back. Restart Claude Code after a change to load the new setup.

## Requirements

The command runs the `deadweight` program, which this plugin does not include. Get it from the [releases page](https://github.com/msertdev/deadweight/releases) (macOS, Linux and Windows), or see the [install section of the project README](https://github.com/msertdev/deadweight#install). Claude asks for your permission before it runs it.

## Privacy

deadweight runs only on your machine and makes no network requests. It reads the session transcripts and settings Claude Code keeps in your Claude Code config folder, to count which plugins, skills, MCP servers and agents you used. It never reads MCP server commands, environment variables or headers, and it doesn't send, upload or share anything. The only files it writes are the settings changes you confirm and their backups, in your Claude Code config folder and, for `scope`, in the `.claude/settings.local.json` of your projects. Delete the backups folder at any time to remove them.

## Support

Questions and bug reports: [GitHub issues](https://github.com/msertdev/deadweight/issues).

## License

MIT
