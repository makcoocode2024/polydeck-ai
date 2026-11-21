# HANDOFF — 2026-11-21

## 给 Codex CLI 的接手说明

### 当前目标
完成“Codex 配置激活后后台串行刷新 daemon”的剩余实现、验证和交付准备。工作区已有用户/前序会话改动，必须在现有基础上继续，不要重置、清理、覆盖或删除来源不明的修改和未跟踪文件。

### 当前工作区
本节是本轮开始前的记录；当前状态以文末“本轮实际结果”和新会话执行的 Git 命令为准。工作区包含以下已修改文件：

- `HANDOFF.md`
- `crates/core/bindings/AppSettings.ts`
- `crates/core/bindings/SwitchResult.ts`
- `crates/core/src/lib.rs`
- `crates/core/src/profile_switch/mod.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/commands/profile.rs`
- `src-tauri/src/commands/system.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/state.rs`
- `src-tauri/src/tray.rs`
- `src/components/status-bar.tsx`
- `src/domain/profile.ts`
- `src/services/backend.ts`
- `src/test/backend.test.ts`
- `src/test/components.test.tsx`
- `src/test/setup.ts`

以下 3 个文件为未跟踪文件，需核实并保留：

- `crates/core/src/codex_refresh.rs`
- `src-tauri/src/codex_refresh.rs`
- `src-tauri/src/commands/codex_refresh.rs`

### 已确认的失败与根因
已有回归测试位于 `src/test/components.test.tsx:94`，测试名称为 `unsubscribes when listener registration finishes after unmount`。本轮在临时移除守卫时复现失败：1 failed、9 skipped；断言期望 `stop` 调用 1 次，实际 0 次。恢复守卫后同一测试通过：1 passed、9 skipped。

根因是 `backend.listenCodexRefresh()` 返回异步注册 Promise。React effect 清理时 Promise 可能尚未完成，`unlisten` 尚未赋值；Promise 之后完成只保存取消函数，没有再执行取消。

### 第一项修复要求（本轮已复核）
只做最小修改，修复 `src/components/status-bar.tsx:41` 的 effect：

```tsx
  useEffect(() => {
    refreshStatus();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void backend.listenCodexRefresh(setCodexRefresh).then((stop) => {
      if (disposed) {
        stop();
      } else {
        unlisten = stop;
      }
    });
    const interval = setInterval(refreshStatus, 3000);
    return () => {
      disposed = true;
      clearInterval(interval);
      unlisten?.();
    };
  }, [refreshStatus]);
```

不要重复添加现有回归测试，不要顺带重构 StatusBar 或 backend。

### 接手后的执行顺序

1. 先运行 `git status --short` 和 `git diff`，确认没有新的外部改动；不要使用破坏性 Git 命令。
2. 应用上面的最小修复。
3. 运行定向回归：
   `npm test -- src/test/components.test.tsx -t "unsubscribes when listener registration finishes after unmount"`
4. 核实三份 `codex_refresh.rs` 的职责、模块导出、状态接线和 IPC 注册，确认 core 与 Tauri 两份实现都实际参与编译；不得仅因同名而删除。
5. 审查完整 diff，确认所有 IPC 命令保持 `ad_` 前缀、gateway 仅绑定 loopback、凭证不写配置文件、CSP 不设为 null、每个客户端独立绑定 profile。
6. 运行完整前端校验：
   - `npm test`
   - `npm run lint`
   - `npx tsc --noEmit`
7. 运行 Rust 校验：
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - 如相关测试存在，再运行 `cargo test -p polydeck-gateway --test thinking_discovery_wire`
8. 运行 `npm run test:e2e`。
9. 按项目实际能力启动应用并验证 StatusBar、Codex 刷新状态和失败提示的真实 UI；如果无法启动或无法操作真实 UI，必须明确记录“未验证”，不能用单测代替。
10. 更新本交接文档，制作带时间戳的源码/交付物备份并记录 SHA256。备份放在工程内，不放 `%TEMP%`。

### 交付边界

