# Changelog

## [Unreleased]

### Added
- Agnes AI provider section in Quick Setup, with both routes (`api.agnes-ai.cn`
  and `apihub.agnes-ai.com`) selectable and a four-model picker
- Built-in `agnes-cn` and `agnes-global` profile templates, wiring all three
  Claude tier slots to `agnes-2.5-flash` so the model is reachable from Claude
  Code's picker, which lists only `claude-`-prefixed ids
- Shared `src/domain/agnes.ts` so both preset lists and the panel read one source

### Fixed
- Responses bridge dropped `reasoning_content`, so a Chat upstream that spent its
  whole output budget reasoning produced an empty `output: []`. It now becomes a
  `reasoning` output item, matching what a native Responses upstream returns

### Notes
- Agnes free tier 429s at 20 RPM and exposes no `RateLimit-*` headers, so the
  templates pin 20 rather than letting the probe leave its generic 60 in place
- Agnes API keys are scoped per site: a CN key lists models on the international
  host but 401s on inference. The route picker warns about this

## [2.0.0] - 2026-08-18

### Added
- Complete rewrite combining AI Deck v1, Provider Deck, and Relay Manager
- Modular IPC command architecture (11 modules, replacing 3039-line God File)
- Responses?Chat bidirectional protocol bridge in core crate
- XChaCha20-Poly1305 encrypted chat backup
- Cross-platform keyring (Windows + macOS + Linux)
- CSP security policy enabled
- toml_edit comment-preserving config writes
- Circuit breaker failover with auto-failback
- Stepwise suggestion service
- CDP injection with loopback verification
- Native user script management
- Worktree creation for developer workflow
- 6 frontend pages with lazy loading
- CI/CD with 3-platform matrix
