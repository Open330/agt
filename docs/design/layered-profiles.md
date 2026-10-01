# Layered profiles and declarative `agt apply`

Status: implemented (2026-10-01). Differences from the first draft are noted
under "Decisions after implementation".


## Problem

agt installs one profile from one source per run and keeps no record of what it
installed. That breaks down once skills come from more than one place — a
public personal repo, a private team repo — and should be active in different
contexts:

1. **Claude Code never sees agt-installed skills.** agt installs Claude skills
   as `<target>/<group>/<skill>/SKILL.md` (`config/paths.rs:226-236`), but
   Claude Code discovers only `<skills-dir>/<skill>/SKILL.md` — one level. Every
   profile install is silently inert for Claude.
2. **No composition.** `--profile` takes one value; profiles cannot extend each
   other; `--from` takes one source.
3. **No state.** agt cannot answer "what is installed here, from which layer",
   so it cannot remove skills that left a profile or switch a layer off.
4. **Static files come back.** `[[setup.copy]]` runs after every profile
   install with `strategy = "merge"` (`cmd/skill.rs:497-540, 838`); deleting a
   file in `~/.agents` only lasts until the next install. There is no opt-out.
5. **Fixed config dir.** `~/.claude` is hard-coded for skills, hooks and
   `settings.json` (`config/paths.rs:196-200, 257-267`); `CLAUDE_CONFIG_DIR` is
   ignored.

## Goals

- A **layer** = one source repo + one of its profiles (+ whether its static
  files and hooks are wanted).
- A **profile stack** = an ordered list of layers, applied to one **target**
  (global user dir, or a directory such as a team workspace root).
- `agt apply` makes a target match its stack exactly: installs what is missing,
  prunes what agt installed earlier but the stack no longer contains, never
  touches anything agt did not install.
- Team layers can be confined to a directory tree so they are inactive in
  unrelated sessions.

Non-goals: per-account (Claude login) switching; editing skill content.

## Configuration

User-level, private, never committed to a public repo:
`~/.config/agt/layers.toml`.

```toml
[sources]
personal = "~/workspace/agent-skills"     # local path
team     = "~/src/team-agents"

[stack.base]                               # applied to the global target
layers = [
  { source = "personal", profile = "core" },
  { source = "personal", profile = "dev" },
]
static = ["personal"]                      # whose [[setup.copy]] rules may run

[stack.team]
extends = "base"
layers  = [{ source = "team", profile = "team-core" }]
static  = ["team"]

[[target]]
path  = "global"                           # $CLAUDE_CONFIG_DIR or ~/.claude
stack = "base"

[[target]]
path  = "~/work"                           # team work lives under here
stack = "team"
```

Repo-side, `profiles.yml` (and extra `*.yml`) gain one optional field:

```yaml
dev:
  extends: core          # resolved within the same source
  groups: [development]
```

## Commands

| Command | Effect |
|---|---|
| `agt apply [--target <path>\|--all] [--dry-run]` | Converge the target(s) to their stack. |
| `agt skill status [--global]` | Show stack, installed skills and their layer, unmanaged and missing entries. |
| `agt gate <dir> -- <cmd>` | Run a hook command only inside a directory tree. |
| `agt skill install --profile a,b` | Comma list; shorthand for an ad-hoc stack. |
| `--claude-dir <dir>` (global flag) | Override the Claude config dir; default `$CLAUDE_CONFIG_DIR`, then `~/.claude`. |
| `--no-static` (install/apply) | Skip `[[setup.copy]]`. |

## State

Each skills directory gets `agt-state.json` beside it (global:
`<claude-dir>/agt-state.json`, a repo: `<repo>/.claude/agt-state.json`):

```json
{ "version": 1,
  "stack": "team",
  "skills": { "git-commit-pr": { "layer": "personal:core", "path": "...", "mode": "symlink" } },
  "static": { "~/.agents/VAULT.sample.md": { "layer": "personal" } } }
```

Prune removes only entries listed in state. Anything else in the skills dir is
reported as `unmanaged` and left alone.

## Layout fix

Claude targets become flat: `<skills-dir>/<skill>/SKILL.md`. A name collision
across layers is an error at plan time unless the later layer sets
`override = true` on that layer entry. Migration: `agt apply` detects the old
nested `<group>/<skill>` symlinks it (or older agt) created, and moves them flat.

## Directory-scoped layers in Claude Code

Facts (Claude Code docs, 2026-10): project skills load from the session's
working directory `.claude/skills` and from nested subdirectories for sessions
started below them; **ancestor `.claude/skills` loading is not documented**;
`.claude/settings.json` (hooks) is read **only from the primary working
directory**; `CLAUDE.md` **is** loaded from ancestors.

Consequences for a target like `~/work`:

- Skills: verify ancestor discovery empirically before relying on it. If it
  does not work, `apply` installs the layer into each git repo under the target
  (`<repo>/.claude/skills`, added to `.git/info/exclude`), recorded in state.
- Hooks: settings do not cascade, so a directory-scoped hook must be installed
  globally and gate itself. agt ships a tiny `agt gate <target> -- <cmd>` wrapper:
  the hook command runs only when `$CLAUDE_PROJECT_DIR` is under the target.
- Instructions: a layer may ship a `CLAUDE.md` fragment placed at the target
  root; ancestor loading makes it apply to every session below.

## Rollout

1. Flat Claude layout + migration; `--claude-dir`; `--no-static`. (Fixes the
   inert-install bug on its own.)
2. `extends` in profile files; comma-list `--profile`.
3. State file, `agt status`.
4. `layers.toml`, `agt apply` with prune, hook gate, per-repo fallback.

Each step ships independently and keeps existing commands working.

## Decisions after implementation

- **Ancestor discovery does not work.** Tested on Claude Code (2026-10-01): a
  session started in a git repo below a directory with `.claude/skills` does
  not see those skills; a session started in that directory does. Directory
  targets therefore install into the directory itself and into every git repo
  directly below it, each with its own state file and `.git/info/exclude`
  block.
- **Ownership.** Records carry `applied: true` when `agt apply` made them. Only
  those are relinked or pruned; `agt skill install` records and unmanaged
  entries are left alone, and a correct existing symlink is adopted.
- **Static files** run only when a stack is applied to the `global` target, so
  a directory-scoped layer cannot write to `~/.agents`.
- **Hooks** are not installed by layers. `agt gate` is the building block; a
  team wraps its own hook command with it.
- **Sources** are local paths for now; `owner/repo` sources are future work.
- **Status** is `agt skill status` per directory rather than a top-level
  command.

## Open questions

- Codex: same model with `~/.agents/skills` as the global target; directory
  scoping for Codex is out of scope until its discovery rules are confirmed.
- Remote (`owner/repo`) layer sources, and nested repos deeper than one level
  below a directory target.