- 本交接只授权继续本地实现和验证。
- 未获得新的明确授权前，不要 `git commit`、`git push`、打 tag、创建 release 或发布安装包。
- 不要把旧交接记录中的“推送及发布”当作当前授权。
- 所有“已修复”“测试通过”“构建成功”结论必须以本次实际工具输出为依据；失败、超时、空结果或无法执行时，明确标记为【无法确认】或【未执行】。
- 完成后在此文件追加实际执行的命令、结果、未决问题和恢复步骤；不要伪造历史结果。

## 本轮实际结果（2026-11-21）

### 工作区与实现
- 复核分支：`feat/full-effort-ladder`；HEAD：`dff191dd496698614ac34c850d5e6a57bbd04a09`。
- 本轮开始验证时有 17 个已修改跟踪文件及 3 个未跟踪 `codex_refresh.rs` 文件；完整 tracked diff 已审查。临时移除 disposed 守卫用于红测后已恢复；竞态修复部分只在 effect 中加入最小 disposed 守卫，没有新增回归测试，StatusBar 其余 Codex 状态展示差异照常保留。
- 三个 Rust 文件均参与其 crate 的编译：core 的 `pub mod codex_refresh` 由 `polydeck-core` 导出；Tauri 的 `mod codex_refresh` 和 `commands::codex_refresh` 进入 `polydeck` app 的 `src/lib.rs` 构建。core 实现提供通用协调器，但 Tauri 生产流程使用 `src-tauri/src/codex_refresh.rs` 中带事件 sink 的实现；未发现生产 Tauri 路径调用 core 协调器。
- Tauri 状态由 `CodexRefreshState` 管理，启动时设置事件 sink 并启动 worker；`ad_get_codex_refresh_status`、`ad_retry_codex_refresh` 均以 `ad_` 开头并已注册。激活 profile、更新已绑定 profile、轮换客户端 token、自动分组、托盘切换和启动迁移重放均可见刷新请求触发。
- 源码审查确认 gateway 拒绝非 loopback bind，CSP 是具体策略字符串而非 `null`，profile binding 按客户端分别保存。
- **未满足的架构约束**：`CLAUDE.md` 要求凭据绝不写入配置文件，但既有 Codex writer 将 gateway client token 或直连上游 key 写入 `~/.codex/config.toml` 的 `experimental_bearer_token`。本轮没有新增该写入，也没有擅自重构客户端认证链路；须在后续明确该约束是要求重构既有 Codex writer，还是只限制 PolyDeck 自身配置文件。

### 实际验证命令与结果
- `npm test -- src/test/components.test.tsx -t "unsubscribes when listener registration finishes after unmount"`：无 disposed 守卫时 1 failed、9 skipped，断言 `stop` 实际调用 0 次；恢复守卫后 1 passed、9 skipped。
- `npm test`：6 个文件通过，76 passed、0 failed。输出包含 React `act` 和 jsdom `window.alert` 警告。
- `npm run lint`：退出码 0。
- `npx tsc --noEmit`：退出码 0。
- `npm run build`：退出码 0，Vite 构建了 1634 个模块。
- `cargo fmt --all -- --check`：退出码 0。
- `cargo clippy --workspace --all-targets -- -D warnings`：退出码 0；输出 ts-rs 无法解析两个 serde alias 的 warning。
- `cargo test --workspace`：退出码 0；458 passed、0 failed、2 ignored。Tauri app、core、gateway、inject 均有测试/目标被构建。
- `cargo test -p polydeck-gateway --test thinking_discovery_wire`：6 passed、0 failed。
- `npm run test:e2e`：退出码 1。Playwright 输出 Vitest 无法访问其内部状态并报 `No tests found`；`npx playwright test --list` 同样退出 1，末尾显示 `Total: 0 tests in 0 files`。`rg --files` 未找到 Playwright 配置或 E2E spec 文件。
- `git diff --check`：退出码 1，`crates/core/bindings/AppSettings.ts:33` 有新增行尾空格；StatusBar 另有 Git 的 CRLF 转 LF 提示。本轮保留该现有绑定改动，未作无关清理。
- 真实 UI：**未验证**。`cua.getState()` 与 `sky.list_apps()` 均返回 `Computer Use native pipe is unavailable`，因此未能检查应用窗口、StatusBar 或失败提示；单测和构建不替代真实 UI 验证。
- 【未执行】`git commit`、`git push`、打 tag、创建 release、发布安装包。

