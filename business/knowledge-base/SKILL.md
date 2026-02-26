---
name: managing-knowledge-base
description: Organizes and maintains project knowledge bases, generates comprehensive documentation and manuals. Use for "문서화", "지식 베이스", "매뉴얼 작성", "가이드 생성", "API 문서", "README 생성", "knowledge base", "documentation" requests.
trigger-keywords: 문서화, 지식 베이스, 매뉴얼, 가이드, knowledge base, documentation, manual, wiki, reference
allowed-tools: Read, Write, Edit, Bash, Grep, Glob
priority: medium
tags: [documentation, knowledge-base, manual, writing]
---

# Knowledge Base Manager

Organize, generate, and maintain project documentation and knowledge bases.

## Overview

**핵심 기능:**
- Project knowledge base structure scaffolding
- Auto-generate documentation from code (API docs, README, ADRs)
- Create user guides, tutorials, and runbooks
- Cross-reference indexing with TOC and glossary
- Documentation freshness checks and link validation

## When to Use

**명시적 요청:**
- "문서화해줘", "지식 베이스 만들어줘"
- "매뉴얼 작성해줘", "가이드 생성해줘"
- "API 문서 만들어줘", "README 생성해줘"
- "Create knowledge base", "Generate documentation"

**자동 활성화:**
- New project without documentation structure
- After major feature completion requiring docs update
- When onboarding documentation is requested

## Knowledge Base Structure

### Standard Layout

```
docs/
├── README.md                 # Documentation index
├── getting-started/          # Onboarding & quickstart
│   ├── installation.md
│   ├── quickstart.md
│   └── configuration.md
├── guides/                   # How-to guides (task-oriented)
│   ├── user-guide.md
│   └── admin-guide.md
├── reference/                # Technical reference (API, CLI)
│   ├── api/
│   │   ├── overview.md
│   │   └── endpoints/
│   ├── cli.md
│   └── configuration.md
├── architecture/             # System design & decisions
│   ├── overview.md
│   ├── adr/                  # Architecture Decision Records
│   │   ├── 0001-use-postgresql.md
│   │   └── template.md
│   └── diagrams/
├── tutorials/                # Learning-oriented walkthroughs
│   └── first-project.md
├── operations/               # Deployment & ops runbooks
│   ├── deployment.md
│   ├── monitoring.md
│   └── troubleshooting.md
├── glossary.md               # Term definitions
└── CHANGELOG.md              # Version history
```

### Documentation Types (Diataxis Framework)

| Type | Purpose | Orientation |
|------|---------|------------|
| Tutorials | Learning | Learning-oriented |
| How-to Guides | Problem-solving | Task-oriented |
| Reference | Information | Information-oriented |
| Explanation | Understanding | Understanding-oriented |

## Workflow

### Step 1: Scaffold Knowledge Base

```bash
# Create standard documentation structure
mkdir -p docs/{getting-started,guides,reference/api/endpoints,architecture/adr/,architecture/diagrams,tutorials,operations}

# Create index
cat > docs/README.md << 'EOF'
# Project Documentation

## Getting Started
- [Installation](getting-started/installation.md)
- [Quickstart](getting-started/quickstart.md)
- [Configuration](getting-started/configuration.md)

## Guides
- [User Guide](guides/user-guide.md)

## Reference
- [API Reference](reference/api/overview.md)
- [CLI Reference](reference/cli.md)

## Architecture
- [System Overview](architecture/overview.md)
- [Decision Records](architecture/adr/)

## Operations
- [Deployment](operations/deployment.md)
- [Troubleshooting](operations/troubleshooting.md)
EOF
```

### Step 2: Generate API Documentation

```python
import ast
import os

def extract_api_docs(source_dir: str) -> dict:
    """Extract docstrings and function signatures from Python source."""
    docs = {}
    for root, _, files in os.walk(source_dir):
        for f in files:
            if not f.endswith(".py"):
                continue
            path = os.path.join(root, f)
            with open(path) as fh:
                tree = ast.parse(fh.read())
            for node in ast.walk(tree):
                if isinstance(node, (ast.FunctionDef, ast.ClassDef)):
                    name = node.name
                    docstring = ast.get_docstring(node) or ""
                    docs[f"{path}:{name}"] = {
                        "name": name,
                        "type": type(node).__name__,
                        "docstring": docstring,
                        "line": node.lineno,
                    }
    return docs
```

### Step 3: Create Architecture Decision Records

```markdown
# ADR Template (docs/architecture/adr/template.md)

# [NUMBER]. [TITLE]

Date: YYYY-MM-DD

## Status

Proposed | Accepted | Deprecated | Superseded by [ADR-XXXX]

## Context

What is the issue that we're seeing that is motivating this decision?

## Decision

What is the change that we're proposing and/or doing?

## Consequences

What becomes easier or more difficult to do because of this change?
```

