# Token Watch

A macOS menu bar app that monitors your AI coding tool usage in real time — so you're never caught off guard by a rate limit.

Tracks **Claude Code** and **Codex CLI** usage windows, remaining credits, and reset timers, all from a compact tray popup.

---

## Features

**Live usage monitoring**
- Claude Code: 5-hour and 7-day rate limit windows, model-specific limits (Opus/Sonnet), extra credits, subscription type
- Codex CLI: primary and secondary rate limit windows, credit availability, plan type
- Countdown timers showing exactly when each window resets

**Smart caching**
- Per-service TTL caching (Claude: 2m, Codex: 1m) to minimize API calls
- Automatic stale data fallback during rate limits or network errors
- Serialized concurrent fetches — no duplicate requests
- Auto token refresh on 401 with graceful retry scheduling

**Clean tray UI**
- Compact 300×360 popup lives below the menu bar, out of your way
- Full dashboard window for deeper stats
- Color-coded progress bars (green → yellow → red as usage climbs)
- No Dock icon — lives entirely in the menu bar

---

## Requirements

- macOS (Apple Silicon or Intel)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated
- [Codex CLI](https://github.com/openai/codex) installed and authenticated (optional)

Token Watch reads your existing credentials — no additional API keys needed.

---

## Install

Download the latest `.dmg` from [Releases](../../releases) and drag Token Watch to your Applications folder.

Or build from source:

```bash
bun install
bunx tauri build
```

Requires [Bun](https://bun.sh) and [Rust](https://rustup.rs).

---

## Development

```bash
bun install
bunx tauri dev
```

Hot reload is enabled for both the React frontend and Rust backend.

---

## Contributing

Bug reports and pull requests are welcome. For larger changes, open an issue first to discuss the approach.

```bash
# Type check
bun run build

# Lint (Rust)
cargo clippy --target aarch64-apple-darwin
```

---

## License

MIT