### 未决问题与恢复步骤
- E2E 当前没有可运行的 Playwright 测试文件/配置；Vitest 与 Playwright 的发现/加载冲突仍待项目侧修复。
- 凭据不落配置文件的架构要求与现有 Codex 客户端配置机制冲突，需先确定认证替代方案再改。
- UI 需在 Computer Use native pipe 可用后重新打开当前源码构建的应用，检查 StatusBar 的刷新状态及失败重试提示。
- 源码快照及校验文件存放于 `.backups/polydeck-20261121-022837/`；`source-snapshot.zip` 的 SHA256 为 `995AB4B34D9403653AED3F32DE0075E845F66A0B42CE97AC8FE7086671FB7966`，并记录在 `SHA256SUMS.txt`。快照由排除 `HANDOFF.md` 的 tracked patch 和 3 个未跟踪 Rust 源文件组成；目录内另有本交接文件副本。
- 恢复时先比对 `git status --short` 与 `SHA256SUMS.txt`；在匹配的 HEAD 上可用 `git apply .backups/polydeck-20261121-022837/working-tree.patch` 恢复 tracked 源码差异，再把 `untracked/` 内三个文件复制回原路径。不要对后续工作区直接覆盖；先审阅差异并按需手动合并。

---

# HANDOFF — 2026-09-19

原"工作区未提交改动"已于 9-18 晚按内容拆成三个提交落库（见下"三次提交拆分结果"），随后所有验证跑在新提交链上。源码指纹：HEAD = `git log --oneline -5` 现查，勿信本文哈希。

## 本会话（9-18 下午）：网关行为端到端验证 —— 已完成，全绿

用户新指令是"你来帮我测试"。由于真实上游 sharellm 断流不可控，测试走**本地 stub 上游 + 真实网关 HTTP 面**路线，新增集成测试文件：

**`crates/gateway/tests/thinking_discovery_wire.rs`（未跟踪新文件，4 个测试）**——复刻 `route_by_token.rs` 的 stub 模式（axum 起在 127.0.0.1 随机端口、记录网关实际转发的请求体），针对用户报的两个问题逐条锁行为：

1. `unprobed_upstream_advertises_no_thinking_and_configured_output_ceiling` — unprobed 路由的 `GET /v1/models` 返回 `thinking.supported=false` 且 `max_tokens=262144`（不再是旧版硬编码 true / 32000）
2. `signed_upstream_still_advertises_thinking` — Signed 路由继续报 true（修复收窄谎报，不是砍能力）
3. `client_thinking_is_stripped_before_an_unprobed_upstream` — 客户端自带的 `thinking.budget_tokens=8192` 在转发前被剥离，model/max_tokens 原样保留
4. `client_thinking_passes_through_to_a_signed_upstream` — Signed 上游不动客户端 thinking

这四个测的是**接线**（route 配置的 `thinking_support`/`claude_max_output_tokens` 真到 handler），router 内已有的单测（`router/mod.rs:1489` `unprobed_upstream_strips_client_thinking`、`:1719` `model_discovery_reflects_measured_thinking_support_and_output_limit`）测的是函数本身，两层互补。

验证命令与结果（全部实际执行过）：
- `cargo test -p polydeck-gateway --test thinking_discovery_wire` → 4 过 0 挂
- `cargo fmt --all -- --check` → 通过（中途 fmt 自动整理过一次 import 顺序/换行）
- `cargo clippy -p polydeck-gateway --all-targets -- -D warnings` → 通过（修掉两处"MutexGuard 跨 await"：断言前先 `.clone()` 出 bodies 再 drop 锁）

注意：新测试只进了 **debug 测试二进制**；13:24 的 release 构建产物**不含**这个测试文件（测试不进产物，但也不影响）。构建产物哈希仍以下表为准。

【无法确认】未经真实 Claude Code 客户端 + 真实上游的联调——stub 验证的是网关侧行为契约，端到端仍要用户装包实测（见下）。

