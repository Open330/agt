# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
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
- `agt skill` 명령의 `--agent codex` 설치·조회·제거·업데이트 지원
- 원격 저장소의 프로필을 바로 설치하는 `--from <repo> --profile <name>` 조합

### Fixed
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
