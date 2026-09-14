# Changelog

## [Unreleased]

## [2.2.1] - 2026-09-14

版本号是 patch，内容不全是——界面基座整层重做了。编号沿用工作区里已定的 2.2.1。

### Added
- **Agnes 3.0 Flash。** 快速配置的默认模型换成 `agnes-3.0-flash`（512K 上下文、
  65,536 最大输出）。刊例价 ¥0.35/M 输入、¥1.00/M 输出，但现价三项都是 ¥0，与 2.5 Flash
  当初同一种促销，因此仍标为免费档
- 四个共享 UI 件：`Tabs`、`StatTile`、`KeyValue`、`CodeSnippet`，替掉各页手写的重复实现

### Changed
- **设计 token 整层重做。** 此前是未改过的 shadcn 默认值，其中三项等于没做事：所有色相
  饱和度 `0%`、`--card` 与 `--background` 数值相同、`--muted`/`--secondary`/`--accent`
  共用一个值。于是没有品牌色、卡片无法读作层次、三个语义角色是同一种颜色。页面只能各自
  手写补偿——全站 151 处原始 emerald/amber/sky 类，明暗两套无人对齐；字号碎成 181 处
  任意像素值，仅 `text-[11px]` 就出现 115 次。现在有品牌色、卡片从背景抬起、三个中性
  角色分开，状态色成为三元组（ink / chip 字色 / chip 底色），另加阴影层级、圆角层级、
  11px 档与中文字体栈
- 侧栏选中态从整条实心 primary 改为浅底加品牌色竖条——六条实心排下来，侧栏会是全屏最
  抢眼的东西
- 编辑方案的 Modal 从 768px 放到 1024px；三列网格断点从 `sm` 推到 `lg`（1280px 窗口
  扣掉侧栏只剩约 1040px，`sm` 下每格约 195px，标签必换行）
- `CardTitle` 默认字号从 `text-2xl` 降到 `text-base`——全站 34 个调用点每一处都在覆盖它
- 历史页三张统计卡改为对筛选后的会话求和。它们就压在被筛选的列表上方，而且没有任何文案
  说明是全局统计，筛到单个客户端时列表变窄、数字不动，会被读成该客户端的数据

### Fixed
- **删除按钮静止态现在就带危险色。** 此前是 `ghost` 加只在悬停时生效的
  `hover:text-destructive`，鼠标到达之前与「编辑」逐像素相同
- **侧栏那个绿色状态点删掉了。** 它是写死的脉动动画，不读任何状态，网关停止时依然宣称
  健康。真实状态在状态栏
- `agnes-2.0-flash` 的上下文窗口从 512K 修正为 256K。窗口表按 `-flash` 子串一刀切，而
  官方对 2.0 与 1.5 publish 的是 256K（2.0 曾短暂开到 1M，2026-06 回滚）。这个数字会
  写进 `CLAUDE_CODE_MAX_CONTEXT_TOKENS`，等于按两倍于上游接受的长度排期会话
- `agnes-2.5-pro-alpha` 更正为 `agnes-2.5-pro-beta`。这个 id 必须准确：Agnes 对未知模型
  返回 503，而 503 属于可重试状态，拼错的表象是「上游故障」加熔断，不是「名字不对」
- 方案编辑器两个 Tab 的计数徽章写了 `py-0.2`——Tailwind 没有这一档，静默失效为零内距
- 客户端页不再把已废弃哨兵 `ai-deck-local` 当接入凭证展示。2.1.0 起网关按每客户端 bearer
  路由并拒绝该值，照抄必然 401；现在读真实 token，未绑定方案时明确说明拿不到而不是给一个
  必然失败的值
- 三处静默失效的死类：`shadow-xs` 不是 Tailwind v3 类；两处 `animate-in fade-in` 依赖
  未安装的 `tailwindcss-animate`

### Notes
- 界面改动的视觉效果由用户在真实窗口中确认，未经自动化视觉验证

## [2.2.0] - 2026-09-13

### Added
- **Claude Code 参数面板。** 编辑方案新增「Claude Code 参数」Tab，参数存 Profile 级。
  输出上限与思考上限按主 provider 的实测能力预填：手填值 > 上次探测结果 > 内置表。
  只有**带签名**的思考块才算支持（与网关的 `is_injectable` 同一判据）——未签名的思考
  块客户端存不下这一轮，注入 `MAX_THINKING_TOKENS` 会让整个会话失败，因此按不支持处理
