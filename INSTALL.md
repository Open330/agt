# Installation Guide

Quick guide to installing and managing skills from this repository.

## Prerequisites

- Claude Code installed
- Git (for cloning and version control)
- Python 3.8+ (for scripts)

## Quick Start

```bash
# Clone the repository
git clone <repository-url> ~/workspace/agent-skills
cd ~/workspace/agent-skills

# Install all skills
python install.py

# Or install specific skills
python install.py context-manager multi-llm-agent
```

## Using the Install Script

### Basic Installation

```bash
# Install all skills (symlink mode)
python install.py

# Install specific skills only
python install.py context-manager git-commit-pr

# View available skills
python install.py --list
```

### Naming Options (Prefix/Postfix)

Use prefix and postfix to organize or differentiate skill installations:

```bash
# Add prefix (e.g., my-context-manager)
python install.py --prefix "my-"

# Add postfix (e.g., context-manager-dev)
python install.py --postfix "-dev"

# Combine both (e.g., team-context-manager-v2)
python install.py --prefix "team-" --postfix "-v2"
```

This is useful when:
- Running multiple versions of the same skill
- Separating personal vs team skills
- Testing new versions alongside stable ones

### Installation Methods

```bash
# Symlink (default) - changes sync automatically
python install.py

# Copy - independent installation
python install.py --copy
```

### Custom Target Directory

```bash
# Install to custom location
python install.py --target-dir ~/.claude/skills-dev

# Install to different Claude Code instance
python install.py -t /path/to/other/.claude/skills
```

### Dry Run (Preview)

```bash
# Preview what will happen without making changes
python install.py --dry-run
python install.py --prefix "test-" --dry-run
```

### Uninstall

```bash
# Remove all skills installed from this repository
python install.py --uninstall

# Remove specific skills
python install.py --uninstall context-manager

# Remove with prefix (if installed with prefix)
python install.py --prefix "my-" --uninstall
```

## Command Reference

```
usage: install.py [-h] [--prefix PREFIX] [--postfix POSTFIX]
                  [--target-dir TARGET_DIR] [--source-dir SOURCE_DIR]
                  [--copy] [--dry-run] [--uninstall] [--list] [--quiet]
                  [skills ...]

options:
  skills                  Skills to install/uninstall (default: all)
  --prefix PREFIX         Add prefix to skill names (e.g., 'my-')
  --postfix, --suffix     Add postfix to skill names (e.g., '-dev')
  --target-dir, -t        Target directory (default: ~/.claude/skills)
  --source-dir, -s        Source directory (default: script location)
  --copy, -c              Copy files instead of symlink
  --dry-run, -n           Preview only, no actual changes
  --uninstall, -u         Uninstall mode
  --list, -l              List available skills
  --quiet, -q             Minimal output
```

## Examples

### Scenario 1: Development Setup

Keep skills synced with your repository:

```bash
# Install via symlinks (default)
python install.py

# Edit skills in repository
vim context-manager/SKILL.md

# Changes immediately available in Claude Code
```

### Scenario 2: Separate Dev and Production

```bash
# Production skills
python install.py --postfix "-stable"

# Development skills (symlinked for editing)
python install.py --postfix "-dev"
```

### Scenario 3: Team Shared Skills

```bash
# Personal copy
python install.py --prefix "june-"

# Team shared version
python install.py --prefix "team-" --copy
```

### Scenario 4: Testing New Versions

```bash
# Keep current version
python install.py --postfix "-v1"

# Test new version
python install.py --postfix "-v2-test" context-manager
```

## Directory Structure

```
~/workspace/agent-skills/         # Source repository
├── install.py                    # Installation script
├── context-manager/              # Skill: context-manager
├── git-commit-pr/                # Skill: git-commit-pr
├── multi-llm-agent/              # Skill: multi-llm-agent
└── proposal-analyzer/            # Skill: proposal-analyzer

~/.claude/skills/                 # Claude Code skills directory
├── context-manager -> ~/workspace/agent-skills/context-manager
├── my-context-manager -> ~/workspace/agent-skills/context-manager
└── context-manager-dev/          # Copied version
```

## Manual Installation

If you prefer manual installation:

### Symlink (Recommended for Development)

```bash
cd ~/.claude/skills
ln -s ~/workspace/agent-skills/context-manager context-manager
```

### Copy

```bash
cp -r ~/workspace/agent-skills/context-manager ~/.claude/skills/
```

## Verifying Installation

```bash
# List installed skills
ls -la ~/.claude/skills/

# Check skill metadata
head -n 5 ~/.claude/skills/context-manager/SKILL.md

# Test script (if applicable)
python ~/.claude/skills/context-manager/scripts/find_context.py --help
```

## Troubleshooting

### Skill Not Recognized

1. Verify SKILL.md frontmatter:
   ```bash
   head -n 10 ~/.claude/skills/context-manager/SKILL.md
   ```
   Should have:
   ```yaml
   ---
   name: context-manager
   description: ...
   ---
   ```

2. Check permissions:
   ```bash
   ls -la ~/.claude/skills/context-manager/
   ```

### Broken Symlink

```bash
# Remove and reinstall
python install.py --uninstall context-manager
python install.py context-manager
```

### Permission Denied on Scripts

```bash
chmod +x ~/.claude/skills/*/scripts/*.py
```

## Syncing Across Machines

```bash
# Machine A: push changes
cd ~/workspace/agent-skills
git add . && git commit -m "Update skills" && git push

# Machine B: pull and reinstall
cd ~/workspace/agent-skills
git pull
python install.py
```

---

**For more information**, see [README.md](README.md)
