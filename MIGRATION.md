# Migration: Split CLI and Skill Catalog

`Open330/agt` is now the CLI and release repository. Skill content remains in
`jiunbae/agent-skills`.

| Before | Current standard |
|---|---|
| Skill catalog duplicated in `Open330/agt` | Catalog exists only in `jiunbae/agent-skills` |
| CLI source duplicated in `agent-skills` | CLI exists only in `Open330/agt` |
| `setup.sh` downloaded a bundled catalog | `setup.sh` installs the published npm CLI |
| `--from open330/agt` | `--from jiunbae/agent-skills` |

## Reinstall

```bash
npm install --global @open330/agt
agt skill install --profile core --from jiunbae/agent-skills --global
agt skill install --profile core --from jiunbae/agent-skills --global --agent codex
```

Existing remote-installed skills keep working. Their `.remote-source` metadata
continues to point to `jiunbae/agent-skills` and can be refreshed with:

```bash
agt skill update
agt skill update --agent codex
```

## Maintainers

- Do not add skill groups, personas, hooks, or static context to this repo.
- Do not add Rust/npm CLI sources or npm release workflows to `agent-skills`.
- Publish `@open330/agt` only from the tracked Gitea release workflow here.
