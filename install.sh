#!/usr/bin/env bash
#
# Agent Skills Installer
# 스킬을 ~/.claude/skills 디렉토리에 설치합니다.
#
# 사용법:
#   ./install.sh                     # 전체 설치
#   ./install.sh agents              # agents 그룹만 설치
#   ./install.sh agents development  # 여러 그룹 설치
#   ./install.sh agents/planning-agents  # 특정 스킬만 설치
#   ./install.sh --list              # 사용 가능한 스킬 목록
#   ./install.sh --uninstall agents  # 삭제
#

set -e

# 색상 정의
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# 기본값
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_DIR="${HOME}/.claude/skills"
PREFIX=""
POSTFIX=""
COPY_MODE=false
DRY_RUN=false
UNINSTALL=false
LIST_MODE=false
QUIET=false

# 그룹 정의 (GROUPS는 bash 내장 변수이므로 SKILL_GROUPS 사용)
SKILL_GROUPS=("agents" "development" "business")

# 사용법 출력
usage() {
    cat << EOF
사용법: $(basename "$0") [옵션] [그룹/스킬...]

스킬을 Claude Code의 skills 디렉토리에 설치합니다.

인자:
  그룹/스킬        설치할 그룹 또는 스킬 (기본: all)
                   예: agents, development, business
                   예: agents/planning-agents, development/git-commit-pr

옵션:
  -h, --help       도움말 표시
  -l, --list       사용 가능한 스킬 목록 표시
  -u, --uninstall  스킬 삭제
  -c, --copy       심볼릭 링크 대신 복사
  -n, --dry-run    실제 변경 없이 미리보기
  -q, --quiet      최소 출력
  --prefix PREFIX  스킬 이름 앞에 접두사 추가 (예: my-)
  --postfix POSTFIX 스킬 이름 뒤에 접미사 추가 (예: -dev)
  -t, --target DIR 대상 디렉토리 지정 (기본: ~/.claude/skills)

예시:
  $(basename "$0")                          # 전체 설치
  $(basename "$0") agents                   # agents 그룹만 설치
  $(basename "$0") agents development       # 여러 그룹 설치
  $(basename "$0") agents/planning-agents   # 특정 스킬만 설치
  $(basename "$0") --prefix "my-" agents    # 접두사 붙여서 설치
  $(basename "$0") --uninstall agents       # agents 그룹 삭제
  $(basename "$0") --list                   # 스킬 목록 표시

그룹:
  agents       AI 에이전트 관련 스킬 (multi-llm-agent, planning-agents)
  development  개발 도구 스킬 (git-commit-pr, context-manager)
  business     비즈니스 스킬 (proposal-analyzer)

EOF
    exit 0
}

