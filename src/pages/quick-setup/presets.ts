import type { CodexToolCompat, ProtocolKind } from "@/domain/profile";
import type { DetectedClient } from "@/domain/client";
import {
  AGNES_BASE_URL_CN,
  AGNES_BASE_URL_GLOBAL,
  AGNES_DEFAULT_MODEL,
  AGNES_FREE_TIER_RPM,
} from "@/domain/agnes";

// Provider presets, protocol descriptors, and the two client-selection rules,
// split out of QuickSetupPage. All pure data and pure functions: no component
// state is involved, which is what makes the move behaviour-preserving.

export interface PresetProvider {
  name: string;
  baseUrl: string;
  defaultModel: string;
  keyPrefix: string;
  protocol: ProtocolKind;
  /**
   * Codex tool compatibility, when the provider has been probed against a live
   * endpoint. Left unset for presets whose upstream has not been verified, in
   * which case the page keeps whatever the probe last concluded.
   */
  codexCompat?: CodexToolCompat;
  /** Requests-per-minute ceiling, when it is known rather than guessed. */
  rpm?: number;
}

export const PRESETS: PresetProvider[] = [
  {
    name: "自定义",
    baseUrl: "https://api.example.com/v1",
    defaultModel: "gpt-4o",
    keyPrefix: "sk-",
    protocol: "openai",
  },
  {
    name: "Agnes AI (国内站)",
    baseUrl: AGNES_BASE_URL_CN,
    defaultModel: AGNES_DEFAULT_MODEL,
    keyPrefix: "sk-",
    protocol: "openai",
    codexCompat: "chat_function",
    rpm: AGNES_FREE_TIER_RPM,
  },
  {
    name: "Agnes AI (国际站)",
    baseUrl: AGNES_BASE_URL_GLOBAL,
    defaultModel: AGNES_DEFAULT_MODEL,
    keyPrefix: "sk-",
    protocol: "openai",
    codexCompat: "chat_function",
    rpm: AGNES_FREE_TIER_RPM,
  },
  {
    name: "OpenAI 官方 (Responses 原生)",
    baseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-4o",
    keyPrefix: "sk-",
    protocol: "responses",
  },
  {
    name: "OpenAI 兼容 (Chat Completions)",
    baseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-4o",
    keyPrefix: "sk-",
    protocol: "openai",
  },
  {
    name: "Anthropic 官方",
    baseUrl: "https://api.anthropic.com/v1",
    defaultModel: "claude-3-7-sonnet-20250219",
    keyPrefix: "sk-ant-",
    protocol: "anthropic",
  },
  {
    name: "DeepSeek 官方",
    baseUrl: "https://api.deepseek.com/v1",
    defaultModel: "deepseek-chat",
    keyPrefix: "sk-",
    protocol: "openai",
  },
  {
    name: "OpenRouter",
    baseUrl: "https://openrouter.ai/api/v1",
    defaultModel: "anthropic/claude-3.7-sonnet",
    keyPrefix: "sk-or-",
    protocol: "openai",
  },
  {
    name: "Ollama (本地)",
    baseUrl: "http://127.0.0.1:11434/v1",
    defaultModel: "llama3.3",
    keyPrefix: "",
    protocol: "openai",
  },
];


