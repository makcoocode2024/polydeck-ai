# HANDOFF — 2026-09-13

分支 `fix/codex-wire-api-direct-mode`，领先 `main` 13 个提交。数字会变，以 `git log --oneline main..HEAD | wc -l` 为准。

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
