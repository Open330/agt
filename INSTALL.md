# Installation Guide

스킬을 설치하고 관리하는 가이드입니다.

## Prerequisites

- Claude Code 설치됨
- Git (클론 및 버전 관리용)
- Bash 4.0+ (대부분의 macOS/Linux에 기본 포함)

## Quick Start

```bash
# 저장소 클론
git clone <repository-url> ~/workspace/agent-skills
cd ~/workspace/agent-skills

# 전체 설치
./install.sh

# 특정 그룹만 설치
./install.sh agents

# 특정 스킬만 설치
./install.sh agents/planning-agents
```

## Directory Structure

스킬은 주제별로 그룹화되어 있습니다:

```
~/workspace/agent-skills/
├── install.sh                    # 설치 스크립트
├── agents/                       # AI 에이전트 관련
│   ├── multi-llm-agent/
│   └── planning-agents/
├── development/                  # 개발 도구
│   ├── context-manager/
│   └── git-commit-pr/
└── business/                     # 비즈니스
    └── proposal-analyzer/
```

## Using the Install Script

### 스킬 목록 보기

```bash
./install.sh --list
```

출력 예시:
```
agents/
  ├── multi-llm-agent
  └── planning-agents

development/
  ├── context-manager
  └── git-commit-pr

business/
  └── proposal-analyzer
```

### 설치 옵션

```bash
# 전체 설치
./install.sh
./install.sh all

# 그룹 단위 설치
./install.sh agents                    # agents 그룹만
./install.sh agents development        # 여러 그룹

# 개별 스킬 설치
./install.sh agents/planning-agents
./install.sh development/git-commit-pr business/proposal-analyzer
```

### 네이밍 옵션 (Prefix/Postfix)

스킬 이름에 접두사/접미사를 추가할 수 있습니다:

```bash
# 접두사 추가 (예: my-planning-agents)
./install.sh --prefix "my-" agents

# 접미사 추가 (예: planning-agents-dev)
./install.sh --postfix "-dev" agents

# 둘 다 (예: team-planning-agents-v2)
./install.sh --prefix "team-" --postfix "-v2"
```

활용 예:
- 여러 버전 동시 운용
- 개인/팀 스킬 구분
- 테스트 버전과 안정 버전 분리

### 설치 방법

```bash
# 심볼릭 링크 (기본) - 변경사항 자동 동기화
./install.sh

# 복사 - 독립적인 설치본
./install.sh --copy
```

### 대상 디렉토리 변경

```bash
# 커스텀 위치에 설치
./install.sh --target ~/.claude/skills-dev

# 다른 Claude Code 인스턴스에 설치
./install.sh -t /path/to/other/.claude/skills
```

### 미리보기 (Dry Run)

```bash
# 실제 변경 없이 미리보기
./install.sh --dry-run
./install.sh --dry-run agents
./install.sh --prefix "test-" --dry-run
```

### 삭제

```bash
# 전체 삭제
./install.sh --uninstall

# 특정 그룹 삭제
./install.sh --uninstall agents

# 특정 스킬 삭제
./install.sh --uninstall agents/planning-agents

# 접두사 붙여서 설치한 경우
./install.sh --prefix "my-" --uninstall
```

## Command Reference

```
사용법: install.sh [옵션] [그룹/스킬...]

옵션:
  -h, --help       도움말 표시
  -l, --list       사용 가능한 스킬 목록 표시
  -u, --uninstall  스킬 삭제
  -c, --copy       심볼릭 링크 대신 복사
  -n, --dry-run    실제 변경 없이 미리보기
  -q, --quiet      최소 출력
  --prefix PREFIX  스킬 이름 앞에 접두사 추가
  --postfix POSTFIX 스킬 이름 뒤에 접미사 추가
  -t, --target DIR 대상 디렉토리 지정 (기본: ~/.claude/skills)

그룹:
  agents       AI 에이전트 스킬 (multi-llm-agent, planning-agents)
  development  개발 도구 스킬 (git-commit-pr, context-manager)
  business     비즈니스 스킬 (proposal-analyzer)
```

## Examples

### 시나리오 1: 개발 환경 설정

저장소와 동기화된 상태로 스킬 사용:

```bash
# 심볼릭 링크로 설치 (기본)
./install.sh

# 저장소에서 스킬 수정
vim agents/planning-agents/SKILL.md

# 변경사항 즉시 반영됨
```

### 시나리오 2: 개발/운영 분리

```bash
# 운영용 (안정 버전)
./install.sh --postfix "-stable" --copy

# 개발용 (심볼릭 링크로 편집 가능)
./install.sh --postfix "-dev"
```

### 시나리오 3: 팀 공유 스킬

```bash
# 개인용
./install.sh --prefix "june-"

# 팀 공유용
./install.sh --prefix "team-" --copy
```

### 시나리오 4: 특정 그룹만 설치

```bash
# AI 에이전트 관련만 설치
./install.sh agents

# 개발 도구만 설치
./install.sh development
```

## Manual Installation

스크립트 없이 수동 설치:

### 심볼릭 링크 (개발용 권장)

```bash
cd ~/.claude/skills
ln -s ~/workspace/agent-skills/agents/planning-agents planning-agents
```

### 복사

```bash
cp -r ~/workspace/agent-skills/agents/planning-agents ~/.claude/skills/
```

## Verifying Installation

```bash
# 설치된 스킬 목록
ls -la ~/.claude/skills/

# 스킬 메타데이터 확인
head -n 5 ~/.claude/skills/planning-agents/SKILL.md

# 스크립트 테스트 (해당되는 경우)
python ~/.claude/skills/planning-agents/scripts/planner.py --help
```

## Troubleshooting

### 스킬이 인식되지 않음

1. SKILL.md frontmatter 확인:
   ```bash
   head -n 10 ~/.claude/skills/planning-agents/SKILL.md
   ```
   다음 형식이어야 함:
   ```yaml
   ---
   name: planning-agents
   description: ...
   ---
   ```

2. 권한 확인:
   ```bash
   ls -la ~/.claude/skills/planning-agents/
   ```

### 심볼릭 링크 깨짐

```bash
# 삭제 후 재설치
./install.sh --uninstall agents/planning-agents
./install.sh agents/planning-agents
```

### 스크립트 권한 오류

```bash
chmod +x ~/.claude/skills/*/scripts/*.py
chmod +x ~/.claude/skills/*/scripts/*.sh
```

## Syncing Across Machines

```bash
# Machine A: 변경사항 푸시
cd ~/workspace/agent-skills
git add . && git commit -m "Update skills" && git push

# Machine B: 풀 및 재설치
cd ~/workspace/agent-skills
git pull
./install.sh
```

---

**추가 정보**: [README.md](README.md) 참고
