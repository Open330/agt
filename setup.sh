#!/usr/bin/env bash
#
# agt — Setup Script
#
# Installs agt binary and optionally clones the agent-skills repository.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash -s -- --skills
#   curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash -s -- --skills --core
#
# Options:
#   --skills            Clone agent-skills repository to ~/.agent-skills
#   --core              Run install.sh --core after cloning skills
#   --npm               Install via npm instead of binary download
#   --uninstall         Uninstall agt and optionally skills
#   -h, --help          Help
#

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

REPO="open330/agt"
SKILLS_REPO="jiunbae/agent-skills"
SKILLS_DIR="${HOME}/.agent-skills"
BIN_DIR="${HOME}/.local/bin"

INSTALL_SKILLS=false
INSTALL_CORE=false
USE_NPM=false
UNINSTALL=false

log_info()    { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[OK]${NC} $1"; }
log_warn()    { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error()   { echo -e "${RED}[ERROR]${NC} $1" >&2; }

usage() {
    cat << 'EOF'
agt — Setup Script

Usage:
  curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash
  curl -fsSL ... | bash -s -- [options]

Options:
  --skills            Clone agent-skills to ~/.agent-skills
  --core              Also install core skills (implies --skills)
  --npm               Install via npm instead of binary download
  --uninstall         Uninstall everything
  -h, --help          Help

Examples:
  # Install agt binary only
  curl -fsSL https://raw.githubusercontent.com/open330/agt/main/setup.sh | bash

  # Install agt + clone skills + install core skills
  curl -fsSL ... | bash -s -- --skills --core

  # Install via npm (if you prefer)
  curl -fsSL ... | bash -s -- --npm

EOF
    exit 0
}

detect_platform() {
    local os arch target
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)

    case "$os" in
        linux)  os="unknown-linux-musl" ;;
        darwin) os="apple-darwin" ;;
        *)      log_error "Unsupported OS: $os"; exit 1 ;;
    esac

    case "$arch" in
        x86_64|amd64)  arch="x86_64" ;;
        arm64|aarch64) arch="aarch64" ;;
        *)             log_error "Unsupported architecture: $arch"; exit 1 ;;
    esac

    echo "${arch}-${os}"
}

get_latest_tag() {
    local tag
    tag=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | \
        grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
    echo "$tag"
}

install_binary() {
    local target tag url tmp_dir

    target=$(detect_platform)
    log_info "Detected platform: $target"

    tag=$(get_latest_tag)
    if [[ -z "$tag" ]]; then
        log_error "Could not determine latest release"
        exit 1
    fi
    log_info "Latest release: $tag"

    url="https://github.com/${REPO}/releases/download/${tag}/agt-${target}.tar.gz"
    log_info "Downloading: $url"

    tmp_dir=$(mktemp -d)
    trap "rm -rf $tmp_dir" EXIT

    if command -v curl &>/dev/null; then
        curl -fsSL "$url" | tar -xz -C "$tmp_dir"
    elif command -v wget &>/dev/null; then
        wget -qO- "$url" | tar -xz -C "$tmp_dir"
    else
        log_error "curl or wget required"
        exit 1
    fi

    mkdir -p "$BIN_DIR"
    mv "$tmp_dir/agt" "$BIN_DIR/agt"
    chmod +x "$BIN_DIR/agt"
    log_success "Installed: $BIN_DIR/agt"

    # Check if BIN_DIR is in PATH
    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$BIN_DIR"; then
        log_warn "$BIN_DIR is not in your PATH"
        log_info "Add to your shell profile: export PATH=\"$BIN_DIR:\$PATH\""
    fi
}

install_npm() {
    if ! command -v npm &>/dev/null; then
        log_error "npm not found. Install Node.js first or use binary install (without --npm)"
        exit 1
    fi
    log_info "Installing via npm..."
    npm install -g @open330/agt
    log_success "Installed via npm"
}

clone_skills() {
    if [[ -d "$SKILLS_DIR" ]]; then
        log_info "Skills directory exists, pulling latest..."
        cd "$SKILLS_DIR" && git pull --ff-only 2>/dev/null || true
        cd ->/dev/null
    else
        log_info "Cloning agent-skills..."
        git clone "https://github.com/${SKILLS_REPO}.git" "$SKILLS_DIR"
    fi
    log_success "Skills at: $SKILLS_DIR"

    if [[ "$INSTALL_CORE" == "true" ]]; then
        local install_script="$SKILLS_DIR/install.sh"
        if [[ -f "$install_script" ]]; then
            log_info "Installing core skills..."
            chmod +x "$install_script"
            "$install_script" --core
        else
            log_warn "install.sh not found in $SKILLS_DIR"
        fi
    fi
}

uninstall() {
    log_info "Uninstalling agt..."

    if [[ -f "$BIN_DIR/agt" ]]; then
        rm -f "$BIN_DIR/agt"
        log_success "Removed: $BIN_DIR/agt"
    fi

    if command -v npm &>/dev/null; then
        npm uninstall -g @open330/agt 2>/dev/null && log_success "Removed npm package" || true
    fi

    if [[ -d "$SKILLS_DIR" ]]; then
        log_warn "Skills directory preserved: $SKILLS_DIR"
        log_info "To remove skills: rm -rf $SKILLS_DIR"
    fi

    log_success "Uninstall complete"
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --skills)    INSTALL_SKILLS=true; shift ;;
        --core)      INSTALL_SKILLS=true; INSTALL_CORE=true; shift ;;
        --npm)       USE_NPM=true; shift ;;
        --uninstall) UNINSTALL=true; shift ;;
        -h|--help)   usage ;;
        *)           log_error "Unknown option: $1"; exit 1 ;;
    esac
done

# Main
echo ""
echo -e "${CYAN}╔════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║     ▄▀█ █▀▀ ▀█▀  Setup                ║${NC}"
echo -e "${CYAN}║     █▀█ █▄█  █                         ║${NC}"
echo -e "${CYAN}╚════════════════════════════════════════╝${NC}"
echo ""

if [[ "$UNINSTALL" == "true" ]]; then
    uninstall
    exit 0
fi

# Phase 1: Install agt binary
if [[ "$USE_NPM" == "true" ]]; then
    install_npm
else
    install_binary
fi

# Phase 2: Optionally clone skills
if [[ "$INSTALL_SKILLS" == "true" ]]; then
    echo ""
    clone_skills
fi

echo ""
echo -e "${GREEN}════════════════════════════════════════${NC}"
echo -e "${GREEN}Setup complete!${NC}"
echo ""
echo "Next steps:"
if [[ "$INSTALL_SKILLS" != "true" ]]; then
    echo "  # Get skills (optional):"
    echo "  git clone https://github.com/${SKILLS_REPO} ~/.agent-skills"
    echo ""
fi
echo "  # List available skills:"
echo "  agt skill list"
echo ""
echo "  # Install a skill to your project:"
echo "  cd your-project"
echo "  agt skill install <name>"
echo ""
echo "Docs: https://github.com/${REPO}"
echo -e "${GREEN}════════════════════════════════════════${NC}"
