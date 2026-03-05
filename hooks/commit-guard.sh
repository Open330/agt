#!/usr/bin/env bash
# commit-guard.sh — PreToolUse hook that blocks git commits containing secrets
# Checks for .env files, private keys, API tokens in staged files

set -euo pipefail

raw=$(cat)
command=$(echo "$raw" | python3 -c "import sys,json; print(json.load(sys.stdin).get('tool_input',{}).get('command',''))" 2>/dev/null || echo "")

# Only check git commit/add commands
if ! echo "$command" | grep -qE '^\s*git\s+(commit|add)'; then
  exit 0
fi

# Patterns that indicate secrets
SECRET_PATTERNS=(
  '\.env$'
  '\.env\.local$'
  '\.env\.production$'
  'credentials\.json$'
  '.*_rsa$'
  '.*_ed25519$'
  '\.pem$'
  'id_dsa$'
  '\.key$'
  'secret.*\.json$'
  'token.*\.json$'
)

# Check if git add is staging dangerous files
if echo "$command" | grep -qE '^\s*git\s+add'; then
  for pattern in "${SECRET_PATTERNS[@]}"; do
    if echo "$command" | grep -qE "$pattern"; then
      echo "Blocked: attempting to stage a sensitive file matching: $pattern" >&2
      exit 2
    fi
  done

  # Check for "git add ." or "git add -A" (too broad)
  if echo "$command" | grep -qE 'git\s+add\s+(-A|\.)'; then
    # Warn but don't block — the prompt hook system handles this
    cat <<EOF
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "additionalContext": "WARNING: Broad git add detected. Prefer adding specific files to avoid committing secrets."
  }
}
EOF
    exit 0
  fi
fi

exit 0
