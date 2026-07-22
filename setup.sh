#!/usr/bin/env bash
#
# agt — CLI bootstrap
#
# Installs the published @open330/agt package. Skill content is always fetched
# from jiunbae/agent-skills; this repository does not bundle a skill catalog.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

PACKAGE='@open330/agt'
SKILLS_SOURCE='jiunbae/agent-skills'
VERSION='latest'
INSTALL_PROFILE=''
INSTALL_CODEX=false
UNINSTALL=false

log_info() { printf '%b[INFO]%b %s\n' "$BLUE" "$NC" "$1"; }
log_success() { printf '%b[OK]%b %s\n' "$GREEN" "$NC" "$1"; }
log_warn() { printf '%b[WARN]%b %s\n' "$YELLOW" "$NC" "$1"; }
log_error() { printf '%b[ERROR]%b %s\n' "$RED" "$NC" "$1" >&2; }

usage() {
  cat <<'EOF'
agt — CLI bootstrap

Usage:
  curl -fsSL https://raw.githubusercontent.com/Open330/agt/main/setup.sh | bash
  curl -fsSL https://raw.githubusercontent.com/Open330/agt/main/setup.sh | bash -s -- [options]

Options:
  --version VERSION   npm package version (default: latest)
  --core              Install the agent-skills core profile globally
  --all               Install all agent-skills globally
  --codex             Also install selected skills for Codex
  --cli               Compatibility flag; the CLI is always installed
  --static            Compatibility flag; remote manifests handle static files
  --uninstall         Uninstall only the @open330/agt CLI package
  -h, --help          Show this help

Examples:
  # CLI only
  curl -fsSL https://raw.githubusercontent.com/Open330/agt/main/setup.sh | bash

  # CLI plus Core for Claude and Codex
  curl -fsSL https://raw.githubusercontent.com/Open330/agt/main/setup.sh \
    | bash -s -- --core --codex
EOF
}

require_npm() {
  if ! command -v npm >/dev/null 2>&1; then
    log_error 'npm is required. Install Node.js, then retry.'
    exit 1
  fi
}

resolve_agt_bin() {
  local prefix candidate
  prefix=$(npm prefix --global)
  candidate="${prefix}/bin/agt"
  if [[ -x "$candidate" ]]; then
    printf '%s\n' "$candidate"
    return
  fi
  if command -v agt >/dev/null 2>&1; then
    command -v agt
    return
  fi
  log_error 'agt was installed but its executable could not be located.'
  exit 1
}

install_cli() {
  local npm_version="${VERSION#v}"
  log_info "Installing ${PACKAGE}@${npm_version}"
  npm install --global "${PACKAGE}@${npm_version}"
  local agt_bin
  agt_bin=$(resolve_agt_bin)
  log_success "$("$agt_bin" --version) installed at $agt_bin"
}

install_skills() {
  [[ -n "$INSTALL_PROFILE" ]] || return 0

  local agt_bin
  agt_bin=$(resolve_agt_bin)
  local args=(skill install --from "$SKILLS_SOURCE" --global --force)
  if [[ "$INSTALL_PROFILE" == 'core' ]]; then
    args+=(--profile core)
  else
    args+=(--all)
  fi

  log_info "Installing ${INSTALL_PROFILE} skills for Claude from ${SKILLS_SOURCE}"
  "$agt_bin" "${args[@]}"

  if [[ "$INSTALL_CODEX" == true ]]; then
    log_info "Installing ${INSTALL_PROFILE} skills for Codex from ${SKILLS_SOURCE}"
    "$agt_bin" "${args[@]}" --agent codex
  fi
}

uninstall_cli() {
  require_npm
  npm uninstall --global "$PACKAGE"
  log_success "${PACKAGE} uninstalled. Existing skills were preserved."
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      [[ $# -ge 2 ]] || { log_error '--version requires a value'; exit 1; }
      VERSION="$2"
      shift 2
      ;;
    --core)
      INSTALL_PROFILE='core'
      shift
      ;;
    --all)
      INSTALL_PROFILE='all'
      shift
      ;;
    --codex)
      INSTALL_CODEX=true
      shift
      ;;
    --cli)
      shift
      ;;
    --static)
      log_warn '--static is no longer required; agent-skills/agt.toml handles static files.'
      shift
      ;;
    --uninstall)
      UNINSTALL=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      log_error "Unknown option: $1"
      usage >&2
      exit 1
      ;;
  esac
done

if [[ "$UNINSTALL" == true ]]; then
  uninstall_cli
  exit 0
fi

require_npm
install_cli
install_skills

printf '\n'
log_success 'agt setup complete.'
printf 'Skill source: https://github.com/%s\n' "$SKILLS_SOURCE"
