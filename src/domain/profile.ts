export type ProtocolKind = "openai" | "responses" | "anthropic" | "gemini" | "azure" | "unknown";
export type CodexToolCompat = "responses_custom" | "responses_function" | "chat_function" | "none" | "unknown";
export type ReasoningConfidence = "unknown" | "declared" | "validated" | "verified";
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

export interface Profile {
  id: string;
  name: string;
  isActive: boolean;
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
