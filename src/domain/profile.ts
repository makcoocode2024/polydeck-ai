export type ProtocolKind = "openai" | "responses" | "anthropic" | "gemini" | "azure" | "unknown";
export type CodexToolCompat = "responses_custom" | "responses_function" | "chat_function" | "none" | "unknown";
/**
 * OpenAI-protocol reasoning signal, measured on `/v1/chat/completions`. It says
 * nothing about whether Anthropic thinking blocks carry a signature — see
 * `ThinkingSupport`.
 */
export type ReasoningConfidence = "unknown" | "declared" | "validated" | "verified";

/**
 * Whether an upstream returns Anthropic thinking blocks a client can use.
 * Only `signed` permits the gateway to inject `thinking`; an unsigned block
 * cannot be persisted or replayed, so the client fails the whole turn.
 */
export type ThinkingSupport = "unprobed" | "signed" | "unsigned" | "absent";

/**
 * How to get a non-streaming Chat Completions answer out of an upstream.
 * Written by the probe: `buffered` where the upstream's non-streaming body came
 * back malformed, `direct` where it was fine, `auto` when never probed.
 */
export type RelayChatCompat = "auto" | "buffered" | "direct";
export type Confidence = "unknown" | "low" | "medium" | "high" | "certain";

export interface ModelInfo {
  id: string;
  name: string;
  contextLength?: number | null;
  maxOutputTokens?: number | null;
}

export interface ProbeResult {
  protocol: ProtocolKind;
  confidence: Confidence;
  evidence: string[];
  models: ModelInfo[];
  codexCompat: CodexToolCompat;
  baseUrl: string;
  supportsStreaming: boolean;
  relayChatCompat?: RelayChatCompat;
}

export interface ChatTestResult {
  success: boolean;
  reply: string;
  latencyMs: number;
  model: string;
  protocol: ProtocolKind;
}


export interface RateLimitSettings {
  enabled: boolean;
  rpm: number;
  tpm: number;
  adaptive: boolean;
}

export interface RateLimitRecommendation {
  recommendedRpm: number;
  recommendedTpm: number;
  detectedFromHeaders: boolean;
  message: string;
}

export interface ProviderConfig {
  id: string;
  name: string;
  baseUrl: string;
  protocol: ProtocolKind;
  defaultModel: string;
  models: string[];
  isPrimary: boolean;
  codexCompat: CodexToolCompat;
  reasoningConfidence: ReasoningConfidence;
  thinkingSupport?: ThinkingSupport;
  /**
   * Non-streaming Chat Completions handling for this upstream. The probe sets
   * it; the profile editor exposes it only so a wrong verdict can be corrected.
   */
  relayChatCompat?: RelayChatCompat;
  acceptInvalidCerts: boolean;
  maxPricePerRequest: number | null;
  rateLimit?: RateLimitSettings;
  supports1mContext?: boolean | null;
  defaultEffortLevel?: string | null;
  opusModel?: string | null;
  sonnetModel?: string | null;
  haikuModel?: string | null;
  /**
   * Names Claude Code is shown for each tier. Claude Code only applies a model's
   * real context window and pricing to names it knows, so these default to
   * current built-in Anthropic IDs. Gateway-only: it is what maps them back to
   * the provider's real model.
   */
  opusDisplayName?: string | null;
  sonnetDisplayName?: string | null;
  haikuDisplayName?: string | null;
  /**
   * The default model's output ceiling as the upstream reported it, kept from the
   * probe so the parameter panel can recommend a measured value.
   */
  probedMaxOutputTokens?: number | null;
}

/**
 * Claude Code launch parameters, stored per profile.
 *
 * `null`/absent on a token field means "follow detection"; a number means the
 * user has overridden it and detection must leave it alone.
 */
export interface ClaudeCodeParams {
  maxOutputTokens?: number | null;
  maxThinkingTokens?: number | null;
  disableAutoupdater: boolean;
}

export const CLAUDE_CODE_TOKEN_MIN = 4096;
export const CLAUDE_CODE_TOKEN_MAX = 262144;
export const OUTPUT_TOKENS_WITH_THINKING = 131072;
export const OUTPUT_TOKENS_WITHOUT_THINKING = 8192;
export const THINKING_TOKENS_DEFAULT = 32768;

