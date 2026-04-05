#!/bin/sh
cat <<'EOF'
{
  "columns": [
    {"name":"service","kind":"string"},
    {"name":"replicas","kind":"int"},
    {"name":"healthy","kind":"bool"}
  ],
  "rows": [
    ["api",4,true],
    ["worker",2,false]
  ]
}
EOF
