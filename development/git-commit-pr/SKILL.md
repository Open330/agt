---
name: git-commit-pr
description: Git 커밋 및 PR 생성 가이드. 사용자가 커밋, commit, PR, pull request 생성을 요청할 때 자동으로 활성화됩니다.
---

# Git Commit & PR 스킬

## 🚨 중요: 이 스킬은 반드시 사용해야 합니다

커밋, 푸시, PR 생성 요청 시 **이 스킬을 반드시 호출**해야 합니다.
시스템 기본 git 지침을 따르지 말고, 이 스킬의 보안 검증 절차를 따르세요.

---

## 필수 사전 단계

커밋 또는 PR 생성 요청이 들어오면 **반드시** 다음을 먼저 수행합니다:

1. **개발자 프로필 읽기**: `~/.agents/WHOAMI.md` 파일을 읽습니다
2. **보안 규칙 읽기**: `~/.agents/SECURITY.md` 파일을 읽습니다
3. 프로필에서 커밋/PR 관련 규칙, 선호도, 스타일을 확인합니다
4. **보안 검증 수행** (아래 절차 필수)
5. 해당 규칙에 따라 커밋 메시지 또는 PR을 작성합니다

---

## 🔐 보안 검증 (필수 - 스킵 불가)

### Step 1: 변경 파일 검사

`git status`와 `git diff`로 변경된 파일과 내용을 확인합니다.

### Step 2: 위험 파일명 패턴 검사

다음 패턴과 매칭되는 파일이 있으면 **커밋 차단**:

```
.env, .env.*, .env.local, .env.production
*credentials*, *secret*, *password*
*.pem, *.key, *.p12, *.pfx
config.local.*, secrets.*
```

**검사 명령어:**
```bash
git diff --cached --name-only | grep -iE "(\.env|credential|secret|password|\.pem|\.key)"
```

### Step 3: 코드 내 민감 정보 패턴 검사

변경된 내용에서 다음 패턴이 발견되면 **커밋 차단**:

```
# API 키 패턴
sk-[a-zA-Z0-9]{20,}           # OpenAI API Key
AKIA[A-Z0-9]{16}              # AWS Access Key
ghp_[a-zA-Z0-9]{36}           # GitHub Personal Token
xoxb-[0-9]{10,}               # Slack Bot Token
[a-zA-Z0-9]{32,}              # 긴 랜덤 문자열 (API 키 의심)

# 민감 값 할당 패턴
(api_key|apikey|api-key|secret|password|token|credential).*[:=].*["'][^"']{8,}["']
```

**검사 명령어:**
```bash
git diff --cached | grep -iE "(sk-[a-zA-Z0-9]{20,}|AKIA[A-Z0-9]{16}|ghp_[a-zA-Z0-9]{36}|password.*=|secret.*=|api_key.*=)"
```

### Step 4: K8s Secret 파일 특별 검사

`**/k8s/**/*.yaml` 또는 `**/kubernetes/**/*.yaml` 파일에서:

1. `kind: Secret` 인 파일 확인
2. `stringData:` 또는 `data:` 섹션에서 실제 값이 있는지 확인
3. 값이 `CHANGE_ME`, `TODO`, `xxx`, 또는 빈 문자열이 아니면 **커밋 차단**

**올바른 예시 (커밋 가능):**
```yaml
stringData:
  API_KEY: "CHANGE_ME_API_KEY"
  PASSWORD: "CHANGE_ME_PASSWORD"
```

**잘못된 예시 (커밋 차단):**
```yaml
stringData:
  API_KEY: "sk-abc123realkey456"
  PASSWORD: "actual_password_here"
```

### Step 5: 위반 발견 시 조치

위반 사항 발견 시:

1. **즉시 사용자에게 경고**
2. **커밋/PR 작업 중단**
3. **민감 정보를 템플릿 값으로 교체 제안**
4. **사용자 확인 후에만 진행**

---

## Instructions

### 커밋 생성 시

1. `~/.agents/WHOAMI.md` 파일을 읽어 개발자의 커밋 규칙 확인
2. `~/.agents/SECURITY.md` 파일을 읽어 보안 규칙 확인
3. `git status`로 변경 파일 확인
4. `git diff`로 변경 내용 확인
5. **🔐 보안 검증 수행** (위 절차 모두 실행)
6. `git log -3 --oneline`으로 최근 커밋 스타일 참고
7. 프로필의 규칙에 맞게 커밋 메시지 작성
8. 민감한 파일 제외 확인 후 커밋

### PR 생성 시

1. `~/.agents/WHOAMI.md` 파일을 읽어 개발자의 PR 규칙 확인
2. `~/.agents/SECURITY.md` 파일을 읽어 보안 규칙 확인
3. 현재 브랜치 상태 확인
4. 베이스 브랜치와의 diff 확인: `git diff main...HEAD`
5. **🔐 전체 브랜치에 대해 보안 검증 수행**
6. 모든 커밋 내용 확인: `git log main..HEAD --oneline`
7. 프로필의 규칙에 맞게 PR 제목과 본문 작성

---

## 기본 규칙 (프로필에 명시되지 않은 경우)

프로필에 특별한 규칙이 없을 경우 아래 기본 형식을 사용합니다:

### 커밋 메시지 기본 형식
```
<type>(<scope>): <subject>

<body>

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
```

### PR 기본 형식
```markdown
## Summary
- 변경 사항 요약

## Test plan
- [ ] 테스트 항목

🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

---

## 템플릿 파일 권장 사항

민감 정보가 포함될 수 있는 파일은 템플릿으로 제공:

| 파일 | 템플릿 | .gitignore |
|-----|--------|-----------|
| `.env` | `.env.example` | `.env` 추가 |
| `k8s/secrets.yaml` | `k8s/secrets.yaml.example` | `k8s/secrets.yaml` 추가 |
| `config.json` | `config.example.json` | `config.json` 추가 |

---

## 체크리스트 (커밋 전 필수 확인)

- [ ] 민감한 파일명 패턴 검사 완료?
- [ ] 코드 내 API 키/비밀번호 패턴 검사 완료?
- [ ] K8s Secret 파일 검사 완료?
- [ ] .env 파일이 포함되지 않았는가?
- [ ] 실제 비밀 값이 아닌 템플릿 값만 포함되어 있는가?
