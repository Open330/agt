# agt

`agt`는 Claude Code와 Codex에서 스킬, 페르소나, 훅, 다중 에이전트
워크플로를 설치하고 실행하는 Rust CLI입니다.

이 저장소는 CLI, npm 패키지, 플랫폼 바이너리와 릴리스 자동화만
관리합니다. 스킬 콘텐츠의 단일 원본은
[`jiunbae/agent-skills`](https://github.com/jiunbae/agent-skills)입니다.

## 설치

```bash
npm install --global @open330/agt
agt --version
```

지원 npm 플랫폼:

- macOS Apple Silicon (`darwin-arm64`)
- Linux x64 (`linux-x64`)
- Linux ARM64 (`linux-arm64`)

bootstrap 스크립트도 동일한 정식 npm 패키지를 설치합니다.

```bash
curl -fsSL https://raw.githubusercontent.com/Open330/agt/main/setup.sh | bash
```

Claude와 Codex에 Core 프로필을 함께 설치하려면:

```bash
curl -fsSL https://raw.githubusercontent.com/Open330/agt/main/setup.sh \
  | bash -s -- --core --codex
```

## 스킬 설치

```bash
# Claude: ~/.claude/skills(또는 $CLAUDE_CONFIG_DIR/skills) 아래 평면형 구조
agt skill install --profile core \
  --from jiunbae/agent-skills --global

# Codex: ~/.agents/skills 아래 평면형 구조
agt skill install --profile core \
  --from jiunbae/agent-skills --global --agent codex

agt skill list --installed --agent claude
agt skill list --installed --agent codex
agt skill update --agent codex
```

원격 설치본에는 `.remote-source`가 기록되므로 이후 `agt skill update`로
갱신할 수 있습니다. `agent-skills`의 `agt.toml` 규칙은 기존 사용자
파일을 덮어쓰지 않고 static context를 병합합니다. 건너뛰려면 `--no-static`을
사용합니다.

Claude Code는 `<skills-dir>/<skill>/SKILL.md`만 읽습니다. 이전 agt가
`<group>/<skill>` 구조로 설치한 스킬은 로드되지 않으므로 다음으로 옮깁니다.

```bash
agt skill migrate --global --dry-run   # 미리 보기
agt skill migrate --global
```

`agt skill status --global`는 설치된 스킬마다 어느 프로필에서 왔는지(skills 디렉터리
옆 `agt-state.json`에 기록)와 agt가 설치하지 않은 항목을 보여줍니다.

`~/.claude`가 아닌 Claude 설정 디렉터리를 쓰려면 `--claude-dir <dir>`(또는
`CLAUDE_CONFIG_DIR`)을 지정합니다. 스킬, 훅, 팀, `settings.json`에 모두
적용됩니다.

## 프로필

스킬 저장소는 `profiles.yml`(과 루트의 다른 `*.yml`)에 프로필을 정의합니다.

```yaml
core:
  description: "Essential skills"
  skills: [development/git-commit-pr, security/security-auditor]

full:
  extends: core          # 여러 개면 [core, dev]
  groups: [agents, development]
```

`agt skill install --profile core,full`은 여러 프로필의 합집합을 설치합니다.
`all`은 저장소의 모든 스킬입니다.

## 레이어

`agt apply`는 머신별 비공개 파일 `~/.config/agt/layers.toml`(`AGT_LAYERS`로
변경 가능)에 맞춰 스킬 디렉터리를 정리합니다. **레이어**는 소스 하나의 프로필
하나, **스택**은 레이어의 순서 있는 목록, **타깃**은 스택을 전역 스킬 디렉터리나
디렉터리 트리에 적용하는 단위입니다.

```toml
[sources]
personal = "~/workspace/agent-skills"
team     = "~/work/agents"

[stack.base]
layers = [
  { source = "personal", profile = "core,dev" },
  { source = "personal", skills = ["integrations/vault-secrets"] },  # one-off picks
]
static = ["personal"]            # 전역 타깃에 적용할 때 이 소스의 [[setup.copy]] 실행

[stack.work]
extends = "base"
layers  = [{ source = "team", profile = "team-core" }]

[[target]]
path  = "global"
stack = "base"

[[target]]
path  = "~/work"                 # 하위 git 리포에 work 스택 적용
stack = "work"
```

```bash
agt apply --dry-run      # 계획만 보기
agt apply                # 링크, 등록(adopt), 제거(prune)
agt apply --check        # 바뀔 것이 있으면 exit 1
```

- Claude Code는 세션을 시작한 디렉터리의 프로젝트 스킬만 읽습니다. 그래서
  디렉터리 타깃은 `<dir>/.claude/skills`와 그 바로 아래 각 git 리포에 설치하고,
  링크는 각 리포의 `.git/info/exclude`에 추가합니다.
- `apply`는 자신이 설치한 스킬(`agt-state.json`에 기록)만 제거합니다. 다른 것이
  같은 이름을 차지하고 있으면 덮어쓰지 않고 경고만 냅니다.
- 두 레이어가 같은 이름의 스킬을 제공하면 오류입니다. 뒤 레이어에
  `override = true`를 지정하면 교체합니다.

훅은 사용자 설정과 세션 자신의 디렉터리에서만 읽고 상위 디렉터리에서는 읽지
않습니다. 전역 훅을 특정 트리에서만 실행하려면 명령을 감쌉니다.

```json
{ "type": "command", "command": "agt gate ~/work -- ~/work/agents/scripts/digest.sh" }
```

`agt gate`는 `$CLAUDE_PROJECT_DIR`이 해당 디렉터리 안일 때만 명령을 실행하고,
그 밖에서는 아무것도 출력하지 않고 0으로 종료합니다.

## 로컬 소스 탐색 순서

1. `AGT_DIR` 또는 `AGENT_SKILLS_DIR`
2. 실행 파일 주변의 스킬 저장소
3. `~/.agent-skills`, 이후 레거시 `~/.agt`, `~/agt`
4. 대화형 설치기가 제안하는 현재 Git 저장소

권장 구성:

```bash
git clone https://github.com/jiunbae/agent-skills ~/workspace/agent-skills
ln -s ~/workspace/agent-skills ~/.agent-skills
```

## 주요 명령

```text
agt skill        Claude/Codex 스킬 관리
agt persona      리뷰어 페르소나 설치·사용
agt hook         Claude Code 훅 관리
agt team         협업 에이전트 팀 실행
agt run          스킬 자동 매칭으로 프롬프트 실행
agt completions  셸 자동완성 생성
```

세부 옵션은 `agt <command> --help`에서 확인합니다.

## 개발

```bash
cargo test --locked --manifest-path agt/Cargo.toml
cargo build --release --manifest-path agt/Cargo.toml
node npm/scripts/verify-platform-packages.js
```

저장소 소유권 원칙:

- CLI 변경과 npm 릴리스는 이 저장소에서만 진행합니다.
- 스킬 콘텐츠 변경은 `jiunbae/agent-skills`에서만 진행합니다.
- 이 저장소에는 중복 스킬 카탈로그를 포함하거나 배포하지 않습니다.

## 릴리스

Gitea Actions가 활성 플랫폼 3개와 wrapper를 빌드·배포하고 npm 버전을
검증한 뒤 동일한 GitHub Release tarball을 업로드합니다. 릴리스 태그는
`vYYYY.M.D` 형식을 사용합니다.

## 라이선스

MIT