- **settings.json env 回显。** 参数 Tab 底部显示 `~/.claude/settings.json` 的 `env`
  块当前真实内容，而不只是「将要写入什么」。这个文件是合并写入而非整体替换，上一个方案
  留下的键、或者手改的值，只能从文件本身看出来。凭证在 core 侧就打码，只报字符数，不过
  IPC 边界；判据是键名后缀（`_API_KEY` / `_AUTH_TOKEN` / `_SECRET` / `_PASSWORD`），
  不用 `TOKEN` 子串——那会把 `CLAUDE_CODE_MAX_OUTPUT_TOKENS` 一起盖掉。
  只在 claude-code 确实绑在当前方案时才做一致性对照，否则每行都会「不一致」，因为文件
  属于它当前跟随的那个方案
- **中转站非流式响应重组。** 很多 OpenAI 兼容中转站的 `stream:false` 返回体是坏的
  （缺字段，或把 SSE 帧当 JSON 发），流式路径却正常。连接时探一次普通非流式调用，把结论
  记为 profile 上的 `RelayChatCompat`；判定为 `Buffered` 时改写出站请求为流式，再把
  content、tool_calls、usage 和流内错误拼回标准 `chat.completion`。官方
  `api.openai.com/v1` 不走这条路径。方案编辑器暴露该字段只为覆盖误判，用户不需要设环境变量

### Fixed
- **网关对所有上游无条件跳过证书校验。** `gateway/src/client.rs` 硬编码
  `.danger_accept_invalid_certs(true)`。前端那个默认关闭、带警示的复选框和 core 侧的
  `accept_invalid_certs`（默认 `false`）都早已存在，穿透链只在 gateway 断掉：探测请求
  尊重开关，真实转发流量不尊重。现在证书策略走构造参数（reqwest 把 TLS 策略烧死在
  `Client` 构建那一刻，事后改不了），且参数必填——新增上游时强制表态，避免因遗漏而继承绕过
- **Codex 报「idle timeout waiting for SSE」并重连。** 桥接路径上
  `reasoning_content` 增量不产生任何客户端事件，GLM 可以思考几十秒而 Codex 一个字节都
  收不到；Anthropic 因为发 ping 所以免疫。`SSE_STREAM_IDLE_TIMEOUT` 从 25s 提到 300s，
  单个无法解析的块跳过而非杀掉整轮，客户端静默超过 10s 就发一个 SSE 注释——按「最后一次
  写客户端」计时，不是「最后一次收到上游字节」
- **思考内容盖掉了答案。** glm-5.3-flash 回一句「你好」用了 237 个输出 token，其中 187
  是推理。无条件把它折进助手文本，Codex 收到的是一轮「已完成」但正文全是思考、没有答案，
  渲染不出东西，看起来像卡死。现在只在这一轮自己没产出文本时才折叠，流式与非流式两条路径
  都改，纯思考预算至少还能作为一条消息到达
- **从方案目标列表里删掉客户端，绑定却留着。** 客户端 chip 渲染的是目标列表，所以删掉后
  没有任何界面显示这个绑定，而 `delete_profile` 仍然算它、仍然拒绝删除——复制出来的方案
  能进入一个没有任何屏幕可以撤销的状态：「该方案仍绑定着 claude-desktop」，却没有
  claude-desktop 的 chip 可点。`update_profile` 现在解绑新列表丢掉的客户端，chip 行改为
  渲染目标列表与绑定的并集

### Changed
- 两个 2400 行以上的文件按职责拆成目录，公共 API 在原路径重新导出，调用方零改动：
  `router/` 拆出 effort / models / respond / sse，`profile_switch/` 拆出
  claude_tiers / codex_catalog

### Notes
- env 预览只在 jsdom（Vitest）里验证过，core 侧读回有真实文件系统测试，但「切到参数 Tab
  → 面板显示正确内容」这条链路没在真实应用里跑过
- 证书校验现在尊重 profile 配置，但没做真实自签名站点的行为验证：reqwest 的 `Client`
  不暴露 TLS 配置，单测只能锁默认值

### Added
- **会话整合。** `HistoryStore::consolidate` 把同一会话在不同 id 方案下的重复行合并
  为一条，统一客户端名与时间戳格式，并在每次索引后自动运行；历史页新增「整合会话」
  按钮做一次显式迁移，并报告合并了多少条。在实测的 1010 行真实数据库上：识别 325 组
  重复、合并后剩 685 行、归一化客户端名 807 条、修正时间戳 623 条，重复运行幂等
- 会话归属：`SessionSummary` 增加 `providerId` / `profileId` / `mergedFrom`。绑定
  方案时为该客户端尚无归属的会话打标，已有归属的不改写——那是真实发生过的历史。
  历史页显示 provider 与合并份数
- `HistoryQuery.provider` 自类型引入起就存在但从未被读取，按 provider 过滤等于没过滤；
  现在行上有归属了，它按字面意思工作
- `HistoryStore::open_at`，可指定库文件且不触发索引，用于迁移校验

