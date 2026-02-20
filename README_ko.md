<p align="center">
  <br>
  <img src="assets/banner.png" alt="agt — AI 코딩 에이전트를 확장하는 모듈형 툴킷" width="720">
  <br><br>
  <a href="https://github.com/open330/agt/stargazers"><img src="https://img.shields.io/github/stars/open330/agt?style=for-the-badge&color=ff6b6b&labelColor=1a1a2e" alt="Stars"></a>
  <a href="https://github.com/open330/agt/releases"><img src="https://img.shields.io/github/v/release/open330/agt?style=for-the-badge&color=feca57&labelColor=1a1a2e" alt="Release"></a>
  <a href="https://www.npmjs.com/package/@open330/agt"><img src="https://img.shields.io/npm/v/@open330/agt?style=for-the-badge&color=c0392b&labelColor=1a1a2e&logo=npm&logoColor=white" alt="npm"></a>
  <a href="#라이선스"><img src="https://img.shields.io/badge/license-MIT-54a0ff?style=for-the-badge&labelColor=1a1a2e" alt="License"></a>
  <img src="https://img.shields.io/badge/skills-33-ee5a24?style=for-the-badge&labelColor=1a1a2e" alt="Skills">
  <img src="https://img.shields.io/badge/personas-8-78e08f?style=for-the-badge&labelColor=1a1a2e" alt="Personas">
  <br><br>
  <a href="#빠른-시작">빠른 시작</a> •
  <a href="#기능">기능</a> •
  <a href="#설치">설치</a> •
  <a href="#스킬-카탈로그">스킬</a> •
  <a href="#페르소나">페르소나</a> •
  <a href="#기여하기">기여하기</a>
  <br>
  <b><a href="README.md">English</a></b>
</p>

---

## 빠른 시작

```bash
# npm으로 설치
npm install -g @open330/agt

# 또는 원라인 설치
curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash -s -- --core --cli

# 스킬 설치
agt skill install kubernetes-skill

# 페르소나 코드 리뷰
agt persona review security-reviewer

# 스킬 자동 매칭 실행
agt run "보안 검사해줘"
```

---

## agt란?

**agt**는 **Claude Code**, **Codex CLI**, **Gemini CLI** 등 AI 코딩 에이전트에 도메인별 스킬, 전문가 페르소나, 자동화 훅을 추가하는 모듈형 툴킷입니다.

```
┌──────────────────────────────────────────────┐
│                    agt                        │
├──────────┬──────────┬──────────┬─────────────┤
│ 🛠 스킬   │ 🎭 페르소나 │ 🪝 훅    │ 📁 컨텍스트 │
│  33개     │  8명       │  2개    │  9개 설정   │
└──────────┴──────────┴──────────┴─────────────┘
       ↕            ↕           ↕
  Claude Code   Codex CLI   Gemini CLI
```

---

## 기능

| | 기능 | 설명 |
|---|---|---|
| 🛠 | **스킬** | 8개 카테고리, 33개 드롭인 스킬 — 보안, 개발, ML, 연동 등 |
| 🎭 | **페르소나** | 코드 리뷰를 위한 8명의 전문가 — 보안, 아키텍처, 성능, DBA, 프론트엔드, DevOps |
| 🪝 | **훅** | 이벤트 기반 자동화 — 영어 코칭, 프롬프트 로깅 |
| 📁 | **정적 컨텍스트** | 글로벌 설정 파일 — 사용자 프로필, 보안 규칙, 서비스 레지스트리 |
| 🤖 | **멀티 에이전트** | Claude, Codex, Gemini, Ollama 병렬 실행 |
| ⚡ | **통합 CLI** | 하나의 명령어: `agt skill`, `agt persona`, `agt run` |
| 🪟 | **크로스 플랫폼** | macOS, Linux, Windows (PowerShell) |
| 🔌 | **Codex 지원** | AGENTS.md + 스킬 심링크로 Codex CLI 연동 |

---

## 설치

### npm (권장)

```bash
npm install -g @open330/agt
agt version
```

**macOS** (arm64, x64) 및 **Linux** (x64, arm64) 사전 빌드 바이너리 포함. Rust 툴체인 불필요.

