import { invoke } from "@tauri-apps/api/core";
import type { DetectedClient } from "@/domain/client";
import type { Profile, ProfileTemplate, ProbeResult, ProfileUpdate, ChatTestResult, ProtocolKind, RateLimitRecommendation, ThinkingSupport } from "@/domain/profile";
import type { McpServer, ManagedSkill, PromptTemplate } from "@/domain/extensions";
import type { SessionSummary } from "@/domain/history";
import type { DiagnosticReport, UpdateInfo, AutoLaunchStatus } from "@/domain/ops";
import type { ProxyStatus } from "@/domain/proxy";
import type { FailoverStatus } from "@/domain/failover";
import type { InjectStatus } from "@/domain/injection";

type CacheEntry = { value: unknown; expiresAt: number };
const READ_CACHE_TTL_MS = 2500;
const readCache = new Map<string, CacheEntry>();
const readInFlight = new Map<string, Promise<unknown>>();

function cachedRead<T>(key: string, request: () => Promise<T>, ttl = READ_CACHE_TTL_MS): Promise<T> {
  const now = Date.now();
  const cached = readCache.get(key);
  if (cached && cached.expiresAt > now) {
    return Promise.resolve(cached.value as T);
  }

  const pending = readInFlight.get(key);
  if (pending) return pending as Promise<T>;

  const promise = request()
    .then((value) => {
      readCache.set(key, { value, expiresAt: Date.now() + ttl });
      return value;
    })
    .finally(() => {
      readInFlight.delete(key);
    });
  readInFlight.set(key, promise);
  return promise;
}

function invalidateReads(...keys: string[]) {
  keys.forEach((key) => {
    readCache.delete(key);
    readInFlight.delete(key);
  });
}