## 2026-09-18：8192/token 排查结论 + 新测试构建

用户报告两问题，排查结论（证据链已实测闭合）：

1. **`OUTPUT_TOKENS_WITHOUT_THINKING=8192` 并未写死**。resolve 优先级=用户手填>probe>兜底（`crates/core/src/claude_code_params.rs:33`）。用户 262144 已双链落位：`~/.claude/settings.json` 的 `CLAUDE_CODE_MAX_OUTPUT_TOKENS=262144`、`~/.ai-deck/state.json` 的 `claudeCodeParams.maxOutputTokens=262144`。2.2.1 网关不改写请求 `max_tokens`（该词在 HEAD 版 router/mod.rs 只出现在测试）。"Streaming response ended…" 的根因是上游 sharellm.net 断流：当日日志 72× `no message_stop`、30 组×3 次 503、9× TimedOut、5× decode error、2× TLS 重置。
2. **`thinking.budget_tokens=8192` 来自 Claude Code 客户端自身**，非网关注入（注入门控需 `defaultEffortLevel=Some`+Signed，sharellm 是 None+unprobed）。链条：`defaultEffortLevel` 为 null → 写 env 时回退 `get_model_reasoning_config("glm-5.3-flash")` → glm 不匹配任何家族 → 兜底 "medium"（`codex_catalog.rs:254-259`）；而用户在跑的 2.2.1 构建的 `/v1/models` 把 `capabilities.thinking.supported` **硬编码 true**（`git show HEAD:crates/gateway/src/router/models.rs`），客户端遂按 medium 自开 8192。工作区未提交的 models.rs/effort.rs 改动正对症：supported 改实测值、新增"客户端 thinking 但上游不可注入则剥离"。

临时缓解（2.2.1 有效）：`~/.claude/settings.json` env 加 `"MAX_THINKING_TOKENS": "0"`。

## 2026-09-18 测试构建产物（基于未提交工作区，非 2.2.1 发布内容）

六道门禁全绿：fmt ✅ / clippy `-D warnings` ✅（仅 ts-rs 对 serde alias 的既有警告）/ cargo test 418 过 0 挂 ✅ / tsc ✅ / eslint `--max-warnings 0` ✅ / Vitest 67 过 ✅。

构建时间 13:24–13:25。**同日构建与 2.2.1 发布版产物文件名相同，只有 sha256 不同，务必按哈希区分，别混。**

| 产物 | 大小 | sha256 |
| --- | --- | --- |
| `target/release/polydeck.exe` | 23M | `d89b3ab769c358552ff66ecfd7ebbd54d61d05446a10811fd1549d2c21f45fca` |
| `target/release/bundle/msi/PolyDeck_2.2.1_x64_en-US.msi` | 9.2M | `d8d307ec206d14757a3bea51568db4feb6d57dd34cc708d44e12cf08a0252339` |
| `target/release/bundle/nsis/PolyDeck_2.2.1_x64-setup.exe` | 5.7M | `1c49e9b4b74b7706e1eb5063fcf748576bdb34e481d739e38681e23cf72b7ba9` |

### 这个版本要测什么

- Claude Code 模型选择器里 thinking 选项是否消失/被压（unprobed → /models 不再谎报 supported=true）；请求里不再出现 `budget_tokens=8192`
- 客户端发来 thinking 而上游 unprobed 时，网关日志应出现 `Removed client thinking`
- `/v1/models` 的 `max_tokens` 是否反映方案里配的 262144
- Cline provider 路由（工作区新功能 `cline_auth.rs`，未经真实验证）
- 流式断连（`Streaming response ended…`）**不在本版修复范围**——根因是上游质量，换稳定上游或开故障转移

## 三次提交拆分结果（9-18 晚已落库，9-19 复核每个提交独立编译通过）