| 패키지 | 플랫폼 |
|---------|----------|
| `@open330/agt` | 메인 패키지 (플랫폼 자동 선택) |
| `@open330/agt-darwin-arm64` | macOS Apple Silicon |
| `@open330/agt-darwin-x64` | macOS Intel |
| `@open330/agt-linux-x64` | Linux x64 |
| `@open330/agt-linux-arm64` | Linux arm64 |

### 원격 설치

```bash
# 권장: Core 스킬 + CLI 도구
curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash -s -- --core --cli

# 전체 스킬
curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash -s -- --all --cli --static

# 특정 버전
curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash -s -- --version v2026.01.15

# 제거
curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash -s -- --uninstall
```

### 수동 설치

```bash
git clone https://github.com/open330/agt.git ~/.agt
cd ~/.agt

./install.sh --core --cli --link-static       # 권장
./install.sh all --link-static --codex --cli   # 전체 설치
./install.sh --list                            # 스킬 목록
```

### 워크스페이스별 설치

```bash
cd my-project
agt skill init                          # .claude/skills/ 생성
agt skill install kubernetes-skill      # 로컬 설치
agt skill install ml/                   # 그룹 전체 설치
```

### Windows

```powershell
./install.ps1
./install.ps1 --core --cli --link-static
```

```cmd
install.cmd --core --cli --link-static
```

> **참고:** Windows에서 심볼릭 링크는 관리자 권한 또는 Developer Mode가 필요합니다. 권한이 없으면 `--copy` 옵션을 사용하세요.

### 설치 옵션

| 옵션 | 설명 |
|------|------|
| `--core` | Core 스킬만 전역 설치 (권장) |
| `--link-static` | `~/.agents` → `static/` 심링크 (글로벌 컨텍스트) |
| `--codex` | Codex CLI 지원 (AGENTS.md + 스킬 심링크) |
| `--cli` | `agt` CLI 도구 설치 |
| `--hooks` | Claude Code 훅 설치 (`~/.claude/hooks`) |
| `--personas` | 에이전트 페르소나 설치 (`~/.agents/personas`) |
| `--copy` | 심링크 대신 복사 |
| `--dry-run` | 미리보기만 |
| `--uninstall` | 설치된 스킬 제거 |

### Core 스킬

`--core` 옵션으로 기본 설치되는 스킬:

- `development/git-commit-pr` — Git 커밋 및 PR 가이드
- `context/context-manager` — 프로젝트 컨텍스트 자동 로드
- `context/static-index` — 글로벌 정적 컨텍스트 인덱스
- `security/security-auditor` — 레포지토리 보안 감사
- `agents/background-implementer` — 백그라운드 병렬 구현
- `agents/background-planner` — 백그라운드 병렬 기획
- `agents/background-reviewer` — 다중 LLM 병렬 코드 리뷰

---

## CLI 사용법

### `agt skill` — 스킬 관리

```bash
agt skill install kubernetes-skill      # 로컬 설치
agt skill install -g git-commit-pr      # 전역 설치
agt skill install ml/                   # 그룹 전체 설치
agt skill list                          # 스킬 목록
agt skill list --installed --local      # 로컬 설치 확인
agt skill uninstall kubernetes-skill    # 제거
agt skill init                          # 워크스페이스 초기화
agt skill which kubernetes-skill        # 소스 경로 확인
```

**스킬 로드 우선순위:**
1. `.claude/skills/` (현재 워크스페이스)
2. `~/.claude/skills/` (전역)

### `agt persona` — 페르소나 관리

```bash
agt persona list                                    # 페르소나 목록
agt persona install security-reviewer                # 로컬 설치
agt persona install -g architecture-reviewer         # 전역 설치
agt persona create my-reviewer                       # 빈 템플릿
agt persona create rust-expert --ai "Rust unsafe specialist"  # LLM 생성
agt persona show security-reviewer                   # 상세 보기
agt persona review security-reviewer                 # 코드 리뷰 (git diff)
agt persona review security-reviewer --codex         # Codex로 리뷰
agt persona review security-reviewer --codex "이 스택 어떻게 생각해?"  # 커스텀 프롬프트
agt persona review security-reviewer -o review.md    # 파일 저장
```

