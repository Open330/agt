# 0001 — Agent 환경 매니페스트(`agt sync`)와 진단(`agt doctor`)

- 상태: Draft (Q1, Q4 결정 반영)
- 작성일: 2026-10-01
- 범위: `agt` CLI (`Open330/agt`)

## 1. 배경

`gh skill install`이 스킬 하나를 GitHub에서 가져와 40여 개 에이전트 디렉터리에
배치하는 일을 잘 해낸다. 태그·SHA pin, `search`/`preview`/`publish`, frontmatter
기반 업데이트 추적까지 갖췄고, 사실상 공식 설치 경로가 되어 가고 있다.
`agt skill install --from`이 하던 일과 대부분 겹친다.

`gh skill`이 다루지 않는 영역은 다음과 같다.

1. **선언과 재현**: "이 repo에서 일하려면 어떤 스킬·hook·persona가 필요한가"를
   파일로 선언하고 팀 전원이 같은 환경을 갖게 하는 수단이 없다. 명령을 하나씩
   실행하는 방식이다.
2. **스킬 이외의 구성요소**: hook(settings.json), persona, team 템플릿은 설치
   대상이 아니다.
3. **설치 이후**: 스킬이 많아질수록 생기는 trigger 충돌, context 토큰 비용,
   미사용 스킬, 업데이트 시 스크립트 변경 검토를 아무도 다루지 않는다.

agt의 포지셔닝을 다음과 같이 옮긴다.

> `gh skill`은 스킬을 **가져온다**. `agt`는 그 위에서 팀의 에이전트 환경을
> **선언하고, 재현하고, 진단한다.**

agt는 `gh skill`과 경쟁하지 않고 그 위에 올라간다. GitHub에서 스킬을 받는 일은
`gh skill`에 위임한다(결정 D-1).

## 2. 목표 / 비목표

### 목표
- G1. repo에 커밋하는 `agt.toml`로 skill·hook·persona를 선언한다.
- G2. `agt.lock`으로 모든 원격 의존성을 commit SHA와 tree SHA로 고정한다.
- G3. `agt sync` 한 번으로 Claude/Codex 양쪽 환경을 lock과 일치시킨다(멱등).
- G4. 업데이트 시 실행 가능한 내용(스크립트, hook 명령)의 변경을 보여 주고 승인을 받는다.
- G5. `agt doctor`가 설치된 스킬 전체(agt·gh skill·수동 설치 포함)를 진단한다.
- G6. `gh skill`과 완전히 호환된다. agt가 설치한 스킬은 `gh skill list`에도 그대로
  보이고, `gh skill`로 설치한 스킬은 `agt doctor`와 `agt adopt`의 대상이 된다.
- G7. 개인 전역 환경도 같은 방식으로 관리한다(`~/.config/agt/agt.toml`, 결정 D-2).

### 비목표
- GitHub 다운로드, 인증, 스킬 탐색 로직 자체 구현 (`gh skill`에 위임)
- 스킬 검색·퍼블리시 레지스트리 (`gh skill search/publish`)
- 40개 에이전트 전체 지원 (Claude, Codex 우선. 나머지는 `agents` 값 확장으로 열어 둔다)
- MCP 서버 설치 (별도 RFC)

## 3. 결정 사항

| ID | 결정 | 근거 |
|---|---|---|
| D-1 | GitHub 원격 스킬 다운로드는 `gh skill install`에 위임한다. hook·persona 등 스킬이 아닌 파일도 `gh api`(tarball)로 받는다. 즉 GitHub 접근은 전부 `gh`를 거친다 | `gh skill`이 공식 경로가 되고 있다. 인증·rate limit·스펙 변경을 `gh`가 따라간다. agt의 자체 토큰 처리(`remote/github.rs`)를 걷어낼 수 있다 |
| D-2 | 사용자 전역 매니페스트 `~/.config/agt/agt.toml`(+`agt.lock`)를 지원한다 | 개인 환경도 선언·재현 대상이다. 머신을 옮길 때 dotfiles로 가져갈 수 있다 |

### 3.1 `gh skill` 위임 검증 결과 (gh 2.102.0)

실제로 실행해 확인한 동작:

- `gh skill install <repo> <path>/SKILL.md --pin <sha> --dir <staging> --force`:
  비대화형으로 동작하고, `<staging>/<name>/`에 설치된다.
