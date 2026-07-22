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
# Claude: ~/.claude/skills 아래 그룹형 구조
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
파일을 덮어쓰지 않고 static context를 병합합니다.

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