**LLM 우선순위:** `codex` > `claude` > `gemini` > `ollama`

### `agt run` — 스킬 실행

```bash
agt run "보안 검사해줘"                  # 스킬 자동 선택
agt run --skill security-auditor "검사"  # 스킬 직접 지정
agt skill list                           # 사용 가능한 스킬
```

---

## 스킬 카탈로그

### 🤖 agents/ — AI 에이전트

| 스킬 | 설명 |
|------|------|
| `background-implementer` | 백그라운드 병렬 구현 (멀티 LLM, 컨텍스트 안전) |
| `background-planner` | 백그라운드 병렬 기획 (멀티 LLM, 자동 저장) |
| `background-reviewer` | 다중 LLM 병렬 코드 리뷰 (보안/아키텍처/코드 품질) |

### 🛠 development/ — 개발 도구

| 스킬 | 설명 |
|------|------|
| `context-worktree` | 작업별 git worktree 자동 생성 |
| `git-commit-pr` | Git 커밋 및 PR 생성 가이드 |
| `iac-deploy-prep` | IaC 배포 준비 (K8s, Dockerfile, CI/CD) |
| `multi-ai-code-review` | 멀티 AI 코드 리뷰 오케스트레이터 |
| `playwright` | Playwright 브라우저 자동화 |
| `pr-review-loop` | PR 리뷰 대기 및 자동 수정 |
| `task-master` | Task Master CLI 작업 관리 |

### 📊 business/ — 비즈니스

| 스킬 | 설명 |
|------|------|
| `bm-analyzer` | 비즈니스 모델 분석 및 수익화 전략 |
| `document-processor` | PDF, DOCX, XLSX, PPTX 문서 처리 |
| `proposal-analyzer` | 사업 제안서/RFP 분석 |

### 🔗 integrations/ — 외부 연동

| 스킬 | 설명 |
|------|------|
| `appstore-connect` | App Store Connect 자동화 |
| `discord-skill` | Discord REST API |
| `google-search-console` | Google Search Console API |
| `kubernetes-skill` | Kubernetes 클러스터 관리 |
| `notion-summary` | Notion 페이지 업로드 |
| `obsidian-tasks` | Obsidian TaskManager (Kanban, Dataview) |
| `obsidian-writer` | Obsidian Vault 문서 업로드 |
| `service-manager` | Docker 컨테이너 및 서비스 중앙 관리 |
| `slack-skill` | Slack 앱 개발 및 API |
| `vault-secrets` | Vaultwarden 자격증명 및 API 키 관리 |

### 🧠 ml/ — ML/AI

| 스킬 | 설명 |
|------|------|
| `audio-processor` | ffmpeg 기반 오디오 처리 |
| `ml-benchmark` | ML 모델 벤치마크 |
| `model-sync` | 모델 파일 서버 동기화 |
| `triton-deploy` | Triton Inference Server 배포 |

### 🔐 security/ — 보안

| 스킬 | 설명 |
|------|------|
| `security-auditor` | 레포지토리 보안 감사 |

### 📁 context/ — 컨텍스트 관리

| 스킬 | 설명 |
|------|------|
| `context-manager` | 프로젝트 컨텍스트 자동 로드 |
| `static-index` | 글로벌 정적 컨텍스트 인덱스 (사용자 프로필 포함) |

### 🔧 meta/ — 메타 스킬

| 스킬 | 설명 |
|------|------|
| `karpathy-guide` | LLM 코딩 오류 감소 가이드라인 |
| `skill-manager` | 스킬 생태계 관리 |
| `skill-recommender` | 스킬 자동 추천 |

---

## 페르소나

전문가 아이덴티티를 정의한 마크다운 파일입니다. 리뷰, 기획, 구현 등 **어떤 작업에든** 어떤 AI 에이전트에서든 사용할 수 있습니다.

### 작동 방식

페르소나는 YAML frontmatter (name, role, domain, tags)와 마크다운 본문 (아이덴티티, 전문 분야, 평가 기준, 출력 포맷)으로 구성된 `.md` 파일입니다. 파일을 읽을 수 있는 에이전트라면 누구든 페르소나를 채택할 수 있습니다.

