# Agent Skills Repository

Claude Code 기능을 확장하는 커스텀 스킬 모음입니다.

## Quick Start

```bash
# 레포지토리 클론
git clone <repository-url> ~/workspace/agent-skills
cd ~/workspace/agent-skills

# 모든 스킬 설치
python3 install.py

# 설치 확인
python3 install.py --list
```

## Available Skills

### 🗂️ context-manager

프로젝트 컨텍스트 문서를 자동으로 탐색하고 로드합니다.

- `context/` 디렉토리에서 관련 문서 자동 탐색
- 키워드, 파일 경로, 작업 유형 기반 매칭
- 작업 완료 후 문서 업데이트

### 🔀 git-commit-pr

Git 커밋 및 Pull Request 생성을 가이드합니다.

- 커밋 메시지 작성 가이드
- PR 생성 워크플로우
- 컨벤션 준수 지원

### 🤖 multi-llm-agent

여러 LLM을 통합하여 멀티 에이전트 협업을 수행합니다.

- **지원 LLM**: OpenAI, Gemini, Anthropic, Ollama
- **협업 패턴**: 역할 분담, 토론/합의, 체인 파이프라인, 병렬 처리
- 동적 시나리오 구성

### 📋 proposal-analyzer

사업 제안서/RFP 문서를 분석합니다.

- 가격, 기한, 기술 스펙 적정성 평가
- 사업 진행 여부 판단 보고서 생성

## Installation

### 설치 스크립트 사용 (권장)

```bash
# 모든 스킬 설치
python3 install.py

# 특정 스킬만 설치
python3 install.py context-manager multi-llm-agent

# 스킬 목록 확인
python3 install.py --list
```

### Prefix/Postfix로 스킬 구분

여러 버전이나 환경을 구분할 때 사용합니다:

```bash
# prefix 추가 (예: my-context-manager)
python3 install.py --prefix "my-"

# postfix 추가 (예: context-manager-dev)
python3 install.py --postfix "-dev"

# 조합 (예: team-context-manager-v2)
python3 install.py --prefix "team-" --postfix "-v2"
```

### 설치 옵션

```bash
# 심볼릭 링크 (기본값) - 변경사항 자동 반영
python3 install.py

# 복사 모드 - 독립적인 설치
python3 install.py --copy

# 설치 미리보기
python3 install.py --dry-run

# 다른 경로에 설치
python3 install.py --target-dir ~/.claude/skills-dev
```

### 제거

```bash
# 모든 스킬 제거
python3 install.py --uninstall

# 특정 스킬만 제거
python3 install.py --uninstall context-manager

# prefix로 설치한 스킬 제거
python3 install.py --prefix "my-" --uninstall
```

## Repository Structure

```
agent-skills/
├── install.py                 # 설치 스크립트
├── README.md                  # 이 문서
├── INSTALL.md                 # 상세 설치 가이드
│
├── context-manager/           # 컨텍스트 관리 스킬
│   ├── SKILL.md
│   ├── scripts/
│   └── references/
│
├── git-commit-pr/             # Git 커밋/PR 스킬
│   └── SKILL.md
│
├── multi-llm-agent/           # 멀티 LLM 에이전트 스킬
│   ├── SKILL.md
│   ├── scripts/
│   │   ├── llm_client.py      # 통합 LLM 클라이언트
│   │   ├── orchestrator.py    # 오케스트레이터
│   │   └── patterns/          # 협업 패턴
│   ├── config/
│   └── references/
│
└── proposal-analyzer/         # 제안서 분석 스킬
    └── SKILL.md
```

## Usage Examples

### 예시 1: 개발 환경 설정

```bash
# 개발용 스킬 (심볼릭 링크로 변경사항 즉시 반영)
python3 install.py --postfix "-dev"

# 스킬 수정
vim multi-llm-agent/SKILL.md

# 변경사항이 Claude Code에 즉시 반영됨
```

### 예시 2: 개인/팀 스킬 분리

```bash
# 개인 스킬
python3 install.py --prefix "personal-"

# 팀 공유 스킬
python3 install.py --prefix "team-" --copy
```

### 예시 3: 버전 관리

```bash
# 안정 버전
python3 install.py --postfix "-stable"

# 테스트 버전
python3 install.py --postfix "-beta" context-manager
```

## Install Script Reference

```
usage: install.py [-h] [--prefix PREFIX] [--postfix POSTFIX]
                  [--target-dir DIR] [--copy] [--dry-run]
                  [--uninstall] [--list] [--quiet]
                  [skills ...]

옵션:
  skills                설치/제거할 스킬 (미지정시 전체)
  --prefix PREFIX       스킬 이름 접두사
  --postfix POSTFIX     스킬 이름 접미사
  --target-dir, -t      설치 경로 (기본: ~/.claude/skills)
  --copy, -c            복사 모드 (기본: 심볼릭 링크)
  --dry-run, -n         미리보기만
  --uninstall, -u       제거 모드
  --list, -l            스킬 목록 출력
  --quiet, -q           최소 출력
```

## Creating New Skills

### 스킬 구조

```
my-skill/
├── SKILL.md           # 필수: 스킬 설명 및 사용법
├── scripts/           # 선택: 실행 스크립트
├── references/        # 선택: 참고 문서
└── config/            # 선택: 설정 파일
```

### SKILL.md 형식

```markdown
---
name: my-skill
description: 스킬에 대한 간단한 설명. 이 설명이 스킬 활성화 조건이 됩니다.
---

# My Skill

## Overview
스킬 개요

## When to Use
활성화 조건

## Workflow
사용 방법
```

### 새 스킬 추가

1. 디렉토리 생성: `mkdir my-skill`
2. SKILL.md 작성
3. 필요시 scripts/, references/ 추가
4. 테스트: `python3 install.py my-skill`
5. 커밋: `git add my-skill && git commit -m "Add my-skill"`

## Syncing Across Machines

```bash
# Machine A
cd ~/workspace/agent-skills
git add . && git commit -m "Update skills" && git push

# Machine B
cd ~/workspace/agent-skills
git pull
python3 install.py
```

## Troubleshooting

### 스킬이 인식되지 않음

1. SKILL.md frontmatter 확인:
   ```bash
   head -n 5 ~/.claude/skills/my-skill/SKILL.md
   ```

2. 설치 상태 확인:
   ```bash
   python3 install.py --list
   ```

### 심볼릭 링크 깨짐

```bash
python3 install.py --uninstall my-skill
python3 install.py my-skill
```

### 스크립트 권한 오류

```bash
chmod +x ~/.claude/skills/*/scripts/*.py
```

## License

Personal use. Individual skills may have their own licenses.

---

**Last Updated**: 2025-12-09
**Skills Count**: 4
