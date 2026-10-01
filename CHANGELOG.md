# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Declarative environments: `agt init`, `agt add skill`, `agt remove`,
  `agt sync [--check|--frozen]`, and `agt lock [--update]` manage skills
  declared in `agt.toml` and pinned to commit and tree SHAs in `agt.lock`.
  `-g` manages the user environment in `~/.config/agt/agt.toml`. GitHub
  content is fetched with `gh skill install`; agt restores the executable
  bits it drops and only touches directories marked `.agt-managed`.

### Changed
- Claude skills now install flat (`<skills-dir>/<skill>`) instead of
  `<skills-dir>/<group>/<skill>`, which Claude Code never loaded. Installing a
  skill moves an existing grouped copy to the flat path.
- Skills install under their frontmatter `name` when it is a valid skill name,
  so same-named directories in different groups (`billing/notion`,
  `crm/notion`) no longer collide.
- The Claude config directory follows `--claude-dir`, then `CLAUDE_CONFIG_DIR`,
  then `~/.claude` for skills, hooks, teams and `settings.json`.
- Defined `Open330/agt` as the single source of truth for the Rust CLI, npm
  packages, platform binaries, and release automation.
- Replaced the legacy catalog installer with an npm CLI bootstrap that can
  optionally install skills from `jiunbae/agent-skills`.
- Limited supported npm platform manifests to Darwin ARM64, Linux x64, and
  Linux ARM64.

### Removed
- Removed the duplicated skill catalog, personas, hooks, static context,
  profiles, and legacy Bash/PowerShell skill installers.
- Removed the inactive Darwin x64 platform manifest.

### Added
- `agt skill migrate [--global] [--dry-run]` moves grouped Claude skills to the
  flat layout; conflicts and non-skill names (backups) are left in place.
- `--no-static` on `agt skill install` skips the source repo's `[[setup.copy]]`
  rules.
- Global `--claude-dir <dir>` option.
- `agt apply [--target <path>] [--dry-run|--check]` converges skill directories
  to the stacks in `~/.config/agt/layers.toml`: links, adopts and prunes only
  what it manages, installs directory targets into each git repo below them
  (excluded via `.git/info/exclude`), and runs static copy only for the global
  target.
- `agt gate <dir> -- <cmd>` runs a hook command only inside a directory tree.
- A layer can list `skills = ["group/skill", ...]` instead of, or after, a
  `profile`, for machine-local picks that do not belong in a shared profile.
- `agt skill status [--global] [--json]` lists what agt installed in a skills
  directory with its layer and source, plus unmanaged and missing entries and
  any skills still in the old grouped layout. Installs and uninstalls record
  this in `agt-state.json` beside the skills directory.
- Profiles can `extends: <name>` or `extends: [a, b]` (same source; cycles are
  an error), and `--profile` accepts a comma list such as `core,dev`.
- `agt skill` 명령의 `--agent codex` 설치·조회·제거·업데이트 지원
- 원격 저장소의 프로필을 바로 설치하는 `--from <repo> --profile <name>` 조합

### Fixed
- `agt skill uninstall <name>` removes the skill of that name instead of a
  same-named virtual group (e.g. a skill called `other`).
- Extra profile `*.yml` files merge in sorted order, so a profile defined twice
  resolves the same way everywhere.
- The built-in `core` profile no longer lists the retired `context-manager`.
- Linux ARM64 npm 선택 패키지가 설치되어도 wrapper가 바이너리를 찾지 못하던 문제

### Changed
- **BREAKING**: Repository rebranded from `jiunbae/agent-skills` to `open330/agt`
- **BREAKING**: CLI tools unified into single `agt` command
  - `agent-skill` → `agt skill`
  - `agent-persona` → `agt persona`
  - `claude-skill` → `agt run`
- Remote install URL: `open330/agt/main/setup.sh`
- Install directory: `~/.agt` (was `~/.agent-skills`)

### Deprecated
- `agent-skill`, `agent-persona`, `claude-skill` commands (still work, use `agt` instead)

### Added
- Unified `agt` Rust CLI binary
- `agt skill`: workspace skill management
  - `agt skill install <skill>`: local install
  - `agt skill install -g <skill>`: global install
  - `agt skill list`: list skills
  - `agt skill init`: workspace init
- `agt persona`: persona management and code review
- `agt run`: skill execution with auto-matching
- `setup.sh`: remote installer (curl one-liner)
- `install.sh --core`: core skills only option
- GitHub Actions release workflow

### Core Skills
- `development/git-commit-pr`
- `context/context-manager`
- `context/static-index`
- `security/security-auditor`
- `agents/background-implementer`
- `agents/background-planner`

## [0.1.0] - 2026-01-15

### Added
- 초기 스킬 셋 (33개)
- `install.sh` 설치 스크립트
- `claude-skill` CLI 도구
- Codex CLI 지원
- Static 디렉토리 (글로벌 컨텍스트)

### Skills by Category
- **agents**: background-implementer, background-planner
- **development**: context-worktree, git-commit-pr, multi-ai-code-review, playwright, pr-review-loop, task-master
- **business**: bm-analyzer, document-processor, proposal-analyzer
- **integrations**: appstore-connect, discord-skill, google-search-console, kubernetes-skill, notion-summary, obsidian-tasks, obsidian-writer, slack-skill
- **ml**: audio-processor, ml-benchmark, model-sync, triton-deploy
- **context**: context-manager, static-index, whoami
- **meta**: skill-manager, skill-recommender
- **security**: security-auditor