```
.agents/personas/security-reviewer.md    ← 프로젝트 로컬 (최우선)
~/.agents/personas/security-reviewer.md  ← 사용자 전역
personas/security-reviewer.md            ← 라이브러리 (번들)
```

### 사용 가능한 페르소나

| 페르소나 | 역할 | 도메인 |
|----------|------|--------|
| `security-reviewer` | Senior AppSec Engineer | OWASP, 인증, 인젝션 |
| `architecture-reviewer` | Principal Architect | SOLID, API 설계, 결합도 |
| `code-quality-reviewer` | Staff Engineer | 가독성, 복잡도, DRY |
| `performance-reviewer` | Performance Engineer | 메모리, CPU, I/O, 확장성 |
| `database-reviewer` | Senior DBA | 쿼리 최적화, 스키마, 인덱싱 |
| `frontend-reviewer` | Senior Frontend Engineer | React, 접근성, 성능 |
| `devops-reviewer` | Senior DevOps/SRE | K8s, IaC, CI/CD |

### 에이전트별 사용법

페르소나는 알려진 경로의 마크다운 파일입니다. 에이전트가 파일을 읽고 아이덴티티를 채택합니다:

| 에이전트 | 사용 방법 |
|---------|-----------|
| **Claude Code** | 대화에서 파일 경로 언급: *"`.agents/personas/security-reviewer.md`를 읽고 그 페르소나로 동작해"* |
| **Codex** | `agt persona review security-reviewer --codex` 또는 프롬프트에 파일 내용 포함 |
| **Gemini** | `agt persona review security-reviewer --gemini` 또는 stdin으로 전달 |
| **Ollama** | `agt persona review security-reviewer` (자동 감지) |
| **OpenCode** | 에이전트 설정에 페르소나 파일 경로 참조 |
| **아무 에이전트** | `cat .agents/personas/<name>.md`로 읽어서 전달 |

```bash
# 코드 리뷰 — 자동 감지된 LLM으로 git diff 리뷰
agt persona review security-reviewer
agt persona review security-reviewer --codex
agt persona review security-reviewer --claude --staged
agt persona review security-reviewer --base main -o review.md

# 커스텀 프롬프트 — 페르소나에게 질문 (git diff 생략)
agt persona review security-reviewer --codex "이 아키텍처 확장 가능할까?"
agt persona review senior-reviewer --codex "Rust vs Go CLI 도구에 뭐가 나을까?"

# 페르소나 내용 확인
agt persona show security-reviewer

# 페르소나 파일 경로 확인 (다른 에이전트에 전달할 때)
agt persona which security-reviewer
```

### 페르소나 탐색

`static-index` 스킬이 페르소나 위치를 등록하므로 에이전트가 자동으로 발견할 수 있습니다:

```bash
agt persona install security-reviewer          # 로컬 → .agents/personas/
agt persona install -g architecture-reviewer   # 전역 → ~/.agents/personas/
agt persona install --from owner/repo/path     # GitHub에서 설치
```

---

## 훅

Claude Code 이벤트 기반 자동화.

```bash
./install.sh --hooks            # 설치
./install.sh --uninstall-hooks  # 제거
```

| 훅 | 이벤트 | 설명 |
|----|--------|------|
| `english-coach` | `UserPromptSubmit` | 프롬프트를 자연스러운 영어로 재작성 + 어휘 학습 |
| `prompt-logger` | `UserPromptSubmit` | MinIO로 프롬프트 로깅 (분석용) |

---

## 아키텍처