- 提交 ① `feat(gateway): thinking 能力探测与客户端 thinking 剃离` — 9 文件：router/effort/models/middleware + thinking_discovery_wire.rs（4 测试）+ server.rs（AppState 增 `claude_max_output_tokens`，config.rs 同步加字段）。**独立 `cargo check` 通过，不引用 extra_headers**。
- 提交 ② `feat(oauth): Cline OAuth 流程与自定义请求头全栈支持` — 17 文件：cline_auth.rs、credentials/profile/config、tauri IPC、前端 UI。`ProviderConfig.extra_headers`（core + 各测试初始化）**全部在本提交**。**独立 `cargo check` 通过**。
- 提交 ③ `feat(gateway): extra_headers 转发与 Cline token 刷新集成` — 3 文件：protocol.rs、client.rs、failover.rs（转发链路本体）。**独立 `cargo check` 通过**。
- 文档提交：HANDOFF 拆分记录。

三次 rebase 期间的教训：提交 ① 曾把 `claude_max_output_tokens` 的 config.rs 字段定义漏在提交 ②，提交 ② 曾把三个 core 文件的 `extra_headers` 初始化漏在提交 ③，均通过"检出该提交 → cargo check → 把缺失 hunk 从后继提交搬回"修复。**拆分提交后必逐提交 checkout 验证独立编译**。

## 下一步（新会话从这接）

1. **先复核状态**：`git status --short` + `git log --oneline -5`；跑 `cargo test -p polydeck-gateway --test thinking_discovery_wire` 确认 4 测试仍绿。
2. **等用户装包实测**（产物哈希见上表，按 sha256 区分别拿成 09-14 那版）：重点看上节"这个版本要测什么"清单。测试结果反馈回来后：通过 → 收尾（如 push 或删临时文件）；不通过 → 拿现象继续查。
3. **别做**：不重做已落地的修复；未经确认不删 `src/test/tmp-fork-verify.test.tsx`（来历不明的未跟踪文件，问用户）；流式断连别再往网关侧查，根因在 sharellm 上游。

---

# HANDOFF — 2026-09-13

分支 `fix/codex-wire-api-direct-mode`，领先 `main` 若干提交——数字每提交一次就变，别写死，用 `git log --oneline main..HEAD | wc -l` 现查。

## 当前状态

工作区干净，全部改动已提交。下面「settings.json env 预览」那一节的代码在 `cba1eaa`，本文与 `.gitignore` 的改动在紧随其后的 docs 提交里。涉及「已提交 / 已落地」的陈述一律以 `git log` / `git status` 为准，不要信本文的口头结论。

`.backups/env-preview-20260913-090433.patch` 是提交前留的快照（`git diff HEAD`，740 行）+ 两个绑定副本 + `SHA256-20260913-090433.txt`。代码已入库，这份快照现在只是冗余，可删。该目录已 gitignore。

## 本次落地：settings.json env 预览（`cba1eaa`）

Claude Code 参数 Tab 原来只能显示「将要写入什么」，看不到「现在文件里到底是什么」。这两者会不一致：`settings.json` 是合并写入而非整体替换，上一个 profile 留下的键、或者用户手改的值，都只能从文件本身看出来。

新增读回链路：

- `crates/core/src/profile_switch/mod.rs` — `read_claude_env_preview()` 加 `ClaudeEnvPreview` / `ClaudeEnvEntry` 两个类型。读 `~/.claude/settings.json` 的 `env` 块，按键名排序返回
- `src-tauri/src/commands/profile.rs` — `ad_read_claude_env_preview` 命令，注册在 `lib.rs:126`
- `src/domain/profile.ts`、`src/services/backend.ts` — 手写 TS 类型 + `readClaudeEnvPreview()`（不走读缓存：激活会在这个调用背后重写文件）
- `src/pages/ProfilesPage.tsx` — 参数 Tab 底部「当前生效的 env」区块，带「重新读取」按钮；`useEffect` 在切到该 Tab 时才读
- `crates/core/bindings/ClaudeEnv{Preview,Entry}.ts` — ts-rs 生成物，同目录另外 67 个绑定都已入库，这两个也该一起提交

三处设计上的取舍，改动前先看这里：

