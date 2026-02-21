<p align="center">
  <br>
  <img src="assets/banner.png" alt="agt — A modular toolkit for extending AI coding agents" width="720">
  <br><br>
  <a href="https://github.com/open330/agt/stargazers"><img src="https://img.shields.io/github/stars/open330/agt?style=for-the-badge&color=ff6b6b&labelColor=1a1a2e" alt="Stars"></a>
  <a href="https://github.com/open330/agt/releases"><img src="https://img.shields.io/github/v/release/open330/agt?style=for-the-badge&color=feca57&labelColor=1a1a2e" alt="Release"></a>
  <a href="https://www.npmjs.com/package/@open330/agt"><img src="https://img.shields.io/npm/v/@open330/agt?style=for-the-badge&color=c0392b&labelColor=1a1a2e&logo=npm&logoColor=white" alt="npm"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT-54a0ff?style=for-the-badge&labelColor=1a1a2e" alt="License"></a>
  <br><br>
  <a href="#quick-start">Quick Start</a> •
  <a href="#installation">Installation</a> •
  <a href="#cli-usage">CLI Usage</a> •
  <a href="#skills">Skills</a>
</p>

---

## Quick Start

```bash
# Install agt
npm install -g @open330/agt

# Get skills (optional — agt discovers skills automatically)
git clone https://github.com/jiunbae/agent-skills ~/.agent-skills

# Use it
agt skill list
agt skill install kubernetes-skill
agt persona review security-reviewer --codex
agt run "scan for security issues"
```

---

## What is agt?

**agt** is a CLI toolkit that extends AI coding agents (Claude Code, Codex, Gemini) with modular skills and expert personas.

- **Skills** — Drop-in markdown modules that give agents domain-specific capabilities
- **Personas** — Expert identities for code review, planning, and implementation
- **agt CLI** — Install, manage, and run skills/personas from any project

Skills and personas live in a separate content repository: **[jiunbae/agent-skills](https://github.com/jiunbae/agent-skills)**

---

## Installation

### npm (Recommended)

```bash
npm install -g @open330/agt
```

Pre-built binaries for **macOS** (arm64, x64) and **Linux** (x64, arm64). No Rust toolchain required.

| Package | Platform |
|---------|----------|
| `@open330/agt` | Main package (auto-selects platform) |
| `@open330/agt-darwin-arm64` | macOS Apple Silicon |
| `@open330/agt-darwin-x64` | macOS Intel |
| `@open330/agt-linux-x64` | Linux x64 |
| `@open330/agt-linux-arm64` | Linux arm64 |

### Setup Script

```bash
# Binary only
curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash

# Binary + skills + core install
curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash -s -- --skills --core

# Uninstall
curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash -s -- --uninstall
```

### Build from Source

```bash
git clone https://github.com/open330/agt.git
cd agt
make release    # builds agt/target/release/agt
```

### Get Skills

agt discovers skills from these locations (in order):

1. `$AGT_DIR` or `$AGENT_SKILLS_DIR` environment variable
2. Executable's parent directories (if installed inside a skills repo)
3. `~/.agent-skills`, `~/.agt`, `~/agt`

```bash
# Recommended: clone to ~/.agent-skills
git clone https://github.com/jiunbae/agent-skills ~/.agent-skills
```

---

## CLI Usage

### `agt skill` — Skill Management

```bash
agt skill list                          # List all skills
agt skill list --installed --local      # List local installs
agt skill install kubernetes-skill      # Install to .claude/skills/
agt skill install -g git-commit-pr      # Install globally
agt skill install ml/                   # Install entire group
agt skill uninstall kubernetes-skill    # Remove
agt skill which kubernetes-skill        # Show source path
agt skill init                          # Init workspace
```

### `agt persona` — Persona Management

```bash
agt persona list                                    # List personas
agt persona install security-reviewer               # Install locally
agt persona install -g --all                        # Install all globally
agt persona show security-reviewer                  # View content
agt persona which security-reviewer                 # Show file path

# Code review (uses git diff)
agt persona review security-reviewer                # Auto-detect LLM
agt persona review security-reviewer --codex        # Use Codex
agt persona review security-reviewer --staged       # Staged changes only
agt persona review security-reviewer -o review.md   # Save to file

# Custom prompt (skips git diff)
agt persona review security-reviewer --codex "is this architecture scalable?"

# Create a new persona
agt persona create my-reviewer                                  # Empty template
agt persona create rust-expert --ai "Rust unsafe specialist"    # AI-generated
```

### `agt run` — Skill Execution

```bash
agt run "scan for security issues"       # Auto skill matching
agt run --skill security-auditor "scan"  # Specify skill
```

### Shell Completions

```bash
# Zsh
agt completions zsh > ~/.cache/.agt-completion.zsh
source ~/.cache/.agt-completion.zsh

# Bash
agt completions bash >> ~/.bashrc

# Fish
agt completions fish > ~/.config/fish/completions/agt.fish
```

---

## Skills

Skills are maintained in **[jiunbae/agent-skills](https://github.com/jiunbae/agent-skills)** — 33+ skills across 8 categories:

| Category | Skills |
|----------|--------|
| agents/ | background-implementer, background-planner, background-reviewer |
| development/ | git-commit-pr, playwright, pr-review-loop, iac-deploy-prep, ... |
| business/ | bm-analyzer, document-processor, proposal-analyzer |
| integrations/ | kubernetes, slack, discord, notion, obsidian, vault, ... |
| ml/ | audio-processor, ml-benchmark, model-sync, triton-deploy |
| security/ | security-auditor |
| context/ | context-manager, static-index |
| meta/ | skill-manager, skill-recommender, karpathy-guide |

### Creating Skills

```
group/my-skill/
├── SKILL.md           # Required: skill definition
├── scripts/           # Optional: executable scripts
├── references/        # Optional: reference docs
└── templates/         # Optional: template files
```

See [jiunbae/agent-skills](https://github.com/jiunbae/agent-skills) for full documentation.

---

## Personas

7 expert personas for code review and beyond:

| Persona | Role |
|---------|------|
| `security-reviewer` | Senior AppSec Engineer |
| `architecture-reviewer` | Principal Architect |
| `code-quality-reviewer` | Staff Engineer |
| `performance-reviewer` | Performance Engineer |
| `database-reviewer` | Senior DBA |
| `frontend-reviewer` | Senior Frontend Engineer |
| `devops-reviewer` | Senior DevOps/SRE |

---

## Architecture

```
open330/agt (this repo)          jiunbae/agent-skills (content)
├── agt/        Rust CLI         ├── agents/       AI agent skills
├── npm/        npm packaging    ├── development/  Dev tool skills
├── setup.sh    Installer        ├── integrations/ Integration skills
├── Makefile                     ├── personas/     Expert personas
└── assets/                      ├── hooks/        Claude Code hooks
                                 └── static/       Global context
```

---

## Contributing

```bash
# Clone and build
git clone https://github.com/open330/agt.git
cd agt
make build      # Dev build
make test       # Run tests
make release    # Release build
```

For skills and personas, contribute to [jiunbae/agent-skills](https://github.com/jiunbae/agent-skills).

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

<p align="center">
  <sub>Built with care for the AI agent community</sub>
</p>