- 설치 시 `SKILL.md` frontmatter를 다시 쓴다. `metadata.github-{repo,path,ref,pinned,tree-sha}`를
  넣고 키 순서도 바꾼다. 따라서 **원본 바이트 해시를 integrity로 쓸 수 없다** → 5장 참고.
- `github-tree-sha`는 해당 디렉터리의 git tree SHA다.
  `gh api repos/{o}/{r}/contents/{parent}?ref={sha}`가 돌려주는 `sha`와 일치함을 확인했다.
  설치하지 않고도 원격의 변경 여부를 판정할 수 있다.
- **jiunbae/agent-skills의 그룹 레이아웃(`<group>/<name>/SKILL.md`)은 자동 탐색되지 않는다**
  ("no skills found"). 다만 `development/git-commit-pr/SKILL.md`처럼 경로를 명시하면 설치된다.
  → agt가 경로를 해석해서 명시적으로 넘긴다. 장기적으로는 카탈로그를
  `skills/<group>/<name>/`(gh가 지원하는 `skills/{scope}/*/SKILL.md`)로 옮기는 것을 권장한다(Q6).
- **`gh skill install`은 실행 비트를 떨어뜨린다.** git에서 `100755`인 스크립트가 `644`로
  설치되어 `./scripts/x.sh` 호출이 깨진다. agt는 resolve 때 받은 git tree의 mode로
  실행 비트를 복구하고, 그 목록을 lock의 `executables`에 기록한다.
- `--pin`으로 설치하면 `gh skill update`가 해당 스킬을 건너뛴다. 사용자가
  `gh skill update --all`을 실행해도 lock이 깨지지 않는다.
- `gh skill list --json path,skillName,sourceURL,version,pinned,scope,agentHosts`로
  40개 host 전체의 설치 현황을 얻을 수 있다(약 0.2초). 다만 **`~/.claude/skills` 아래의 심링크
  스킬은 목록에 나오지 않는다.** 그래서 doctor는 Claude/Codex 경로를 직접 스캔하고
  (심링크와 그룹 레이아웃 포함), 그 밖의 host만 `gh skill list`에서 합친다.

## 4. 사용자 시나리오

```bash
# 팀 리드: repo에 환경 선언
agt add skill rtzr/callabo-skills callabo-release
agt add skill jiunbae/agent-skills development/git-commit-pr
agt add hook  jiunbae/agent-skills pre-commit-lint
git add agt.toml agt.lock && git commit

# 팀원: clone 후 한 번
agt sync            # Claude + Codex 프로젝트 스코프에 설치
agt sync --check    # CI용: 불일치하면 exit 1

# 개인 전역 환경 (dotfiles에 ~/.config/agt/ 포함)
agt add -g skill anthropics/skills pdf
agt sync -g

# 업데이트
agt outdated [-g]
agt update callabo-release    # diff 보여 주고 승인 → lock 갱신

# 진단 (전역 + 현재 프로젝트 모두)
agt doctor
```

## 5. 매니페스트: `agt.toml`

### 5.1 위치와 스코프

| 매니페스트 | lock | 설치 대상 | 명령 |
|---|---|---|---|
| `<repo>/agt.toml` | `<repo>/agt.lock` | 프로젝트 스코프 (`.claude/skills`, `.agents/skills`, `.claude/settings.json`) | `agt sync` |
| `~/.config/agt/agt.toml` (`$XDG_CONFIG_HOME` 존중) | `~/.config/agt/agt.lock` | 사용자 스코프 (`~/.claude/skills`, `~/.agents/skills`, `~/.claude/settings.json`) | `agt sync -g` |

- **프로젝트 매니페스트는 사용자 스코프에 쓰지 못한다.** clone한 repo가 `~/`를
  건드리는 경로를 막기 위해서다. 그래서 `scope` 필드는 두지 않는다.
- 두 매니페스트는 서로 다른 디렉터리를 관리하므로 설치 단계에서 충돌하지 않는다.
  같은 이름이 양쪽에 있을 때 에이전트가 무엇을 쓰는지는 에이전트마다 다르므로
  doctor D2가 보고한다(Q5).

### 5.2 기존 `agt.toml`과의 관계