# 로그 함수
log_info() {
    [[ "$QUIET" == "false" ]] && echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    [[ "$QUIET" == "false" ]] && echo -e "${GREEN}[OK]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

log_dry() {
    echo -e "${CYAN}[DRY-RUN]${NC} $1"
}

# 스킬 목록 가져오기
get_skills_in_group() {
    local group="$1"
    local group_dir="${SCRIPT_DIR}/${group}"

    if [[ ! -d "$group_dir" ]]; then
        return
    fi

    for skill_dir in "$group_dir"/*/; do
        if [[ -f "${skill_dir}SKILL.md" ]]; then
            basename "$skill_dir"
        fi
    done
}

# 모든 스킬 목록
get_all_skills() {
    for group in "${SKILL_GROUPS[@]}"; do
        for skill in $(get_skills_in_group "$group"); do
            echo "${group}/${skill}"
        done
    done
}

# 스킬 목록 출력
list_skills() {
    echo ""
    echo -e "${CYAN}사용 가능한 스킬${NC}"
    echo "================="
    echo ""

    for group in "${SKILL_GROUPS[@]}"; do
        local group_dir="${SCRIPT_DIR}/${group}"
        if [[ -d "$group_dir" ]]; then
            echo -e "${YELLOW}${group}/${NC}"

            for skill_dir in "$group_dir"/*/; do
                if [[ -f "${skill_dir}SKILL.md" ]]; then
                    local skill_name=$(basename "$skill_dir")
                    local description=$(grep -A1 "^description:" "${skill_dir}SKILL.md" 2>/dev/null | tail -1 | sed 's/^description: *//' | cut -c1-60)

                    if [[ -z "$description" ]]; then
                        description=$(sed -n '3p' "${skill_dir}SKILL.md" 2>/dev/null | sed 's/^description: *//' | cut -c1-60)
                    fi

                    printf "  ├── ${GREEN}%-25s${NC} %s\n" "$skill_name" "${description}..."
                fi
            done
            echo ""
        fi
    done

    echo "설치 예시:"
    echo "  ./install.sh all              # 전체 설치"
    echo "  ./install.sh agents           # agents 그룹만"
    echo "  ./install.sh agents/planning-agents  # 특정 스킬만"
    echo ""
}

# 스킬 설치
install_skill() {
    local group="$1"
    local skill="$2"
    local source_path="${SCRIPT_DIR}/${group}/${skill}"
    local target_name="${PREFIX}${skill}${POSTFIX}"
    local target_path="${TARGET_DIR}/${target_name}"

    # 소스 확인
    if [[ ! -d "$source_path" ]]; then
        log_error "스킬을 찾을 수 없습니다: ${group}/${skill}"
        return 1
    fi

    if [[ ! -f "${source_path}/SKILL.md" ]]; then
        log_error "SKILL.md가 없습니다: ${group}/${skill}"
        return 1
    fi

    # 이미 존재하는 경우
    if [[ -e "$target_path" ]]; then
        if [[ "$DRY_RUN" == "true" ]]; then
            log_dry "기존 스킬 덮어쓰기: $target_name"
        else
            log_warn "기존 스킬 덮어쓰기: $target_name"
            rm -rf "$target_path"
        fi
    fi

    # 설치
    if [[ "$DRY_RUN" == "true" ]]; then
        if [[ "$COPY_MODE" == "true" ]]; then
            log_dry "복사: ${group}/${skill} -> ${target_name}"
        else
            log_dry "심볼릭 링크: ${group}/${skill} -> ${target_name}"
        fi
    else
        if [[ "$COPY_MODE" == "true" ]]; then
            cp -r "$source_path" "$target_path"
            log_success "복사됨: ${group}/${skill} -> ${target_name}"
        else
            ln -s "$source_path" "$target_path"
            log_success "링크됨: ${group}/${skill} -> ${target_name}"
        fi
    fi
}

# 스킬 삭제
uninstall_skill() {
    local group="$1"
    local skill="$2"
    local target_name="${PREFIX}${skill}${POSTFIX}"
    local target_path="${TARGET_DIR}/${target_name}"

    if [[ ! -e "$target_path" ]]; then
        log_warn "설치되지 않음: $target_name"
        return 0
    fi

    if [[ "$DRY_RUN" == "true" ]]; then
        log_dry "삭제: $target_name"
    else
        rm -rf "$target_path"
        log_success "삭제됨: $target_name"
    fi
}

# 그룹 설치
install_group() {
    local group="$1"

    if [[ ! -d "${SCRIPT_DIR}/${group}" ]]; then
        log_error "그룹을 찾을 수 없습니다: $group"
        return 1
    fi

    log_info "그룹 설치 중: $group"

    for skill in $(get_skills_in_group "$group"); do
        if [[ "$UNINSTALL" == "true" ]]; then
            uninstall_skill "$group" "$skill"
        else
            install_skill "$group" "$skill"
        fi
    done
}

# 전체 설치
install_all() {
    log_info "전체 스킬 설치 중..."

    for group in "${SKILL_GROUPS[@]}"; do
        if [[ -d "${SCRIPT_DIR}/${group}" ]]; then
            install_group "$group"
        fi
    done
}

# 인자 파싱
TARGETS=()

while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            usage
            ;;
        -l|--list)
            LIST_MODE=true
            shift
            ;;
        -u|--uninstall)
            UNINSTALL=true
            shift
            ;;
        -c|--copy)
            COPY_MODE=true
            shift
            ;;
        -n|--dry-run)
            DRY_RUN=true
            shift
            ;;
        -q|--quiet)
            QUIET=true
            shift
            ;;
        --prefix)
            PREFIX="$2"
            shift 2
            ;;
        --postfix|--suffix)
            POSTFIX="$2"
            shift 2
            ;;
        -t|--target)
            TARGET_DIR="$2"
            shift 2
            ;;
        -*)
            log_error "알 수 없는 옵션: $1"
            echo "도움말: $(basename "$0") --help"
            exit 1
            ;;
        *)
            TARGETS+=("$1")
            shift
            ;;
    esac
