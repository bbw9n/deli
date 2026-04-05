#!/bin/sh
cat <<'EOF'
[
  {
    "id": "intro",
    "path": "README.md",
    "format": "markdown",
    "raw": "# Deli\n\nDeveloper control plane"
  },
  {
    "id": "runbook",
    "path": "docs/runbook.mdx",
    "format": "mintlify",
    "raw": "---\ntitle: Runbook\n---\n# Runbook\n\n<Info>\nDeploy carefully\n</Info>"
  }
]
EOF