현재 `agt.toml`(`agt/src/config/manifest.rs`)은 **스킬 소스 저장소** 쪽 파일이다.
`[[setup.copy]]` 규칙으로 저장소 내용을 `~/` 아래에 복사한다. 새 매니페스트는
**소비자** 쪽 파일이다. 같은 파일명을 쓰고 top-level 테이블로 구분한다.

| 테이블 | 의미 | 언제 적용 |
|---|---|---|
| `[setup]` | 소스 저장소의 정적 복사 규칙 (기존) | 이 저장소가 **소스로 설치될 때만** |
| `[env]`, `[sources]`, `[skills]`, `[hooks]`, `[personas]` | 소비자 선언 (신규) | `agt sync` 실행 시 |

`agt sync`는 **현재 매니페스트의 `[setup]`을 실행하지 않는다.**

### 5.3 스키마

```toml
[env]
agents = ["claude", "codex"]     # 기본값: ["claude"]

# 별칭. 의존성 선언에서 반복을 줄인다.
[sources]
callabo = { github = "rtzr/callabo-skills", rev = "main" }
open330 = { github = "jiunbae/agent-skills", rev = "v2026.08.22.1" }

[skills]
callabo-release = { source = "callabo" }                               # 이름으로 탐색 (gh에 위임)
git-commit-pr   = { source = "open330", path = "development/git-commit-pr" }
pdf             = { github = "anthropics/skills", rev = "main" }       # inline 소스
local-helper    = { path = "./tools/skills/local-helper" }             # repo 내 로컬
core            = { source = "open330", profile = "core" }             # 프로필 펼치기

[skills.callabo-release]
agents = ["claude"]               # 에이전트별 예외 (기본: env.agents)

[hooks]
pre-commit-lint = { source = "open330" }

[personas]
security-reviewer = { source = "open330" }
```

규칙
- 의존성 키가 설치 이름이 된다.
- `rev`에는 branch, tag, SHA가 올 수 있다. 생략하면 gh와 같은 규칙을 따른다
  (최신 릴리스 태그, 없으면 기본 브랜치). lock에는 항상 SHA로 기록된다.
- `profile`은 소스 저장소의 `profiles.yml`(`config/profiles.rs`)을 `gh api`로 받아
  개별 스킬로 펼친 뒤 lock에 기록한다.
- 로컬 `path` 스킬은 `gh skill install --from-local`로 복사한다. 개발 중 즉시 반영이
  필요하면 `link = true`로 기존 agt 심링크 방식을 쓴다(gh 메타데이터는 붙지 않는다).

### 5.4 스킬 경로 해석

1. `path`가 명시되면 `<path>/SKILL.md`를 gh에 넘긴다.
2. 없으면 이름만 gh에 넘겨 gh 탐색 규칙을 따른다.
3. gh가 "no skills found"를 돌려주면 agt가 그룹 레이아웃(`<group>/<name>/SKILL.md`)을
   `gh api .../git/trees/{sha}?recursive=1`로 찾아 경로를 확정한다. 확정된 경로는
   lock에 기록하므로 이후에는 탐색하지 않는다.

## 6. Lockfile: `agt.lock`

```toml
version = 1

[[package]]
kind = "skill"
name = "callabo-release"
source = "github:rtzr/callabo-skills"
rev = "main"                                   # 선언된 값
commit = "3f9c1e0a..."                         # 해석된 commit SHA
path = "skills/callabo-release"
tree = "6369f464..."                           # gh의 github-tree-sha (원격 내용 식별자)
integrity = "agt1-sha256-Zk1..."               # 설치본 정규화 해시 (로컬 변조 감지)
executables = ["scripts/release.sh"]

[[package]]
kind = "hook"
name = "pre-commit-lint"
source = "github:jiunbae/agent-skills"
commit = "a71b..."
path = "hooks/pre-commit-lint"
tree = "..."
integrity = "agt1-sha256-..."
event = "PreToolUse"
executables = ["pre-commit-lint.sh"]
```

해시는 두 가지를 둔다.

- **`tree`** (원격 측): git tree SHA. `outdated`는 새 commit에서 `contents` API로 tree만 비교한다.
  repo의 다른 곳이 바뀌어도 이 스킬이 그대로면 "변경 없음"으로 판정한다.
  설치 후 frontmatter의 `github-tree-sha`가 lock의 `tree`와 다르면 설치를 중단한다.
