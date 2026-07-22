# agt

`agt` is a Rust CLI for installing and running skills, personas, hooks, and
multi-agent workflows across Claude Code and Codex.

This repository owns only the CLI, npm packages, platform binaries, and
release automation. The maintained skill catalog lives in
[`jiunbae/agent-skills`](https://github.com/jiunbae/agent-skills).

## Install

```bash
npm install --global @open330/agt
agt --version
```

Supported npm platforms:

- macOS Apple Silicon (`darwin-arm64`)
- Linux x64 (`linux-x64`)
- Linux ARM64 (`linux-arm64`)

The bootstrap script installs the same published npm package:

```bash
curl -fsSL https://raw.githubusercontent.com/Open330/agt/main/setup.sh | bash
```

To install the Core profile for both Claude and Codex in one step:

```bash
curl -fsSL https://raw.githubusercontent.com/Open330/agt/main/setup.sh \
  | bash -s -- --core --codex
```

## Install Skills

```bash
# Claude (grouped layout under ~/.claude/skills)
agt skill install --profile core \
  --from jiunbae/agent-skills --global

# Codex (flat layout under ~/.agents/skills)
agt skill install --profile core \
  --from jiunbae/agent-skills --global --agent codex

agt skill list --installed --agent claude
agt skill list --installed --agent codex
agt skill update --agent codex
```

Remote installs write `.remote-source` metadata so `agt skill update` can
refresh them later. Repository `agt.toml` setup rules merge static context
without replacing existing user files.

## Source Discovery

Commands that need a local skills library use this priority:

1. `AGT_DIR` or `AGENT_SKILLS_DIR`
2. A skills source near the resolved executable
3. `~/.agent-skills`, then legacy `~/.agt` and `~/agt`
4. The current Git repository when offered by the interactive installer

Recommended local setup:

```bash
git clone https://github.com/jiunbae/agent-skills ~/workspace/agent-skills
ln -s ~/workspace/agent-skills ~/.agent-skills
```

## Main Commands

```text
agt skill        Manage Claude/Codex skills
agt persona      Install and use reviewer personas
agt hook         Manage Claude Code hooks
agt team         Run coordinated agent teams
agt run          Run a prompt with automatic skill matching
agt completions  Generate shell completions
```

Use `agt <command> --help` for command-specific options.

## Development

```bash
cargo test --locked --manifest-path agt/Cargo.toml
cargo build --release --manifest-path agt/Cargo.toml
node npm/scripts/verify-platform-packages.js
```

Repository ownership rules:

- CLI changes and npm releases belong here.
- Skill content changes belong in `jiunbae/agent-skills`.
- This repository must not contain or publish a duplicated skill catalog.

## Releases

The tracked Gitea Actions workflow builds and publishes the three active
platform packages, publishes the wrapper package, verifies registry versions,
and uploads matching GitHub Release tarballs. Release versions use the
`vYYYY.M.D` tag format.

## License

MIT