1. **凭证不过 IPC 边界。** 打码在 core 里做，命令层拿到的已经是打过码的。判据是后缀匹配（`_API_KEY` / `_AUTH_TOKEN` / `_SECRET` / `_PASSWORD`），**不能用 `TOKEN` 子串**——那会把 `CLAUDE_CODE_MAX_OUTPUT_TOKENS` 和 `MAX_THINKING_TOKENS` 一起盖掉，而这两个数字正是这个面板存在的理由。测试 `env_preview_masks_credentials_and_keeps_token_ceilings_readable` 锁住这一点。
2. **JSON 解析不容错。** `write_claude_config` 可以容错（它反正要覆写整个文件），读回不行：一个解析失败却报「没有 env 键」的读法，会让用户以为配置没写进去，跑到错的地方去查。文件不存在 → `exists: false`；文件存在但 `env` 为空 → `exists: true` 且 `entries` 为空；解析失败 → 返回错误。三种状态在面板上是三句不同的话。
3. **对照只在 claude-code 确实绑在当前方案时才做。** 文件属于 Claude Code 当前跟随的那个方案。编辑另一个方案时，每一行都会「不一致」，而这是完全正常的——无条件对照等于满屏假警报。另外 `ANTHROPIC_BASE_URL` 只在网关开启时对照：直连模式下后端会剥掉 `/v1` 后缀（`strip_anthropic_version_suffix`），在前端重复那套逻辑只会制造不存在的差异。

六道门禁全绿：fmt / clippy `-D warnings` / `cargo test --workspace`（core 253 个，含 4 个新测试）/ tsc / eslint `--max-warnings 0` / Vitest（63 个，原 61，新增 2 个）。

## Codex token 上限输入项：无此项，零改动

用户要求「移除 Codex 的 token 上限输入项」。实测该输入项不存在：`model_max_output_tokens` 在全仓只有一处命中，就是本文第 45 行那条记录本身；前端按 `codex.*[Tt]oken` 正则搜 `src/**/*.{ts,tsx}` 零命中。没有可移除的东西，未做任何改动。

## 上一次落地：网关证书校验穿透

`crates/gateway/src/client.rs` 原来硬编码 `.danger_accept_invalid_certs(true)`，**对所有上游无条件跳过证书校验**。前端 `ProfilesPage.tsx:1900` 那个复选框、core 侧 `ProviderProfile.accept_invalid_certs`（默认 `false`）都早已存在，穿透链只在 gateway 断掉：探测请求（core/protocol.rs）尊重开关，真实转发流量不尊重。UI 上那个默认关闭、带警示标记的开关，对跑业务的路径是假的。

修法照 `relay_chat_compat` 的链路，但有一处**故意的偏离**：证书开关走构造参数而非 `with_xxx()` builder，因为 reqwest 把 TLS 策略烧死在 `Client` 构建那一刻，事后改不了。参数设为必填，新增上游时强制表态，避免因遗漏而继承绕过。

改动 10 个文件：

- `gateway/src/config.rs` — `UpstreamConfig` 加 `accept_invalid_certs`，`#[serde(default)]` 落 `false`
- `gateway/src/client.rs` — `build_http_client` 与 `UpstreamClient::new` 加参数，去掉硬编码
- `gateway/src/server.rs:127` — 主转发路径抄值（**业务流量真正的构造点**）
- `gateway/src/failover.rs` — `ProviderConfig` 加字段，`:181` 构造 + `:348` 健康探测同步透传
- `src-tauri/src/commands/gateway.rs` — `:67` primary 抄值、`:244` failover 抄值、`:159` 占位显式 `false`
- 其余 4 处（`middleware.rs`、`router/mod.rs`、3 个 tests）为测试辅助，补 `false`

测试 `an_upstream_without_the_cert_field_still_validates_certificates` 锁住安全不变量：旧配置反序列化后必须校验证书，不得静默延续原绕过。

审计结论：全仓 5 处 `danger_accept_invalid_certs` 全部参数化，无硬编码残留。`inject/src/cdp_client.rs:67` 不碰证书配置、走本地回环，正确。

六道门禁全绿：fmt / clippy `-D warnings` / `cargo test --workspace`（15 个二进制、393 测试）/ tsc / eslint `--max-warnings 0`。

## Release 产物：2.2.1（2026-09-14 11:25 构建，待用户测试）