export const backend = {
  // Core
  getVersion: () => cachedRead("version", () => invoke<string>("ad_get_version"), 30_000),
  ping: () => invoke<string>("ad_ping"),
  detectClients: () => cachedRead("clients", () => invoke<DetectedClient[]>("ad_detect_clients"), 15_000),

  // Profiles & Providers
  listProfiles: () => invoke<Profile[]>("ad_list_profiles"),
  getActiveProfile: () => invoke<Profile | null>("ad_get_active_profile"),
  createProfile: (name: string) =>
    invoke<Profile>("ad_create_profile", { name }).then((value) => {
      invalidateReads("profiles");
      return value;
    }),
  duplicateProfile: (id: string) =>
    invoke<Profile>("ad_duplicate_profile", { id }).then((value) => {
      invalidateReads("profiles");
      return value;
    }),
  updateProfile: (id: string, update: ProfileUpdate) =>
    invoke<Profile>("ad_update_profile", { id, update }).then((value) => {
      invalidateReads("profiles", "active-profile");
      return value;
    }),
  deleteProfile: (id: string) =>
    invoke<void>("ad_delete_profile", { id }).then((value) => {
      invalidateReads("profiles", "active-profile");
      return value;
    }),
  switchProfile: (id: string) =>
    invoke<void>("ad_switch_profile", { id }).then((value) => {
      invalidateReads("profiles", "active-profile");
      return value;
    }),
  getProfileTemplates: () => cachedRead("profile-templates", () => invoke<ProfileTemplate[]>("ad_get_profile_templates"), 30_000),
  probeProvider: (baseUrl: string, apiKey: string, acceptInvalidCerts?: boolean) =>
    invoke<ProbeResult>("ad_probe_provider", { baseUrl, apiKey, acceptInvalidCerts }),
    probeRateLimits: (baseUrl: string, apiKey: string, model?: string, acceptInvalidCerts?: boolean) =>
    invoke<RateLimitRecommendation>("ad_probe_rate_limits", { baseUrl, apiKey, model, acceptInvalidCerts }),
  // Writes the result into the profile, so the profile list is now stale.
  probeThinkingSupport: (profileId: string, providerId: string) =>
    invoke<ThinkingSupport>("ad_probe_thinking_support", { profileId, providerId }).then((value) => {
      invalidateReads("profiles", "active-profile");
      return value;
    }),
  testProviderChat: (
    baseUrl: string,
    apiKey: string,
    model: string,
    protocol?: ProtocolKind,
    acceptInvalidCerts?: boolean,
    prompt?: string
  ) =>
    invoke<ChatTestResult>("ad_test_provider_chat", {
      baseUrl,
      apiKey,
      model,
      protocol,
      acceptInvalidCerts,
      prompt,
    }),
  setProfileApiKey: (profileId: string, apiKey: string) =>
    invoke<void>("ad_set_profile_api_key", { profileId, apiKey }),
  getProfileApiKey: (profileId: string) =>
    invoke<string | null>("ad_get_profile_api_key", { profileId }),

  // Gateway
  gatewayStart: () => invoke<string>("ad_gateway_start"),
  gatewayStop: () => invoke<void>("ad_gateway_stop"),
  gatewayStatus: () => invoke<{ running: boolean; port: number | null }>("ad_gateway_status"),

  // Failover
  failoverStatus: () => invoke<FailoverStatus>("ad_failover_status"),
  failoverHistory: (limit: number) => invoke<unknown[]>("ad_failover_history", { limit }),

  // Extensions
  listMcpServers: () => cachedRead("mcp-servers", () => invoke<McpServer[]>("ad_list_mcp_servers")),
  listSkills: () => cachedRead("skills", () => invoke<ManagedSkill[]>("ad_list_skills")),
  listPrompts: () => cachedRead("prompts", () => invoke<PromptTemplate[]>("ad_list_prompts")),

  // History
  queryHistory: () => invoke<SessionSummary[]>("ad_query_history"),
  exportHistory: (format: string) => invoke<string>("ad_export_history", { format }),
  createEncryptedBackup: (password: string) => invoke<string>("ad_create_encrypted_backup", { password }),
  restoreEncryptedBackup: (path: string, password: string) =>
    invoke<void>("ad_restore_encrypted_backup", { path, password }).then((value) => {
      invalidateReads("history", "profiles");
      return value;
    }),

  // Inject
  injectStatus: () => cachedRead("inject-status", () => invoke<InjectStatus>("ad_inject_status")),
  injectInstallNative: () => invoke<InjectStatus>("ad_inject_install_native"),
  injectUninstallNative: () => invoke<InjectStatus>("ad_inject_uninstall_native"),
  injectRepair: () => invoke<InjectStatus>("ad_inject_repair"),

  // System
  trayStatus: () => invoke<{ status: string }>("ad_tray_status"),
  handleDeepLink: (url: string) => invoke<unknown>("ad_handle_deep_link", { url }),
  autolaunchStatus: () => cachedRead("autolaunch", () => invoke<AutoLaunchStatus>("ad_autolaunch_status")),
  setAutolaunch: (enabled: boolean) =>
    invoke<void>("ad_set_autolaunch", { enabled }).then((value) => {
      invalidateReads("autolaunch");
      return value;
    }),

  // Proxy
  detectProxy: () => invoke<ProxyStatus>("ad_detect_proxy"),

  // Ops
  runDiagnostics: () => invoke<DiagnosticReport>("ad_run_diagnostics"),
  checkUpdate: () => invoke<UpdateInfo>("ad_check_update"),
  getLogs: (limit: number) => invoke<string[]>("ad_get_logs", { limit }),

  // Importer
  detectImportable: () => cachedRead("importable", () => invoke<string[]>("ad_detect_importable"), 10_000),
  importFromProviderDeck: (path: string) =>
    invoke<void>("ad_import_from_provider_deck", { path }).then((value) => {
      invalidateReads("profiles", "importable");
      return value;
    }),
};

export function invalidateBackendReadCache(...keys: string[]) {
  invalidateReads(...keys);
}
