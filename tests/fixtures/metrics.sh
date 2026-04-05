#!/bin/sh
cat <<'EOF'
{
  "title":"CPU Load",
  "series":[
    {
      "name":"api",
      "unit":"%",
      "points":[
        {"timestamp":1712300000,"value":40.0},
        {"timestamp":1712300060,"value":42.5}
      ]
    }
  ]
}
EOF
