# CLAUDE.md ? AI Coding Assistant Guidance

## Project Overview
PolyDeck (v2.1.1) is a Tauri 2 + React 19 desktop application that manages AI development environments.
The version of record is `Cargo.toml` / `package.json`; do not restate it in code.

## Architecture
- **Rust workspace** with 3 crates: `core`, `gateway`, `inject`
- **Tauri app** with modular IPC commands in `src-tauri/src/commands/`
- **React frontend** with Jotai state management and shadcn/ui components

## Key Conventions
- All IPC commands use `ad_` prefix
- Responses<->Chat bridge is in `crates/core/src/responses_chat.rs` (shared by gateway)
- Gateway MUST only bind to loopback addresses
- Credentials stored via keyring, NEVER in config files
- CSP is enabled in `tauri.conf.json` - do not set to null
- Clients bind to profiles individually; one gateway routes by per-client bearer
  token. There is no single "active profile"
- Never return a success result from an unimplemented path. Return an error or
  report the capability as unsupported
- Use `toml_edit` for TOML writes to preserve comments

## Testing
```bash
cargo fmt --all -- --check                              # formatting
cargo clippy --workspace --all-targets -- -D warnings   # lints
cargo test --workspace                                  # Rust tests
npm run lint && npx tsc --noEmit                        # frontend lint + types
npm test                                                # Vitest
npm run test:e2e                                        # Playwright
```

## Build
```bash
npm run tauri build
```
