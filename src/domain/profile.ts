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