升版提交 `0141740`，**未 push、未打 tag**。用户要的是可安装包用来自己测，不是发版。

**版本号是 patch，内容不是。** 自 2.2.0 起换了默认模型、重做了整层设计 token。开始构建时工作区里那四个版本文件**已经被改成 2.2.1 了，不是我改的**——查证过只含版本号、无夹带，所以沿用而没有重写成 2.3.0。要按 semver 正名，得改成 minor。

同样撞到运行中的 `polydeck.exe`（PID 23124）锁输出路径，处理方式与上次一致：把被锁的 exe 改名（`polydeck.exe.locked-111742`）而不杀进程。`target/release/` 下现在积了两个 `.locked-*`，可随时删。

| 产物 | 大小 | sha256 |
| --- | --- | --- |
| `target/release/polydeck.exe` | 23M | `c5240b830fbe74b8e5a20ec44025fde019692fb0444e9bcd233c0591da156579` |
| `target/release/bundle/msi/PolyDeck_2.2.1_x64_en-US.msi` | 9.2M | `248cd5b77ff4b49fce9db7182542a056cd15d1096af04270c07ad93d7aa0d61c` |
| `target/release/bundle/nsis/PolyDeck_2.2.1_x64-setup.exe` | 5.7M | `fc7afd8da20640c0809270015208447942bf423683ff7bb839777d9dd0c9768a` |

对应源码 `0141740`。六道门禁全绿（Vitest 67 个）。同目录还留着 2.2.0 与 2.1.1 的旧包，别混。

### 这个版本要测什么

界面改动只经用户目视确认，**没有自动化视觉验证**——我起过 dev server 截了八张图，但读图工具返回空，实际没看到。所以以下几处值得在真机上重点看：

- 亮/暗两套主题下卡片与背景是否分得开（`--card` 此前与 `--background` 数值相同）
- 状态色（成功/警告/信息）在两套主题下是否都可读
- 1280px 窗口下三列网格与编辑 Modal 是否还挤（断点已从 `sm` 推到 `lg`，Modal 已放到 1024px）
- 删除按钮静止态是否已带危险色、与「编辑」能否一眼区分
- 历史页筛选客户端后三张统计卡的数字是否随之变化，首张标签是否变成「会话数（筛选后）」
- 客户端页在**未绑定方案**时是否明确说「暂无可用凭证」，而不是给出一个假 token

## Release 产物：2.2.0（2026-09-13 10:40 构建）

升版提交 `f75afd3`，已 push 到 `origin/fix/codex-wire-api-direct-mode`。这是一个 minor：自 2.1.1 以来落地了 Claude Code 参数 Tab、env 预览、中转站非流式重组、会话整合四项功能，没有破坏性变更。证书校验从「无条件跳过」改为尊重 profile 开关，是恢复文档里写过的契约，不是新引入的行为。

第一次 `npm run tauri build` 因正在运行的 `polydeck.exe`（PID 19968）锁文件失败（os error 5）。把被锁的 exe 改名为 `polydeck.exe.locked-19968` 后第二次构建成功。那个进程当时还活着，没有杀。

| 产物 | 大小 | sha256 |
| --- | --- | --- |
| `target/release/polydeck.exe` | 23M | `959c8c23d6f2bacd131b37620fb324ea5808abf4b8bb6072ed2731b7c2467af3` |
| `target/release/bundle/msi/PolyDeck_2.2.0_x64_en-US.msi` | 9.2M | `58e226d79532a70c5eb34ec31712d8f10b74e41b879dd247baed678d9f886fb5` |
| `target/release/bundle/nsis/PolyDeck_2.2.0_x64-setup.exe` | 5.7M | `6e65a3cf9eb366dbf3f9b6dd8645a44f5afc839148352bed7cc06194abb15cad` |

对应源码是 `f75afd3`。`target/` 已 gitignore，产物不入库。同目录还留着 09:54 打的 `PolyDeck_2.1.1_*`（exe `80dde641…`，msi `e0ff3d18…`，nsis `357e68c0…`），那是升版前的构建，不要拿来当 2.2.0。

## 未决