```
agt/
├── setup.sh                # 원격 설치 (curl)
├── install.sh              # 로컬 설치 (macOS/Linux)
├── install.ps1             # 로컬 설치 (Windows)
├── install.cmd             # Windows CMD 래퍼
│
├── agt/                    # 🦀 Rust CLI 바이너리
│   ├── Cargo.toml
│   └── src/
│
├── agents/                 # 🤖 AI 에이전트 스킬
├── development/            # 🛠 개발 도구 스킬
├── business/               # 📊 비즈니스 스킬
├── integrations/           # 🔗 외부 연동 스킬
├── ml/                     # 🧠 ML/AI 스킬
├── security/               # 🔐 보안 스킬
├── context/                # 📁 컨텍스트 관리
├── meta/                   # 🔧 메타 스킬
│
├── personas/               # 🎭 에이전트 페르소나 라이브러리
├── static/                 # 📁 글로벌 정적 컨텍스트 (.sample.md)
├── hooks/                  # 🪝 Claude Code 훅
├── codex-support/          # Codex CLI 지원
│
└── cli/                    # 레거시 CLI (deprecated)
    ├── agent-skill         # → `agt skill` 사용
    ├── agent-persona       # → `agt persona` 사용
    └── claude-skill        # → `agt run` 사용
```

---

## 스킬 만들기

### 스킬 구조

```
group/my-skill/
├── SKILL.md           # 필수: 스킬 정의
├── scripts/           # 선택: 실행 스크립트
├── references/        # 선택: 참고 문서
└── templates/         # 선택: 템플릿 파일
```

### SKILL.md 형식

```markdown
---
name: my-skill
description: 스킬 설명. 키워드로 활성화.
---

# My Skill

## Overview
스킬 개요

## When to Use
활성화 조건

## Workflow
사용 방법

## Examples
사용 예시
```

### 새 스킬 추가

```bash
mkdir -p development/my-skill
vim development/my-skill/SKILL.md
agt skill install my-skill          # 테스트 설치
agt skill list | grep my-skill      # 확인
```

---

## 페르소나 만들기

```bash
agt persona create my-reviewer                       # 빈 템플릿
agt persona create rust-expert --ai "Rust unsafe specialist"  # LLM으로 자동 생성
agt persona create rust-expert --codex "Rust unsafe specialist"  # 특정 LLM으로 생성
```

### 페르소나 형식

```markdown
---
name: my-reviewer
role: "역할 제목"
domain: security | architecture | quality | performance
type: review | planning | implementation
tags: [tag1, tag2]
---

## Identity
배경 및 전문 분야

## Review Lens
리뷰 관점

## Evaluation Framework
평가 기준

## Output Format
결과 형식
```

---

## Codex CLI 지원

```bash
./install.sh --codex
```

`~/.codex/AGENTS.md`에 스킬 가이드를 추가하고, `~/.codex/skills` → `~/.claude/skills` 심링크를 생성합니다.

---

## 문제 해결

### 스킬이 인식되지 않음

```bash
head -n 5 ~/.claude/skills/my-skill/SKILL.md    # frontmatter 확인
agt skill list                                    # 설치 확인
```

### 심볼릭 링크 깨짐

```bash
agt skill uninstall my-skill
agt skill install my-skill
```

### Codex에서 스킬 인식 안됨

```bash
ls -la ~/.codex/skills          # 심링크 확인
./install.sh --codex            # 재설치
```

---

## agent-skills에서 마이그레이션

이전 `agent-skills` 레포를 사용하셨다면 [MIGRATION.md](MIGRATION.md)를 참고하세요.

**요약:**
- 이전 CLI 이름 (`agent-skill`, `agent-persona`, `claude-skill`)은 계속 동작하지만 deprecated
- `~/.agents/` 경로는 변경 없음
- 설치 URL을 `open330/agt`로 업데이트

---

## 기여하기

기여를 환영합니다! 참여 방법:

1. **스킬 추가** — 적절한 카테고리에 새 스킬 생성
2. **페르소나 추가** — 도메인 전문가 페르소나 생성
3. **문서 개선** — 오타 수정, 예제 추가, 번역
4. **이슈 제보** — 버그 리포트 및 기능 요청 환영

```bash
git clone https://github.com/open330/agt.git
cd agt
./install.sh --core --cli --link-static    # 개발 환경 설정
```

---

## 라이선스

MIT License. 자세한 내용은 [LICENSE](LICENSE)를 참고하세요.

---

<p align="center">
  <sub>AI 에이전트 커뮤니티를 위해 ❤️ 로 만들었습니다</sub><br>
  <sub><strong>33</strong> 스킬 • <strong>8</strong> 페르소나 • <strong>2</strong> 훅 • <strong>∞</strong> 가능성</sub>
</p>
