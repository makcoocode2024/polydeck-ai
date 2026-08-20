# CLAUDE.md ? AI Coding Assistant Guidance

## Project Overview
AI Deck v2.0.0 is a Tauri 2 + React 19 desktop application that manages AI development environments.

## Architecture
- **Rust workspace** with 3 crates: `core`, `gateway`, `inject`
- **Tauri app** with modular IPC commands in `src-tauri/src/commands/`
- **React frontend** with Jotai state management and shadcn/ui components

## Key Conventions
- All IPC commands use `ad_` prefix
- Responses?Chat bridge is in `crates/core/src/responses_chat.rs` (shared by gateway)
- Gateway MUST only bind to loopback addresses
- Credentials stored via keyring, NEVER in config files
- CSP is enabled in `tauri.conf.json` ? do not set to null
- Use `toml_edit` for TOML writes to preserve comments

## Testing
```bash
cargo test --workspace    # Rust tests
npm test                  # Vitest
npm run test:e2e          # Playwright
```

## Build
```bash
npm run tauri build
```
