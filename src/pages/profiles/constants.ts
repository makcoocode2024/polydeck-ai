import type {
  CodexToolCompat,
  ModelInfo,
  ProtocolKind,
  ReasoningConfidence,
} from "@/domain/profile";
import {
  AGNES_BASE_URL_CN,
  AGNES_BASE_URL_GLOBAL,
  AGNES_DEFAULT_MODEL,
} from "@/domain/agnes";

// Client lists, alias tiers, and provider presets, split out of ProfilesPage so
// the component file holds behaviour rather than tables. These are static data
// with no component state, which is what makes them safe to move as-is.

// Ids must match `client_detector::detect_all`, since `allClientOptions` merges
// the two by id — a stale id here shows up as a second, never-installed row for
// a client that is already in the list.
export const KNOWN_CLIENTS = [
  { id: "codex-cli", name: "Codex CLI" },
  { id: "claude-code", name: "Claude Code" },
  { id: "claude-desktop", name: "Claude Desktop" },
  { id: "hermes", name: "Hermes" },
  { id: "cursor", name: "Cursor" },
  { id: "windsurf", name: "Windsurf" },
  { id: "cherry-studio", name: "Cherry Studio" },
  { id: "chatbox", name: "Chatbox" },
  { id: "vscode", name: "VS Code (Cline / Continue)" },
  { id: "opencode", name: "OpenCode" },
];

// The clients `profile_switch::write_client_config` has a writer for. The rest bind
// and route through the gateway just the same, but their address and token have to
// be pasted in by hand, so the inspector has to say so rather than let a binding
// that changed no file look like it worked.
export const AUTO_CONFIG_CLIENTS = ["codex-cli", "claude-code", "claude-desktop", "hermes"];

/**
 * Claude Code's three model tiers.
 *
 * `defaultDisplayName` mirrors `DEFAULT_*_DISPLAY_NAME` in
 * `crates/core/src/profile_switch.rs`, which is what actually gets written when
 * the field is left blank — keep the two in step when bumping generations. Only
 * shown as placeholder text, so a drift misleads but never misconfigures.
 */
export const ALIAS_TIERS = [
  { alias: "opus", label: "Opus", field: "opusModel", displayField: "opusDisplayName", defaultDisplayName: "claude-opus-5" },
  { alias: "sonnet", label: "Sonnet", field: "sonnetModel", displayField: "sonnetDisplayName", defaultDisplayName: "claude-sonnet-5" },
  { alias: "haiku", label: "Haiku", field: "haikuModel", displayField: "haikuDisplayName", defaultDisplayName: "claude-haiku-4-5" },
] as const;

export const PROVIDER_PRESETS = [
  {
    name: "自定义",
    baseUrl: "https://api.example.com/v1",
    protocol: "openai" as ProtocolKind,
    defaultModel: "gpt-4o",
    codexCompat: "responses_custom" as CodexToolCompat,
    reasoningConfidence: "validated" as ReasoningConfidence,
  },
  {
    name: "Agnes AI (国内站)",
    baseUrl: AGNES_BASE_URL_CN,
    protocol: "openai" as ProtocolKind,
    defaultModel: AGNES_DEFAULT_MODEL,
    codexCompat: "chat_function" as CodexToolCompat,
    reasoningConfidence: "verified" as ReasoningConfidence,
  },
  {
    name: "Agnes AI (国际站)",
    baseUrl: AGNES_BASE_URL_GLOBAL,
    protocol: "openai" as ProtocolKind,
    defaultModel: AGNES_DEFAULT_MODEL,
    codexCompat: "chat_function" as CodexToolCompat,
    reasoningConfidence: "verified" as ReasoningConfidence,
  },
  {
    name: "OpenAI 官方 (Responses 原生)",
    baseUrl: "https://api.openai.com/v1",
    protocol: "responses" as ProtocolKind,
    defaultModel: "gpt-4o",
    codexCompat: "responses_custom" as CodexToolCompat,
    reasoningConfidence: "validated" as ReasoningConfidence,
  },
  {
    name: "OpenAI 兼容 (Chat Completions)",
    baseUrl: "https://api.openai.com/v1",
    protocol: "openai" as ProtocolKind,
    defaultModel: "gpt-4o",
    codexCompat: "chat_function" as CodexToolCompat,
    reasoningConfidence: "validated" as ReasoningConfidence,
  },
  {
    name: "Anthropic Claude",
    baseUrl: "https://api.anthropic.com/v1",
    protocol: "anthropic" as ProtocolKind,
    defaultModel: "claude-3-7-sonnet-20250219",
    codexCompat: "responses_custom" as CodexToolCompat,
    reasoningConfidence: "verified" as ReasoningConfidence,
  },
  {
    name: "DeepSeek 官方",
    baseUrl: "https://api.deepseek.com/v1",
    protocol: "openai" as ProtocolKind,
    defaultModel: "deepseek-chat",
    codexCompat: "chat_function" as CodexToolCompat,
    reasoningConfidence: "validated" as ReasoningConfidence,
  },
  {
    name: "OpenRouter",
    baseUrl: "https://openrouter.ai/api/v1",
    protocol: "openai" as ProtocolKind,
    defaultModel: "anthropic/claude-3.7-sonnet",
    codexCompat: "responses_custom" as CodexToolCompat,
    reasoningConfidence: "declared" as ReasoningConfidence,
  },
  {
    name: "SiliconFlow 硅基流动",
    baseUrl: "https://api.siliconflow.cn/v1",
    protocol: "openai" as ProtocolKind,
    defaultModel: "deepseek-ai/DeepSeek-V3",
    codexCompat: "chat_function" as CodexToolCompat,
    reasoningConfidence: "validated" as ReasoningConfidence,
  },
  {
    name: "Ollama 本地大模型",
    baseUrl: "http://127.0.0.1:11434/v1",
    protocol: "openai" as ProtocolKind,
    defaultModel: "llama3.3",
    codexCompat: "chat_function" as CodexToolCompat,
    reasoningConfidence: "unknown" as ReasoningConfidence,
  },
];

export interface ProbeState {
  loading: boolean;
  success?: boolean;
  message?: string;
  models?: ModelInfo[];
  latency?: number;
}
