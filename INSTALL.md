# Installation Guide

Quick guide to installing and managing skills from this repository.

## Prerequisites

- Claude Code installed
- Git (for cloning and version control)
- Python 3.7+ (for script-based skills)

## Directory Structure

```
~/personal/agent-skills/          # Source repository
└── context-manager/              # Skill sources

~/.claude/skills/                 # Claude Code skills directory
└── context-manager -> ~/personal/agent-skills/context-manager  # Symlink
```

## Installation Methods

### Method 1: Symlink (Recommended for Development)

This method creates symbolic links, allowing you to edit skills in the repository and have changes immediately reflected.

```bash
# Install specific skill
cd ~/.claude/skills
ln -s ~/personal/agent-skills/context-manager context-manager

# Verify installation
ls -la ~/.claude/skills/context-manager
```

**Advantages**:
- Changes in repository immediately available
- Easy to update and iterate
- One source of truth

**Use when**:
- Actively developing skills
- Want to keep skills in version control
- Need easy updates

### Method 2: Copy/Extract (Recommended for Stable Use)

This method copies the skill files to Claude Code's directory.

```bash
# Install from zip
cd ~/.claude/skills
unzip ~/personal/agent-skills/context-manager.zip

# Or copy directly
cp -r ~/personal/agent-skills/context-manager ~/.claude/skills/
```

**Advantages**:
- Self-contained installation
- No dependencies on source repository
- Stable, won't change unexpectedly

**Use when**:
- Installing on different machine
- Want stable, unchanging version
- Sharing with others

## Installing New Skills

When you create or download new skills:

```bash
# 1. Add to repository
cd ~/personal/agent-skills
# ... create or copy skill ...

# 2. Install to Claude Code
cd ~/.claude/skills
ln -s ~/personal/agent-skills/new-skill new-skill

# 3. Verify
cat ~/.claude/skills/new-skill/SKILL.md
```

## Verifying Installation

Check that skills are properly installed:

```bash
# List installed skills
ls -la ~/.claude/skills/

# View skill metadata
head -n 5 ~/.claude/skills/context-manager/SKILL.md

# Test skill scripts (if any)
python ~/.claude/skills/context-manager/scripts/find_context.py --help
```

## Updating Skills

### With Symlinks

Updates are automatic:

```bash
cd ~/personal/agent-skills/context-manager
# ... make changes ...
git add .
git commit -m "Update context-manager"

# Changes immediately available in Claude Code
```

### With Copies

Re-install the skill:

```bash
# Remove old version
rm -rf ~/.claude/skills/context-manager

# Copy new version
cp -r ~/personal/agent-skills/context-manager ~/.claude/skills/

# Or extract new zip
unzip -o ~/personal/agent-skills/context-manager.zip -d ~/.claude/skills/
```

## Uninstalling Skills

```bash
# Remove symlink or directory
rm -rf ~/.claude/skills/context-manager

# Verify removal
ls ~/.claude/skills/
```

## Troubleshooting

### Skill Not Recognized

**Symptom**: Claude Code doesn't seem to use the skill

**Solutions**:
1. Verify SKILL.md has proper frontmatter:
   ```bash
   head -n 10 ~/.claude/skills/context-manager/SKILL.md
   ```
   Should show:
   ```yaml
   ---
   name: context-manager
   description: ...
   ---
   ```

2. Check file permissions:
   ```bash
   ls -la ~/.claude/skills/context-manager/
   ```

3. Restart Claude Code (if applicable)

### Symlink Broken

**Symptom**: Symlink shows as red or broken

**Solution**:
```bash
# Remove broken link
rm ~/.claude/skills/context-manager

# Recreate with absolute path
ln -s /home/june/personal/agent-skills/context-manager ~/.claude/skills/context-manager
```

### Script Not Executable

**Symptom**: Permission denied when running scripts

**Solution**:
```bash
chmod +x ~/.claude/skills/context-manager/scripts/*.py
```

## Repository Management

### Syncing Multiple Machines

If using this repository across machines:

```bash
# On machine A: push changes
cd ~/personal/agent-skills
git add .
git commit -m "Update skills"
git push

# On machine B: pull changes
cd ~/personal/agent-skills
git pull

# Skills with symlinks automatically updated
```

### Backup

The repository serves as your backup, but for extra safety:

```bash
# Create backup archive
cd ~/personal
tar -czf agent-skills-backup-$(date +%Y%m%d).tar.gz agent-skills/

# Or use git remote
cd ~/personal/agent-skills
git remote add origin <your-git-url>
git push -u origin main
```

## Development Workflow

When developing new skills:

1. **Create in repository**:
   ```bash
   cd ~/personal/agent-skills
   ~/.claude/plugins/marketplaces/anthropic-agent-skills/skill-creator/scripts/init_skill.py \
     my-new-skill --path .
   ```

2. **Install via symlink**:
   ```bash
   cd ~/.claude/skills
   ln -s ~/personal/agent-skills/my-new-skill my-new-skill
   ```

3. **Develop and test**:
   - Edit files in `~/personal/agent-skills/my-new-skill/`
   - Test in Claude Code
   - Iterate

4. **Package when ready**:
   ```bash
   cd ~/personal/agent-skills
   ~/.claude/plugins/marketplaces/anthropic-agent-skills/skill-creator/scripts/package_skill.py \
     my-new-skill .
   ```

5. **Commit to repository**:
   ```bash
   git add my-new-skill/ my-new-skill.zip
   git commit -m "Add my-new-skill"
   ```

## Quick Reference

```bash
# Install skill via symlink
ln -s ~/personal/agent-skills/SKILL_NAME ~/.claude/skills/SKILL_NAME

# Install skill via zip
unzip ~/personal/agent-skills/SKILL_NAME.zip -d ~/.claude/skills/

# List installed skills
ls ~/.claude/skills/

# Remove skill
rm -rf ~/.claude/skills/SKILL_NAME

# Update repository
cd ~/personal/agent-skills
git pull

# Create new skill
cd ~/personal/agent-skills
~/.claude/plugins/marketplaces/anthropic-agent-skills/skill-creator/scripts/init_skill.py \
  SKILL_NAME --path .
```

---

**For more information**, see [README.md](README.md)
