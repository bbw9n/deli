#!/bin/sh
query="${1:-${DELI_QUERY:-avg(rate(container_cpu_usage_seconds_total[5m]))}}"
cat <<EOF
{
  "title": "Production CPU Load: ${query}",
  "series": [
    {
      "name": "api",
      "unit": "%",
      "points": [
        { "timestamp": 1712300000, "value": 41.0 },
        { "timestamp": 1712300060, "value": 44.5 },
        { "timestamp": 1712300120, "value": 46.2 },
        { "timestamp": 1712300180, "value": 42.8 }
      ]
    },
    {
      "name": "worker",
      "unit": "%",
      "points": [
        { "timestamp": 1712300000, "value": 33.4 },
        { "timestamp": 1712300060, "value": 35.1 },
        { "timestamp": 1712300120, "value": 36.8 },
        { "timestamp": 1712300180, "value": 34.2 }
      ]
    }
  ]
}
EOF
