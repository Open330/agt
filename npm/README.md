# @open330/agt

Cross-platform Rust CLI for managing skills, personas, hooks, and agent teams
for Claude Code and Codex.

The CLI and npm platform packages are maintained in
[`Open330/agt`](https://github.com/Open330/agt). Skill content is maintained
separately in
[`jiunbae/agent-skills`](https://github.com/jiunbae/agent-skills).

## Install

```bash
npm install --global @open330/agt
agt --version
```

Supported platforms:

- macOS Apple Silicon
- Linux x64
- Linux ARM64

## Install Core Skills

```bash
# Claude
agt skill install --profile core \
  --from jiunbae/agent-skills --global

# Codex
agt skill install --profile core \
  --from jiunbae/agent-skills --global --agent codex
```

Claude skills retain their group layout under `~/.claude/skills`. Codex user
skills are installed flat under `~/.agents/skills`.

## Common Commands

```bash
agt skill list --installed --agent claude
agt skill list --installed --agent codex
agt skill update
agt skill update --agent codex
agt persona install --global --from jiunbae/agent-skills
agt run "review this change"
```

Run `agt --help` or `agt <command> --help` for the full CLI reference.

## License

MIT