- **`integrity`** (로컬 측): gh가 frontmatter를 다시 쓰기 때문에 정규화한 뒤 해시한다.
  - `SKILL.md`: frontmatter를 파싱해 `metadata.github-*` 키를 지우고, 키를 정렬해
    직렬화한 다음 본문과 이어 붙인다.
  - 나머지 파일: `(relative_path, exec_bit, sha256(content))`를 경로순으로 정렬한다.
  - 전체를 sha256 한다. 접두사 `agt1-`는 정규화 규칙의 버전이다.
- **`executables`**: G4의 승인 대상. 실행 비트 파일, `*.sh|*.py|*.js|*.ts`, hook command,
  frontmatter `allowed-tools`.

SHA 해석은 `gh api repos/{o}/{r}/commits/{rev} --jq .sha`로 한다.

## 7. 설치 상태와 소유권

`agt sync`는 **자기가 설치한 것만** 지우거나 덮어쓴다.

- 스킬의 출처 정보는 gh가 frontmatter에 넣은 `metadata.github-*`를 그대로 쓴다.
  agt 소유 여부만 별도로 표시한다. 각 설치 디렉터리에 `.agt-managed` 파일을 둔다.
  ```
  owner: project                     # 전역이면 ~/.config/agt/agt.toml 경로
  name: callabo-release
  integrity: agt1-sha256-Zk1...
  ```
  프로젝트 스킬 디렉터리는 repo 안에 있으므로 경로를 기록하지 않는다. 절대경로를
  쓰면 checkout 위치가 바뀌었을 때 소유권 판정이 깨진다.
  `gh skill`은 이 파일을 모르지만 디렉터리 안의 추가 파일이라 영향이 없다.
  기존 `.remote-source`는 읽기 호환만 유지한다.
- hook은 settings.json에 병합한다. 기존 `merge_hooks_into_settings`를 쓰고, 관리 목록은
  별도 상태 파일(`.claude/.agt-state.json`, 전역은 `~/.config/agt/state.json`)에 기록한다.
- 같은 이름의 비관리 디렉터리가 있으면 덮어쓰지 않고 에러를 낸다.
  `agt adopt <name>`으로 편입할 수 있다. gh로 설치된 스킬이라면 frontmatter의
  `github-*` 정보로 매니페스트 항목과 lock을 자동 생성한다.

## 8. 명령

| 명령 | 동작 |
|---|---|
| `agt init [-g] [--from-installed]` | 빈 `agt.toml`을 만든다. `--from-installed`면 `gh skill list --json`과 기존 설치본으로 역생성한다 |
| `agt add [-g] <kind> <repo> [<skill>]` | 매니페스트에 추가 → resolve → lock 갱신 → 설치 |
| `agt remove [-g] <name>` | 매니페스트, lock, 설치본에서 제거 |
| `agt sync [-g] [--check] [--frozen]` | lock 기준으로 설치·삭제한다. `--frozen`: lock이 매니페스트와 안 맞으면 실패(CI). `--check`: 변경 없이 차이만 보고 |
| `agt lock [-g]` | 매니페스트를 다시 resolve해 lock만 갱신한다 (설치 안 함) |
| `agt outdated [-g]` | 각 의존성의 `rev` 최신 commit에서 tree SHA를 비교한다 |
| `agt update [-g] [name]` | 새 commit으로 resolve → **변경 리뷰** → 승인 시 lock과 설치 갱신 |
| `agt adopt [-g] <name>` | 기존 설치본(gh·수동)을 매니페스트로 편입 |
| `agt doctor [--json] [--deep]` | 10장 참고. 전역과 현재 프로젝트를 모두 검사 |

`-g`는 `~/.config/agt/` 매니페스트를 대상으로 한다는 뜻이다. 기존 `agt skill install --from`은
deprecated로 표시하고 내부적으로 `gh skill install`을 호출하도록 바꾼다.

### 8.1 `agt sync` 알고리즘