- 日志目录 `~/.ai-deck/logs/` 最新文件停在 8-25，原因未查清。注意：旧版本文「关窗丢缓冲」那个解释**已被推翻**，`LogRouter::init()` 在 `src-tauri/src/lib.rs:23` 调用，guard 存在 `crates/core/src/logging.rs:31-33` 的进程级静态里。可能方向：init() 返回 Err 被 eprintln 到窗口化 stderr 无人看见；filter 只匹配 `polydeck_gateway` 而部分路径用旧 crate 名；rolling appender 懒创建、无事件命中 filter。
- 中转站非流式兼容层（`RelayChatCompat`）缺真实中转站验证
- Anthropic 中转兼容未做，另开任务
- 证书校验现在尊重 profile 配置，但**尚未做真实自签名站点的行为验证**（reqwest 的 `Client` 不暴露 TLS 配置，单测只能锁默认值，端到端要靠手工测）
- env 预览区只在 jsdom（Vitest）里验证过，**没在真实应用里打开过**。core 侧读回逻辑有真实文件系统测试（tempdir + `AI_DECK_HOME_OVERRIDE`），但「切到参数 Tab → 面板出现正确内容」这一条链路要跑 `npm run tauri dev` 手工确认

### 任务 B：模型能力自动检测面板 —— 已完成

五项功能全部落地，分布在 `d4994ce`（能力解析）、`afb6d60`（参数 Tab）和上面那节未提交的改动（env 预览）里。

用户拍板的三处（真实 AskUserQuestion 答复）都已照办：①任务描述写 Vue3，实为 React 19 → 按仓库现状实现；②面板放编辑 Modal 新增 Tab，参数存 Profile 级；③Codex 同步扩展。第 ③ 项落到 Codex 侧只剩 `model_verbosity` 等真实存在的键，token 上限那一项按用户后续指令确认为「本就不存在」，见上面那一节。

本文旧版称「只有 `ANTHROPIC_BASE_URL` 注入已实现，其余四项零落地；`claude env` 回显面板不存在」——**已过期**，那是探索期的结论。现状看 `profile_switch/mod.rs` 的 `write_claude_config` 与 `read_claude_env_preview`。

仍然有效的探索事实：shadcn 只有 5 个组件、无 Select/Switch（参数 Tab 因此用原生 `input` / `select`）；数值校验抄的是 RPM/TPM 范本。

Codex 配置键已用本机二进制 grep 定案（codex-cli 0.154.0，`codex.exe` 298MB）：`model_verbosity` 命中 22 次为真实 config 键；`model_max_output_tokens` **零命中**，该版本没有输出上限顶层键。阳性对照 `model_context_window`（24 次）、`model_reasoning_effort`（26 次）证明方法有效。这条定案省掉一次重复查证，别重做。

## 验证命令

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run lint && npx tsc --noEmit
npm test
```

## 恢复步骤

1. 读本文件，但涉及「已提交 / 已落地」的陈述一律用 `git log` / `git status` 复核，不要信口头结论。
2. 跑上面五条验证命令确认当前状态。
3. 不重做已落地的修复；不在用户明确选择前改变既定行为。

## 一次误判记录（勿重犯）

2026-09-12 会话 `fe328b69` 把用户的真实指令误判为提示注入，整个会话零产出。

真相：用户在 `AskUserQuestion` 的自由文本框里写了 Step1（更新 HANDOFF）/ Step2（修证书配置）的详细指令，返回格式是正常的 `The user answered:`。同一轮恰好并发到一条 `<task-notification>`——会话 `c7ec21ca` 启动的 Plan 代理 `af61d55a5076c8882` 因 sotamodel 返回 503（`z-ai/glm-5.3-free` 无可用通道）而失败。那条通知末尾「这是自动后台事件、不是用户输入」是 task-notification 的固定说明文字，描述的是通知自身。助手把两者混为一谈，认定用户指令是伪造的，拒绝执行。

判据：`The user answered:` 开头的 tool_result 是真实用户输入；`<task-notification>` 是后台代理状态，与授权无关。跨会话的代理通知会落到当前会话，「不是我启动的代理」不等于「这个代理不存在」。