### Step 4: Generate Glossary

```bash
# Scan documentation for terms marked with bold or backtick patterns
grep -rh '\*\*[A-Z][a-zA-Z ]*\*\*' docs/ | \
  sed 's/.*\*\*\([^*]*\)\*\*.*/\1/' | \
  sort -u > docs/glossary_candidates.txt
```

### Step 5: Validate Documentation

```bash
# Check for broken internal links
grep -roh '\[.*\](\.\/[^)]*\|[^http][^)]*\.md)' docs/ | \
  while read -r link; do
    target=$(echo "$link" | sed 's/.*(\(.*\))/\1/')
    [ ! -f "docs/$target" ] && echo "BROKEN: $link"
  done

# Find orphan pages (not linked from any other doc)
for f in $(find docs -name '*.md'); do
  basename=$(basename "$f")
  if ! grep -rq "$basename" docs/ --include='*.md' -l | grep -v "$f" > /dev/null 2>&1; then
    echo "ORPHAN: $f"
  fi
done

# Check freshness (files not updated in 90+ days)
find docs -name '*.md' -mtime +90 -exec echo "STALE: {}" \;
```

## Examples

### 예시 1: 새 프로젝트 문서화

```
사용자: 이 프로젝트에 문서화 구조를 만들어줘

Claude:
1. 프로젝트 구조 분석 (소스 코드, 설정 파일)
2. docs/ 디렉토리 스캐폴딩 생성
3. README.md 인덱스 자동 생성
4. 코드에서 API 문서 추출
5. getting-started/quickstart.md 초안 작성

Generated structure:
docs/
├── README.md (index with 12 links)
├── getting-started/quickstart.md
├── reference/api/overview.md (23 endpoints documented)
└── architecture/overview.md
```

### 예시 2: API 문서 생성

```
사용자: API 레퍼런스 문서를 생성해줘

Claude:
1. 소스 코드에서 엔드포인트/함수 시그니처 추출
2. docstring 기반으로 설명 생성
3. 요청/응답 예시 포함
4. reference/api/ 디렉토리에 구조화하여 저장

Generated: docs/reference/api/
├── overview.md
├── endpoints/users.md (8 endpoints)
├── endpoints/auth.md (4 endpoints)
└── endpoints/data.md (12 endpoints)
```

### 예시 3: 운영 매뉴얼 작성

```
사용자: 배포 및 운영 매뉴얼을 만들어줘

Claude:
1. Dockerfile, docker-compose, CI/CD 설정 분석
2. 배포 절차 문서화
3. 모니터링/알림 가이드 작성
4. 트러블슈팅 가이드 생성

Generated: docs/operations/
├── deployment.md (step-by-step deployment guide)
├── monitoring.md (metrics, dashboards, alerts)
└── troubleshooting.md (common issues & fixes)
```

## Configuration

### Documentation Config (docs/.docconfig.yaml)

```yaml
# Knowledge base configuration
project:
  name: "Project Name"
  version: "1.0.0"

structure:
  framework: diataxis    # diataxis | custom
  languages: [en, ko]    # Supported languages

generation:
  api_source: src/        # Source directory for API doc extraction
  include_private: false  # Include private/internal APIs
  example_format: curl    # curl | httpie | python

validation:
  check_links: true
  check_freshness: true
  freshness_days: 90
  require_glossary: false
```

## Best Practices

**DO:**
- Follow the Diataxis framework (tutorials, guides, reference, explanation)
- Keep documentation close to the code it describes
- Include runnable examples in API documentation
- Use ADRs for architectural decisions
- Validate links and freshness regularly

**DON'T:**
- Put everything in a single README
- Duplicate information across documents (link instead)
- Use date-based filenames (git tracks history)
- Write documentation without considering the audience
- Skip the glossary for domain-specific terminology

## Troubleshooting

### 문제 1: 문서가 코드와 동기화되지 않음
```bash
# Pre-commit hook으로 문서 검증 추가
cat > .git/hooks/pre-commit << 'HOOK'
#!/bin/bash
# Check if API source changed but docs didn't
if git diff --cached --name-only | grep -q "^src/" && \
   ! git diff --cached --name-only | grep -q "^docs/"; then
  echo "WARNING: Source changed but docs not updated"
fi
HOOK
chmod +x .git/hooks/pre-commit
```

### 문제 2: 깨진 링크 발견
- `docs/.docconfig.yaml`에서 `check_links: true` 설정
- CI 파이프라인에 링크 검증 단계 추가
- 상대 경로 사용 시 디렉토리 기준 확인

## Resources

- [Diataxis Framework](https://diataxis.fr/) - Documentation system framework
- [ADR Tools](https://github.com/npryce/adr-tools) - Architecture Decision Records
- `docs/.docconfig.yaml` - Knowledge base configuration