/**
 * Mirror of `claude_code_params::resolve` on the Rust side. Only `signed`
 * thinking may be injected — an unsigned block cannot be persisted by the
 * client, so treating it as supported poisons the session.
 */
export function resolveClaudeCodeParams(
  provider: Pick<ProviderConfig, "thinkingSupport" | "probedMaxOutputTokens"> | undefined,
  params: ClaudeCodeParams | undefined,
): {
  maxOutputTokens: number;
  maxThinkingTokens: number | null;
  thinkingSupported: boolean;
} {
  const thinkingSupported = provider?.thinkingSupport === "signed";
  const maxOutputTokens =
    params?.maxOutputTokens ??
    provider?.probedMaxOutputTokens ??
    (thinkingSupported ? OUTPUT_TOKENS_WITH_THINKING : OUTPUT_TOKENS_WITHOUT_THINKING);
  return {
    maxOutputTokens,
    maxThinkingTokens: thinkingSupported
      ? (params?.maxThinkingTokens ?? THINKING_TOKENS_DEFAULT)
      : null,
    thinkingSupported,
  };
}

export interface McpServerConfig {
  id: string;
  name: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  enabled: boolean;
}

/// One client pinned to one profile, with the profile's name resolved for display.
///
/// A client can only follow one profile at a time because every client has a single
/// config file, so this is a map from client to profile rather than a set.
export interface ClientBindingView {
  clientId: string;
  profileId: string;
  profileName: string | null;
  gatewayEnabled: boolean;
  boundAt: string;
}

/// Where a client should point, for the ones PolyDeck cannot write a config for.
export interface ClientConnectionInfo {
  clientId: string;
  profileId: string;
  profileName: string;
  baseUrl: string;
  token: string;
  isGateway: boolean;
}

/// One key from `~/.claude/settings.json` `env`, as read back from disk.
export interface ClaudeEnvEntry {
  key: string;
  /// Display text. For a masked entry this describes the value, never carries it.
  value: string;
  /// The real value is a credential, so `value` only reports its length.
  masked: boolean;
}

/// What Claude Code's settings file has in effect right now.
///
/// The file is merged rather than replaced, so this can differ from what the
/// current profile would write — a leftover key from a previous profile, or a hand
/// edit, shows up only here.
export interface ClaudeEnvPreview {
  path: string;
  /// `false` means there is no settings file at all, which is not the same as a
  /// file whose `env` block is empty.
  exists: boolean;
  entries: ClaudeEnvEntry[];
}

export interface SwitchResult {
  success: boolean;
  profileId: string;
  profileName: string;
  /// Clients whose config file was rewritten.
  clientsWritten: string[];
  /// Clients now recorded as following this profile. Wider than `clientsWritten`:
  /// a client with no writer binds and routes but has nothing on disk to update.
  clientsBound: string[];
  warnings: string[];
  message: string;
}

export interface Profile {
  id: string;
  name: string;
  // No `isActive`. Which clients follow a profile comes from `listClientBindings`,
  // since one flag cannot say "Codex follows me but Claude Code does not".
  providers: ProviderConfig[];
  clients: string[];
  mcpServers: McpServerConfig[];
  skills: string[];
  prompts: string[];
  gatewayEnabled: boolean;
  failoverEnabled: boolean;
  /** Claude Code launch parameters for this profile's clients. */
  claudeCodeParams?: ClaudeCodeParams;
  createdAt: string;
  updatedAt: string;
}

export interface ProfileCreate {
  name: string;
  providers?: ProviderConfig[];
  clients?: string[];
}

export interface ProfileUpdate {
  name?: string;
  providers?: ProviderConfig[];
  clients?: string[];
  gatewayEnabled?: boolean;
  failoverEnabled?: boolean;
  claudeCodeParams?: ClaudeCodeParams;
}

export interface ProfileTemplate {
  id: string;
  name: string;
  description: string;
  provider: ProviderConfig;
}
