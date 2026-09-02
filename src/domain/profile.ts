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
}

export interface ProfileTemplate {
  id: string;
  name: string;
  description: string;
  provider: ProviderConfig;
}