export const PROTOCOLS: { id: ProtocolKind; name: string; desc: string; defaultModel: string; defaultUrl: string }[] = [
  {
    id: "responses",
    name: "OpenAI Responses 原生",
    desc: "原生 /v1/responses 协议 (OpenAI 官方与 Codex 原生直连专线)",
    defaultModel: "gpt-4o",
    defaultUrl: "https://api.openai.com/v1",
  },
  {
    id: "openai",
    name: "OpenAI Chat 兼容",
    desc: "标准 /v1/chat/completions 与兼容 API (DeepSeek, Hermes, Ollama 等)",
    defaultModel: "gpt-4o",
    defaultUrl: "https://api.openai.com/v1",
  },
  {
    id: "anthropic",
    name: "Anthropic Claude 协议",
    desc: "原生 /v1/messages API (Claude 官方与 Anthropic 直连专线)",
    defaultModel: "claude-3-7-sonnet-20250219",
    defaultUrl: "https://api.anthropic.com/v1",
  },
  {
    id: "gemini",
    name: "Google Gemini 协议",
    desc: "Google AI Studio 原生 v1beta generateContent API",
    defaultModel: "gemini-2.5-pro",
    defaultUrl: "https://generativelanguage.googleapis.com/v1beta",
  },
  {
    id: "azure",
    name: "Azure OpenAI 协议",
    desc: "微软 Azure OpenAI 专用部署终点",
    defaultModel: "gpt-4o",
    defaultUrl: "https://your-resource.openai.azure.com",
  },
];

/// The clients `profile_switch::write_client_config` has a writer for, so activating
/// a profile configures them with no further steps. The others can be bound too and
/// route through the gateway the same way, but their address and token have to be
/// pasted in by hand — see the 需要手动填写 panel on the profiles page.
export const CORE_CLIENT_IDS = ["codex-cli", "claude-code", "claude-desktop", "hermes"];

/**
 * Whether Codex needs the gateway to reach this upstream at all.
 *
 * Codex sends `type: "custom"` tools (`apply_patch` among them). Two probe
 * verdicts mean the upstream will reject those:
 *
 * - `chat_function` — no Responses endpoint, so everything needs bridging.
 * - `responses_function` — serves Responses but refused a custom tool during
 *   probing. This is the verdict Agnes returns, and it used to read as merely
 *   "recommended", which let a user switch the gateway off and get
 *   `tools[7].type: unknown variant \`custom\`` on the first Codex turn.
 *
 * `responses_custom` is the only verdict that genuinely makes the gateway
 * optional for Codex; `none`/`unknown` are unproven, so they are not promoted.
 */
export function codexNeedsGateway(compat: CodexToolCompat): boolean {
  return compat === "chat_function" || compat === "responses_function";
}

/**
 * Clients worth pre-selecting for a given upstream protocol.
 *
 * `viaGateway` matters because the gateway translates protocols: with it in the
 * path, a Claude client can be served from an OpenAI-protocol upstream, which is
 * the whole point of routing Claude Code at a relay. Without it, only clients
 * that speak the upstream's own protocol can be configured — a Claude client
 * pointed straight at an OpenAI endpoint would just fail.
 *
 * Before this took `viaGateway`, an OpenAI upstream never pre-selected
 * `claude-code`, so tier slots mapping Claude names to the provider's model were
 * written and then never reached any client.
 */
export function getSmartClients(
  protocol: ProtocolKind,
  detected: DetectedClient[],
  viaGateway: boolean = false
): string[] {
  const installedIds = new Set(detected.filter((d) => d.installed).map((d) => d.id));

  const openaiCompatible = ["codex-cli", "hermes", "vscode", "cursor", "windsurf", "cherry-studio", "chatbox", "opencode"];
  const anthropicCompatible = ["claude-code", "claude-desktop", "cherry-studio", "chatbox"];

  if (protocol === "anthropic") {
    const matched = anthropicCompatible.filter((id) => installedIds.has(id));
    return matched.length > 0 ? matched : ["claude-code"];
  }

  // OpenAI-family upstream. Through the gateway the Claude clients are reachable
  // too, so offer both families.
  const candidates = viaGateway
    ? [...openaiCompatible, ...anthropicCompatible]
    : openaiCompatible;
  const matched = candidates.filter((id) => installedIds.has(id));
  if (matched.length > 0) {
    // Preserve the candidate order rather than detection order.
    return candidates.filter((id) => matched.includes(id));
  }
  return viaGateway ? ["codex-cli", "claude-code"] : ["codex-cli"];
}
