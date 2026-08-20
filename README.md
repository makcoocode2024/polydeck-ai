# PolyDeck

<div align=center>

**多协议多客户端本地 AI 网关与开发者工作台** 
*Polymorphic Multi-Client AI Protocol Gateway & Developer Cockpit*

[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-v2-blue.svg)](https://tauri.app/)
[![React](https://img.shields.io/badge/React-19-61dafb.svg)](https://react.dev/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.8-3178c6.svg)](https://www.typescriptlang.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

[简体中文](#zh-cn) | [English](#en-us)

</div>

---

<a id=zh-cn></a>
## 简体中文

**PolyDeck** 是一个基于 **Rust + Tauri 2 + React 19** 构建的高性能本地多客户端 AI 协议网关与开发者工作台。

它旨在解决主流 AI 客户端（如 **OpenAI Codex CLI**、**Claude Code**、**Cursor**、**Windsurf**、**Aider**、**Hermes**）与多样化大模型提供商（OpenAI Responses、Anthropic Claude、Google Gemini、DeepSeek、本地 Ollama、中转聚合 API）之间的协议格式割裂、推理参数不兼容与配置分散痛点。

> **设计准则：真实探测，动态适配，绝不臆测。** 协议类型、推理能力与模型档位完全通过实测通道确定，实现零故障转译与平滑降级。

---

### 核心特性

#### 1. 🔀 多协议多端双向智能转译 (Polymorphic Protocol Engine)
- **Responses ↔ Chat Completions ↔ Claude Messages ↔ Gemini ↔ DeepSeek**：真正的双向动态协议转换，完整支持流式 SSE、Tool Calling / Custom Function 与错误格式对齐。
- **Auto / Native / Bridge 三重模式**：优先直通原生协议；遇到不支持端点时毫秒级无缝降级为桥接模式，客户端零感知。

#### 2. ⚡ 模型推理等级与硬件档位全自动动态适配
- **GPT-5.6 硬件档位与 Effort 匹配**：
 - Sol（旗舰档）：支持全档位推理努力（
one / low / medium / high / xhigh / max）。
 - Terra（均衡档）：动态自适应最高至 xhigh，防止越界 400 报错。
 - Luna（极速低成本档）：最高自适应至 high。
- **Gemini Thinking 预算自适应**：智能将 Minimal / Low / Medium / High 映射为符合 Google API 规范的 hinkingBudget 与配置，杜绝 Thinking level MINIMAL is not supported 报错。
- **DeepSeek & QwQ & Claude 3.7+ 深度思考适配**：自动探测并适配 easoning_content、 hinking 参数与预算。

#### 3. 🎯 客户端一键感知与原子配置注入
- 自动识别并管理 **OpenAI Codex CLI**、**Claude Code CLI**、**Claude Desktop**、**Cursor**、**Windsurf**、**Hermes** 等主流开发工具。
- 基于 oml_edit 与 JSON 的原子配置写入与 .bak 自动备份，方案切换失败即刻全量回滚。

#### 4. 🛡️ 高可用本地网关与熔断故障转移 (Circuit Breaker)
- **多服务商故障转移**：Closed / Open / HalfOpen 三态熔断器，主节点异常自动切换备用节点，恢复后自动切回。
- **模型名称改写规则**：支持按客户端灵活改写模型标识符（如 claude-3-7-sonnet ↔ glm-5.2 ↔ gpt-5.6-sol）。
- **SSE 流式边转边推**：无需缓冲整个响应体，极速响应首字。

#### 5. 🔐 操作系统级安全凭据托管
- **Keyring 原生安全存储**：API Key 与 WebDAV 凭据统一存入 Windows Credential Manager / macOS Keychain / Linux Secret Service，配置文件仅记录凭据引用，杜绝落盘泄露。
- **XChaCha20-Poly1305 加密备份**：会话历史与配置支持高强度加密导出与云端 WebDAV 同步。
- **严格安全限制**：本地网关仅监听 127.0.0.1 环回接口，Tauri 启用了严苛的 CSP。

#### 6. 🧩 丰富的扩展生态与开发者工具
- **MCP 服务器中心**：内置与自定义 MCP 服务按方案维度隔离同步。
- **会话历史检索**：基于 SQLite 的全文检索，按日聚合索引 Claude 与 Codex 会话记录。
- **CDP 渲染进程注入**：针对 Electron / WebView 客户端提供会话拦截、UI 优化与 Stepwise 建议。
- **深链支持**：polydeck:// 协议快速唤醒与方案切换。

---

### 项目架构

`
polydeck/
├── crates/
│ ├── core/ # 业务核心：协议探测、推理发现、方案系统、凭据、MCP、历史
│ ├── gateway/ # 本地 HTTP 代理网关：模型改写、协议转译、熔断故障转移
│ └── inject/ # CDP 注入层与 Stepwise 推理辅助
├── src-tauri/ # Tauri 2 宿主容器与 IPC 命令分发
├── src/ # React 19 + TypeScript + Jotai + Tailwind 前端 UI
├── scripts/ # 构建与环境维护脚本
└── docs/ # 用户手册与开发者文档
`

---

### 快速上手

#### 1. 环境准备
- **Node.js**: >= 18.0.0
- **Rust**: stable toolchain (2021 edition)
- **Tauri 2 Prerequisites**

#### 2. 本地运行与开发
`ash
# 安装前端依赖
npm install

# 运行前端与 Rust 后端（开发模式）
npm run tauri dev

# 仅运行前端调试
npm run dev
`

#### 3. 运行测试
`ash
# 运行 Rust 全工作区测试（149+ 单元与集成测试）
cargo test --workspace

# 运行前端测试（Vitest）
npm run test
`

#### 4. 构建发布包
`ash
# 编译生产版本二进制与安装包
npm run tauri build
`
产物生成于 arget/release/bundle/。

---

<a id=en-us></a>
## English

**PolyDeck** is a high-performance, local multi-client AI protocol gateway and developer cockpit built with **Rust + Tauri 2 + React 19**.

It bridges the gap between various AI developer tools (OpenAI Codex CLI, Claude Code, Cursor, Windsurf, Aider, Hermes) and diverse upstream LLM providers (OpenAI Responses, Anthropic Claude, Google Gemini, DeepSeek, local Ollama, aggregator relays).

### Key Features
- **Polymorphic Protocol Engine**: Bi-directional conversion across OpenAI Responses, Chat Completions, Anthropic Messages, Google Gemini, and DeepSeek.
- **Dynamic Reasoning Effort & Hardware Adaptation**: Dynamic adaptation for GPT-5.6 Sol / Terra / Luna tiers, Google Gemini thinking budgets, and DeepSeek reasoning parameters.
- **Zero-Touch Client Injections**: Seamless atomic configuration injection for Codex, Claude, Cursor, and Windsurf with automatic rollback.
- **Circuit Breaker & Failover**: Closed/Open/HalfOpen circuit breaker with automatic provider failover and recovery.
- **OS-Native Keyring Storage**: API keys are securely stored in the OS credential vault (Windows Credential Manager / macOS Keychain / Linux Secret Service).
- **SQLite Chat History & MCP Sync**: Full-text indexed conversation history and profile-scoped MCP server synchronization.

---

## 许可证 / License

[MIT License](LICENSE)

