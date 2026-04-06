# Observability Checklist

Use this checklist before and after production-facing changes.

- CPU and memory are within expected bounds
- Error rate is flat or improved
- Queue depth is stable
- Background workers drain normally

## Dashboards

- `api-latency`
- `worker-throughput`
- `rollout-health`

## Notes

If the terminal does not support kitty graphics, deli falls back to text summaries for chart panes.
