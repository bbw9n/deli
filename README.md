# deli

`deli` is a local-first Rust TUI for day-to-day DevOps control-plane work.

Current v1 MVP includes:

- pane-based terminal UI with document, config, worktree, and monitoring views
- built-in provider traits with command-backed adapters
- markdown and Mintlify document normalization
- typed dataframe support with JSON/CSV/NDJSON export
- gnuplot chart planning with kitty-image or text-fallback rendering modes

## Run

```bash
cargo run -- --config deli.toml
```

## Test

```bash
cargo test
```