done

# 목록 모드
if [[ "$LIST_MODE" == "true" ]]; then
    list_skills
    exit 0
fi

# 대상 디렉토리 생성
if [[ "$DRY_RUN" == "false" ]]; then
    mkdir -p "$TARGET_DIR"
fi

# 헤더 출력
if [[ "$QUIET" == "false" && "$DRY_RUN" == "false" ]]; then
    echo ""
    echo -e "${CYAN}Agent Skills Installer${NC}"
    echo "======================="
    echo ""
    if [[ "$UNINSTALL" == "true" ]]; then
        echo -e "모드: ${RED}삭제${NC}"
    elif [[ "$COPY_MODE" == "true" ]]; then
        echo -e "모드: ${YELLOW}복사${NC}"
    else
        echo -e "모드: ${GREEN}심볼릭 링크${NC}"
    fi
    echo -e "대상: ${TARGET_DIR}"
    [[ -n "$PREFIX" ]] && echo -e "접두사: ${PREFIX}"
    [[ -n "$POSTFIX" ]] && echo -e "접미사: ${POSTFIX}"
    echo ""
fi

# 대상이 없으면 전체 설치
if [[ ${#TARGETS[@]} -eq 0 ]] || [[ "${TARGETS[0]}" == "all" ]]; then
    if [[ "$UNINSTALL" == "true" ]]; then
        log_info "전체 스킬 삭제 중..."
        for group in "${SKILL_GROUPS[@]}"; do
            for skill in $(get_skills_in_group "$group"); do
                uninstall_skill "$group" "$skill"
            done
        done
    else
        install_all
    fi
else
    # 지정된 대상 처리
    for target in "${TARGETS[@]}"; do
        if [[ "$target" == *"/"* ]]; then
            # 특정 스킬 (예: agents/planning-agents)
            group="${target%%/*}"
            skill="${target#*/}"

            if [[ "$UNINSTALL" == "true" ]]; then
                uninstall_skill "$group" "$skill"
            else
                install_skill "$group" "$skill"
            fi
        else
            # 그룹 전체 (예: agents)
            if [[ "$UNINSTALL" == "true" ]]; then
                log_info "그룹 삭제 중: $target"
                for skill in $(get_skills_in_group "$target"); do
                    uninstall_skill "$target" "$skill"
                done
            else
                install_group "$target"
            fi
        fi
    done
fi

# 완료 메시지
if [[ "$QUIET" == "false" ]]; then
    echo ""
    if [[ "$DRY_RUN" == "true" ]]; then
        echo -e "${CYAN}[DRY-RUN 완료]${NC} 실제 변경 없음"
    elif [[ "$UNINSTALL" == "true" ]]; then
        echo -e "${GREEN}삭제 완료!${NC}"
    else
        echo -e "${GREEN}설치 완료!${NC}"
        echo ""
        echo "설치된 스킬 확인:"
        echo "  ls -la ${TARGET_DIR}/"
    fi
    echo ""
fi