```
require gh >= MIN_GH (gh skill 지원 버전); 없으면 설치 안내 후 종료
manifest = parse(agt.toml); lock = parse(agt.lock) or empty
if !frozen and lock.stale(manifest): lock = resolve(manifest, lock)  # 변경된 항목만
desired  = expand(lock, agents)            # (agent, kind, name) → package
actual   = scan_managed()                  # .agt-managed, state.json
plan     = diff(desired, actual)           # add / replace / remove
print(plan); if --check: exit(plan.empty ? 0 : 1)

for op in plan (skill):
  staging = cache/staging/<uuid>
  gh skill install github.com/{o}/{r} {path}/SKILL.md --pin {commit} --dir {staging} --force
  verify frontmatter github-tree-sha == lock.tree
  verify normalize_hash(staging/<name>) == lock.integrity   # 최초 resolve 때는 기록
  for agent in pkg.agents: copy to agent dir → write .agt-managed → atomic rename
for op in plan (hook/persona):
  gh api repos/{o}/{r}/tarball/{commit} → extract (기존 extract_archive 제한 재사용)
  verify tree → 기존 hook/persona 설치 로직
write lock (resolve가 일어났을 때만)
```

- 한 번 받은 스킬은 캐시(`~/.cache/agt/pkgs/<tree>/`)에서 여러 에이전트 디렉터리로 복사한다.
  gh 호출은 패키지당 한 번이다.
- 실패하면 해당 항목만 롤백하고 나머지 결과를 보고한다. 부분 실패면 exit 1.
- 캐시가 있으면 네트워크 없이 다시 실행할 수 있다.

### 8.2 `agt update` 변경 리뷰 (G4)

```
callabo-release  3f9c1e0 → 8b02d7a  (tree 6369f46 → 91c0a2e)
  SKILL.md                 description 변경
  scripts/release.sh       +14 −3   ⚠ 실행 파일
      + curl -fsSL "$DEPLOY_HOOK" ...        ⚠ 네트워크
      + rm -rf "$BUILD_DIR"                  ⚠ 삭제
  allowed-tools            + Bash(git push:*)   ⚠ 권한 확대
Apply? [y/N/d(전체 diff)]
```

- diff는 캐시에 있는 이전 tree와 새로 staging한 tree를 비교한다.
- 실행 파일, hook 명령, `allowed-tools`가 바뀐 경우에만 프롬프트를 띄운다. 문서만
  바뀌면 자동 승인한다.
- 위험 패턴은 정적 규칙으로 표시한다: `curl|wget|nc`, `rm -rf`, `eval`, base64 디코드
  후 실행, `~/.ssh`·`~/.aws` 접근, 권한 확대.
- `--yes`는 문서 변경만 자동 승인한다. 실행 파일 변경까지 넘기려면 `--yes-all`이 필요하다.

## 9. 비 GitHub 소스 (M5)

`gh`는 GitHub 전용이다. Gitea 등은 다음과 같이 처리한다.

```toml
[sources]
internal = { git = "https://gitea.example.com/team/skills.git", rev = "main" }
```

`git ls-remote`로 SHA를 해석하고, 캐시에 shallow fetch한 뒤
`gh skill install --from-local <cache>/<path>`로 설치한다. 이렇게 하면 메타데이터
형식이 GitHub 소스와 같아진다. tree는 `git rev-parse <commit>:<path>`로 구한다.

## 10. `agt doctor`

스캔 대상은 `gh skill list --json`(40개 host, project+user)과 agt가 아는 경로(Codex
`.agents/skills`, 심링크 설치본)의 합집합이다. 관리 여부와 무관하게 전부 본다.
출처는 `.agt-managed` → `metadata.github-*` → `.remote-source` → 없음(수동) 순으로 판별한다.

### 10.1 검사 항목

| ID | 검사 | 방법 | 기본 심각도 |
|---|---|---|---|
| D1 | **Lock 불일치**: 설치본이 lock integrity와 다름 (로컬 수정·변조) | 정규화 해시 재계산 | error |
| D2 | **중복 이름**: 같은 이름이 user/project, agt/gh에 중복 설치 | 이름 집계 | warn |
| D3 | **Trigger 충돌**: description이 서로 많이 겹치는 스킬 쌍 | 아래 10.2 | warn |
| D4 | **Context 예산**: 세션 시작마다 로드되는 description 총 토큰 | 문자 수 기반 추정, 상위 기여자 표시 | info, 임계치 초과 시 warn |
| D5 | **미사용 스킬**: 최근 N일 동안 한 번도 발동하지 않음 | 아래 10.3 | info |
| D6 | **frontmatter 오류**: name 누락, description 과다·과소, 디렉터리명과 `name` 불일치 | `frontmatter/parser.rs` | error / warn |
| D7 | **깨진 링크**: 대상이 없는 심링크, 실행 비트 없는 스크립트 | fs 검사 | error |
| D8 | **원격 소스 만료**: 고정된 commit에 접근 불가(저장소 삭제·비공개 전환) | `gh api`, `--online`일 때만 | warn |
| D9 | **미관리 스킬**: 매니페스트가 있는데 그 밖에서 설치된 스킬 | 스캔 결과 − lock | info, `agt adopt` 제안 |