### Fixed
- **会话记录看起来「换 key 后丢失」，实际是两个读取缺陷叠加。** 会话文件从未被删除，
  切换方案也不碰任何 session 目录：
  - `list_summaries` 的 `LIMIT 500` 会静默截断。叠加下一条，1010 行里有 623 行落在
    列表之外
  - SQLite 按值而非按列定型，库中 623 行的时间戳存为 INTEGER、387 行存为 TEXT。以
    `String` 读取 INTEGER 会返回 `InvalidColumnType`，而读取路径用 `rows.flatten()`
    把这些行直接丢掉了。所有列现在都容忍两种存储类型
  - 混合格式下 `ORDER BY updated_at DESC` 按文本比较，数字时间戳永远排在 ISO 之后，
    因此被截断的正是最旧的那批
- 客户端筛选此前按字面值比较，而库中同一客户端存在四种拼写（`Codex`/`codex`/
  `Claude Code`/`claude-code`），筛 `codex-cli` 只能命中其中一部分
- 新建库的 `title`/`created_at`/`updated_at` 声明为 `NOT NULL`，与升级后的库不一致
  （真实库有 623 行 title 为 NULL），放宽以保持两者形状相同

### Notes
- 归属只对「此后」的会话有意义：历史行没有记录过 provider，整合不会凭当前配置追认
- 上述数字来自对真实库副本运行迁移的实测，非构造数据

### Added
- Real WebDAV sync: `CloudSync::upload`/`download` now perform the transfer,
  validate that a download is JSON before it replaces `state.json`, and back the
  existing file up to `state.json.bak` first
- Windows auto-launch via `HKCU\...\Run`, with `AutoLaunchStatus` gaining
  `supported` and `command` so the UI can disable the toggle where no
  implementation exists instead of offering a switch that does nothing
- `eslint.config.js` (flat config). `npm run lint` and the CI step that ran it
  had never linted anything: ESLint 9 dropped `.eslintrc` support and the repo had
  no replacement, so it exited early and every CI step after it was skipped

### Fixed
- **Failover was never running.** `FailoverManager` and
  `GatewayServer::with_failover_slot` both existed, but nothing constructed a
  manager, and `ad_failover_status` returned a hardcoded
  `{"running": false, "providers": []}` — indistinguishable from a healthy idle
  gateway. `refresh_gateway` now builds a manager from the bound profiles that
  enable it and shares the slot with the router, so the reported circuit state,
  provider health, and current upstream are the ones in effect. Stopping the
  gateway stops the probe loop
- `ad_get_logs` returned `Ok(vec![])` unconditionally, so the Settings log view
  was permanently empty while `~/.ai-deck/logs/` held data. It now calls
  `LogStore::get_logs`, which already did the reading and redaction, and returns
  structured entries with level and timestamp
- The auto-launch toggle updated optimistically over a backend that only logged,
  so it displayed "enabled" until a restart proved otherwise. It re-reads the real
  status after writing and surfaces failures instead of swallowing them
- Five stray `U+FEFF` characters embedded mid-file in `QuickSetupPage.tsx`
- Hardcoded `"2.0.0"` version placeholders in `SettingsPage` and `QuickSetupPage`,
  which would have shown a stale number before `ad_get_version` resolved

### Changed
- CI split: lint and tests gate every PR; `tauri build --debug` moved to a
  `bundle` job on `main` and `workflow_dispatch`. Packaging previously ran inside
  the same matrix job on all three platforms, making every PR wait on it.
  Adds `Swatinem/rust-cache` and npm caching, plus an explicit `tsc --noEmit` step

### Notes
- WebDAV upload/download have no test against a live server; the URL-joining
  rules are unit-tested, the transfer itself is not

## [2.1.1] - 2026-08-30

### Fixed
- Each client is written its own gateway token rather than the upstream provider
  key, which could not select a route since two profiles may share an upstream
- Claude Desktop tests derive both the normal and third-party data directories
  instead of hardcoding the Windows layout, and are gated to platforms where
  `data_dirs` resolves

## [2.1.0] - 2026-08-29

### Added
- **Per-client profile bindings.** Claude Code can sit on one profile while Codex
  CLI sits on another; one gateway on 18888 serves them all and routes by the
  bearer token each client presents
- `state.json` gains a `bindings` list, migrated automatically from the old single
  `activeProfileId`
- Per-client gateway tokens in the keyring, with rotation
- The status bar reports every bound profile instead of reading
  `getActiveProfile`, which answers null whenever bound clients disagree — so it
  previously said "未指定" precisely when the most was configured

### Changed
- **Breaking:** the sentinel token `ai-deck-local` no longer means "no token
  required", and the upstream provider's API key is no longer accepted as an
  inbound credential. Clients PolyDeck configured are re-issued tokens on first
  launch after the upgrade; a hand-configured client needs re-activating from the
  profiles page and will 401 until then

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
