# agt

[![CI](https://github.com/Open330/agt/actions/workflows/ci.yml/badge.svg)](https://github.com/Open330/agt/actions/workflows/ci.yml) [![npm](https://img.shields.io/npm/v/%40open330%2Fagt?logo=npm)](https://www.npmjs.com/package/@open330/agt) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

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
# Claude (flat layout under ~/.claude/skills, or $CLAUDE_CONFIG_DIR/skills)
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
without replacing existing user files. Pass `--no-static` to skip them.

Claude Code loads only `<skills-dir>/<skill>/SKILL.md`. Skills that older agt
versions installed as `<group>/<skill>` are invisible to it; move them with:

```bash
agt skill migrate --global --dry-run   # preview
agt skill migrate --global
```

`agt skill status --global` shows each installed skill with the profile it came
from (recorded in `agt-state.json` next to the skills directory) and flags
entries agt did not install.

Use `--claude-dir <dir>` (or `CLAUDE_CONFIG_DIR`) to target a Claude config
directory other than `~/.claude`; it applies to skills, hooks, teams and
`settings.json`.

## Profiles

A skills repository defines profiles in `profiles.yml` (and any other root
`*.yml`):

```yaml
core:
  description: "Essential skills"
  skills: [development/git-commit-pr, security/security-auditor]

full:
  extends: core          # or a list: [core, dev]
  groups: [agents, development]
```

`agt skill install --profile core,full` installs the union of several
profiles; `all` is every skill in the repository.

## Layers

`agt apply` makes skill directories match a private, per-machine file,
`~/.config/agt/layers.toml` (override with `AGT_LAYERS`). A **layer** is one
profile from one source; a **stack** is an ordered list of layers; a
**target** applies a stack to the global skill directory or to a directory
tree.

```toml
[sources]
personal = "~/workspace/agent-skills"
team     = "~/work/agents"

[stack.base]
layers = [
  { source = "personal", profile = "core,dev" },
  { source = "personal", skills = ["integrations/vault-secrets"] },  # one-off picks
]
static = ["personal"]            # run this source's [[setup.copy]] on the global target

[stack.work]
extends = "base"
layers  = [{ source = "team", profile = "team-core" }]

[[target]]
path  = "global"
stack = "base"

[[target]]
path  = "~/work"                 # its git repos get the work stack
stack = "work"
```

```bash
agt apply --dry-run      # show the plan
agt apply                # link, adopt, prune
agt apply --check        # exit 1 if anything would change
```

- Claude Code reads project skills only from the directory a session starts
  in, so a directory target is installed into `<dir>/.claude/skills` **and**
  each git repository directly under it; the links are added to that repo's
  `.git/info/exclude`.
- `apply` only removes skills it installed itself (recorded in
  `agt-state.json`). A name already taken by something else is skipped with a
  warning, never overwritten.
- Two layers providing the same skill name is an error unless the later layer
  sets `override = true`.

Hooks are read from user settings and the session's own directory, never from
parent directories. To limit a global hook to one tree, wrap its command:

```json
{ "type": "command", "command": "agt gate ~/work -- ~/work/agents/scripts/digest.sh" }
```

`agt gate` runs the command only when `$CLAUDE_PROJECT_DIR` is inside the
directory and otherwise exits 0 silently.

## Declarative Environments

Declare a project's skills in `agt.toml`, pin them in `agt.lock`, and let
`agt sync` make every checkout match. GitHub content is fetched through
`gh skill install`, so the GitHub CLI must be installed and authenticated.

```bash
agt init --agents claude,codex
agt add skill anthropics/skills pdf
agt add skill jiunbae/agent-skills development/git-commit-pr
git add agt.toml agt.lock

agt sync             # teammates: install exactly what agt.lock pins
agt sync --frozen    # CI: fail if agt.lock is stale
agt sync --check     # report drift (including local edits) without changing anything
agt lock --update    # move pins to the latest commit of each declared rev
```

Use `-g` with any of these commands to manage the user environment in
`~/.config/agt/agt.toml`. agt only modifies skill directories it installed
(marked with `.agt-managed`), and restores executable bits that
`gh skill install` drops. Design notes:
[`docs/design/0001-agent-env-manifest-and-doctor.md`](docs/design/0001-agent-env-manifest-and-doctor.md).

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

---
<p align="center"><sub>Part of <a href="https://github.com/Open330">Open330</a> · open source tools for AI-agent workflows · <a href="https://open330.github.io">open330.github.io</a></sub></p>