출력 예:
```
agt doctor
  ✗ D1 callabo-release: 설치본이 lock과 다름 (scripts/release.sh 수정됨)
  ⚠ D3 code-review ↔ pr-review: description 유사도 0.81 — 서로 잘못 발동할 수 있음
  ⚠ D4 스킬 description 합계 ≈ 6.2k tokens (임계 4k). 상위: skill-a 1.1k, skill-b 0.9k …
  · D5 최근 30일 미사용 7개: sql-tuner, k8s-debug, …  →  agt remove 제안
  · D9 gh로 설치된 pdf가 agt.toml에 없음  →  agt adopt -g pdf
```

### 10.2 Trigger 충돌 (D3)

- 기본(오프라인): description과 "Triggers on …" 문구를 토큰화한 뒤 TF-IDF 코사인
  유사도를 구한다. 0.75 이상인 쌍을 보고한다. 한국어·영어가 섞인 trigger 문구를
  고려해 문자 n-gram(3)도 함께 쓴다.
- `--deep`: 기존 `llm/invoke.rs`로 후보 쌍만 LLM에 넘겨 "이 프롬프트들이 어느 스킬로
  가야 하는가"를 판정받는다. 비용이 들기 때문에 명시적으로 요청할 때만 실행한다.

### 10.3 사용 통계 (D5)

- Claude: `~/.claude/projects/*/*.jsonl`에서 `tool_use.name == "Skill"`인 항목의
  `input.skill`을 집계한다. 사용자가 `/skill` 명령으로 직접 호출한 경우도 포함한다.
- Codex: `~/.codex/sessions/**` 로그에서 스킬 파일 읽기 이벤트를 집계한다(형식 확인 필요, Q3).
- 통계는 로컬에서만 계산하고 어디에도 전송하지 않는다. `--no-transcripts`로 끌 수 있다.

## 11. 모듈 구조 (안)

```
agt/src/
  config/manifest.rs     # [setup] 유지 + 신규 EnvManifest 파싱, 전역 경로
  gh/                    # 신규: gh CLI 래퍼 (버전 확인, skill install, api, list --json)
  environment/           # 신규 (`env/`는 .gitignore의 ENV/와 충돌)
    lock.rs              # agt.lock 읽기/쓰기
    integrity.rs         # 정규화 해시 (frontmatter github-* 제거)
    resolve.rs           # rev → commit, 경로 해석, tree 조회, profile 펼치기
    plan.rs              # desired vs actual diff
    apply.rs             # staging → 에이전트별 복사 → atomic rename, settings 병합
    review.rs            # update 변경 리뷰, 위험 패턴
    cache.rs
  doctor/                # 신규
    scan.rs  checks.rs  similarity.rs  usage.rs
  remote/github.rs       # 점진 제거: 다운로드는 gh로 이전, extract_archive만 유지
  cmd/
    env.rs               # init/add/remove/sync/lock/outdated/update/adopt
    doctor.rs
```

재사용하는 코드: `config::paths`의 에이전트별 경로, `config::profiles`,
`cmd::hook`의 settings 병합·검증, `util`의 트랜잭션 교체, `frontmatter`,
`remote::extract_archive`(크기·경로 제한).

테스트: `gh` 호출은 trait(`GhClient`)으로 감싸고, 단위 테스트에서는 fixture 디렉터리를
돌려주는 fake를 쓴다. 실제 gh를 쓰는 통합 테스트는 `#[ignore]`로 두고 CI에서 별도로 실행한다.

## 12. 단계별 계획

