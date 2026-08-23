# Changelog

## [2.0.7] - 2026-08-23

### Added
- Agnes AI provider section in Quick Setup, with both routes (`api.agnes-ai.cn`
  and `apihub.agnes-ai.com`) selectable and a four-model picker
- Built-in `agnes-cn` and `agnes-global` profile templates, wiring all three
  Claude tier slots to `agnes-2.5-flash` so the model is reachable from Claude
  Code's picker, which lists only `claude-`-prefixed ids
- Shared `src/domain/agnes.ts` so both preset lists and the panel read one source

### Fixed
- **Codex made no tool calls through a bridged stream.** The bridged Responses
  path used a stream adapter that revealed tool calls only inside the closing
  `response.completed` snapshot, emitting no `response.output_item.added` or
  `response.function_call_arguments.delta`. Codex reads tool calls from the
  incremental events, so every call was invisible — the model asked to run
  `exec_command`, Codex showed an empty reply and did nothing. It now uses the
  core adapter, which emits the incremental events and restores a bridged custom
  tool's original name. Affected any bridged streaming upstream, not just Agnes
- `responses_function` read as "gateway recommended" rather than required. That
  verdict means the upstream refused a `custom` tool while probing, which is
  exactly when Codex cannot reach it directly, so switching the gateway off
  produced `tools[7].type: unknown variant \`custom\`` on the first turn
- Probing discarded the user's client selection: the detection effect re-derived
  it on every protocol change, dropping `claude-code` and leaving Agnes tier slots
  unreachable. `getSmartClients` also never offered Claude clients for an OpenAI
  upstream, which is wrong whenever the gateway is translating
- Image and video models from `/v1/models` were saved as chat models, putting
  `agnes-video-2.5` in Codex's catalogue where it can only fail
- Responses bridge dropped `reasoning_content`, so a Chat upstream that spent its
  whole output budget reasoning produced an empty `output: []`. It now becomes a
  `reasoning` output item, matching what a native Responses upstream returns

### Removed
- `crates/gateway/src/stream_adapter.rs`. No callers remained after the streaming
  fix, and a public adapter that silently drops tool calls invites re-wiring

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