| 단계 | 내용 | 완료 기준 |
|---|---|---|
| **M1** (MVP, 구현됨) | `[skills]` GitHub 소스만. `gh` 래퍼, `init/add/remove/sync/lock`, 프로젝트·전역(`-g`) 매니페스트, tree+integrity, `.agt-managed`, Claude+Codex | 빈 머신에서 `agt sync` 결과가 lock과 일치. 두 번째 실행에서 변경 0. 결과가 `gh skill list`에 pinned로 보임 |
| **M2** (구현됨) | `agt doctor` D1·D2·D4·D6·D7·D9 (오프라인, 빠름), `agt adopt` | 1초 이내, `--json` 출력 |
| **M3** (구현됨) | `outdated/update` 변경 리뷰(G4), 캐시, `sync --frozen/--check` + GitHub Action 예시, `agt skill install --from` deprecate | CI에서 drift 감지 |
| **M4** | `[hooks]`, `[personas]`, 로컬 `path`/`link` 스킬 | settings.json에 관리 항목만 추가·제거 |
| **M5** | doctor D3·D5·D8 (+`--deep`), 비 GitHub git 소스 | 실제 스킬 30개 이상 환경에서 오탐률 검토 |

M1 구현 범위: `init/add/remove/sync/lock`(`lock --update`로 갱신). `[sources]` 별칭, 에이전트별
`agents`, `-g` 전역 매니페스트, 캐시(`~/Library/Caches/agt/pkgs/<tree>/`, Linux는 `~/.cache`)를 포함한다.
`profile`, 로컬 `path`, `adopt`, `outdated`, 변경 리뷰는 이후 단계로 미뤘다.

M2 구현 메모: `agt doctor [--json] [--budget N] [--no-gh]`는 error가 있으면 exit 1로 끝난다.
`agt adopt`는 비교하기 전에 lock의 `executables`에 실행 비트를 먼저 복구한다. 그래서 gh로
설치한 일반적인 사본은 그 자리에서 편입되고, 실제로 내용이 다른 사본만 캐시의
`adopted/`로 옮긴 뒤 다시 설치한다. D6은 Agent Skills 스펙(name ≤64, 소문자·하이픈,
디렉터리명과 일치 / description 1–1024자)을 따른다.

M3 구현 메모: CI 게이트는 `sync --frozen` 대신 `agt lock --check`(오프라인)로 했다. CI에는
설치본이 없으므로 `sync --check`는 의미가 없고, `--frozen`은 실제로 설치까지 한다.
`agt update`는 리뷰를 `similar`의 줄 diff로 만든다. `-y`는 "묻지 않음"이라 실행 파일이
바뀐 업데이트는 보류하고, `--yes-all`일 때만 모두 적용한다. 보류된 것이 있으면 exit 1로 끝난다.
`agt skill install --from`은 단일 스킬에만 deprecated 경고를 낸다. 원격 profile 설치는
bootstrap(`setup.sh --core`)이 쓰므로 유지한다.

M1과 M2만으로도 `gh skill`과 구분되는 기능이 성립한다("선언·재현·진단").
README 개편은 M2 시점에 한다.

## 13. 열린 질문

- ~~Q1. gh skill에 위임할지~~ → **D-1로 결정**: 위임한다.
- **Q2. 최소 gh 버전**: `gh skill`이 들어온 버전과 `--pin`, `--dir`, `list --json`이
  안정화된 버전을 확인해 `MIN_GH`로 정해야 한다. 현재 확인한 버전은 2.102.0이다.
  preview 기능이라 플래그가 바뀔 수 있으므로, gh 래퍼에서 출력 파싱을 한곳에 모은다.
- **Q3. Codex 사용 로그 형식**: 스킬 발동을 식별할 수 있는 이벤트가 있는지 조사가 필요하다.
- ~~Q4. user scope 매니페스트~~ → **D-2로 결정**: `~/.config/agt/agt.toml`.
- **Q5. 이름 충돌 정책**: 프로젝트 스코프와 user 스코프에 같은 이름이 있을 때 에이전트가
  어느 쪽을 쓰는지 에이전트별로 확인하고, D2 메시지에 반영해야 한다.
- **Q6. 카탈로그 레이아웃 이전**: `jiunbae/agent-skills`를 `skills/<group>/<name>/`으로 옮기면
  `gh skill install jiunbae/agent-skills <name>`이 그대로 동작하고 5.4의 우회 경로가 필요 없어진다.
  카탈로그 저장소에서 따로 진행한다.
- **Q7. `gh` 미설치 환경**: 원격 소스가 있는 매니페스트에서 `gh`가 없으면 실패시킬지,
  기존 ureq 다운로드로 fallback할지 정해야 한다. 제안: M1에서는 실패시키고 설치 안내를 출력한다.
