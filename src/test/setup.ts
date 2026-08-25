import "@testing-library/jest-dom";
import { vi } from "vitest";
import type { Profile } from "@/domain/profile";

// Mock matchMedia
Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});

// Mock clipboard
Object.defineProperty(navigator, "clipboard", {
  value: {
    writeText: vi.fn().mockResolvedValue(undefined),
  },
});

// Mock Tauri IPC for testing
const mockResponses: Record<string, unknown> = {
  ad_get_version: "2.0.0",
  ad_ping: "pong",
  ad_detect_clients: [
    {
      id: "codex",
      name: "Codex CLI",
      installed: true,
      version: "0.1.0",
      configPath: "C:\\Users\\admin\\.codex\\config.toml",
      supportsAutoConfig: true,
    },
    {
      id: "cursor",
      name: "Cursor IDE",
      installed: false,
      version: null,
      configPath: null,
      supportsAutoConfig: false,
    },
    // Real ids, so client-selection logic can be exercised. `codex` above keeps
    // its legacy id because tests assert on `clients[0].id`.
    {
      id: "codex-cli",
      name: "Codex CLI (codex-cli)",
      installed: true,
      version: "0.1.0",
      configPath: "C:\\Users\\admin\\.codex\\config.toml",
      supportsAutoConfig: true,
    },
    {
      id: "claude-code",
      name: "Claude Code",
      installed: true,
      version: "2.1.0",
      configPath: "C:\\Users\\admin\\.claude\\settings.json",
      supportsAutoConfig: true,
    },
  ],
  ad_list_profiles: [
    {
      id: "prof_default",
      name: "Default Profile",
      isActive: true,
      providers: [
        {
          id: "prov_1",
          name: "OpenAI Primary",
          baseUrl: "https://api.openai.com/v1",
          protocol: "openai",
          defaultModel: "gpt-4o",
          models: ["gpt-4o", "gpt-4o-mini"],
          isPrimary: true,
          codexCompat: "responses_custom",
          reasoningConfidence: "validated",
          acceptInvalidCerts: false,
          maxPricePerRequest: null,
        },
      ],
      clients: ["codex"],
      mcpServers: [],
      skills: [],
      prompts: [],
      gatewayEnabled: true,
      failoverEnabled: true,
      createdAt: "2026-08-18T00:00:00Z",
      updatedAt: "2026-08-18T00:00:00Z",
    },
  ],
  ad_get_active_profile: {
    id: "prof_default",
    name: "Default Profile",
    isActive: true,
  },
  ad_duplicate_profile: (_args?: Record<string, unknown>) => {
    const dup = {
      id: `prof_dup_${Date.now()}`,
      name: "Default Profile (副本)",
      isActive: false,
      providers: [
        {
          id: "prov_dup_1",
          name: "OpenAI Primary",
          baseUrl: "https://api.openai.com/v1",
          protocol: "openai",
          defaultModel: "gpt-4o",
          models: ["gpt-4o", "gpt-4o-mini"],
          isPrimary: true,
          codexCompat: "responses_custom",
          reasoningConfidence: "validated",
          acceptInvalidCerts: false,
          maxPricePerRequest: null,
        },
      ],
      clients: ["codex"],
      mcpServers: [],
      skills: [],
      prompts: [],
      gatewayEnabled: true,
      failoverEnabled: true,
      createdAt: "2026-08-18T00:00:00Z",
      updatedAt: "2026-08-18T00:00:00Z",
    };
    if (Array.isArray(mockResponses.ad_list_profiles)) {
      mockResponses.ad_list_profiles.push(dup);
    }
    return dup;
  },
  ad_create_profile: {
    id: "prof_new",
    name: "New Profile",
    isActive: false,
    providers: [],
    clients: [],
    mcpServers: [],
    skills: [],
    prompts: [],
    gatewayEnabled: true,
    failoverEnabled: false,
    createdAt: "2026-08-18T00:00:00Z",
    updatedAt: "2026-08-18T00:00:00Z",
  },
  ad_update_profile: (args?: Record<string, unknown>) => {
    const update = args?.update as Partial<Profile> | undefined;
    return {
      id: args?.id || "prof_default",
      name: update?.name || "Updated Profile",
      isActive: true,
      providers: update?.providers || [],
      clients: update?.clients || ["codex"],
      mcpServers: [],
      skills: [],
      prompts: [],
      gatewayEnabled: update?.gatewayEnabled ?? true,
      failoverEnabled: update?.failoverEnabled ?? true,
      createdAt: "2026-08-18T00:00:00Z",
      updatedAt: "2026-08-18T12:00:00Z",
    };
  },
  ad_delete_profile: null,
  ad_switch_profile: {
    success: true,
    profileId: "prof_default",
    profileName: "Default Profile",
    clientsWritten: ["codex-cli", "claude-code"],
    warnings: [],
    message: "Profile 激活成功",
  },
  ad_set_profile_api_key: null,
  ad_get_profile_api_key: "sk-mock-key-123456",
  ad_get_profile_templates: [
    {
      id: "tpl_openai",
      name: "OpenAI 官方",
      description: "标准 OpenAI API 访问模板",
      provider: {
        id: "p_tpl_openai",
        name: "OpenAI",
        baseUrl: "https://api.openai.com/v1",
        protocol: "openai",
        defaultModel: "gpt-4o",
        models: ["gpt-4o"],
        isPrimary: true,
        codexCompat: "responses_custom",
        reasoningConfidence: "validated",
        acceptInvalidCerts: false,
        maxPricePerRequest: null,
      },
    },
  ],
  ad_test_provider_chat: {
    success: true,
    reply: "你好！这是一条来自 AI 模型的实时对话测试回复。",
    latencyMs: 180,
    model: "gpt-4o",
    protocol: "openai",
  },
  ad_probe_rate_limits: {
    recommendedRpm: 60,
    recommendedTpm: 100000,
    detectedFromHeaders: true,
    message: "从上游响应头获取到限制：RPM=60, TPM=100,000",
  },
  ad_probe_provider: {
    protocol: "openai",
    confidence: "high",
    evidence: ["GET /v1/models 返回有效模型列表"],
    models: [
      { id: "gpt-4o", name: "GPT-4o" },
      { id: "gpt-4o-mini", name: "GPT-4o mini" },
      { id: "o1-preview", name: "o1-preview" },
    ],
    codexCompat: "responses_custom",
    baseUrl: "https://api.openai.com/v1",
    supportsStreaming: true,
  },
  ad_gateway_start: "127.0.0.1:18888",
  ad_gateway_stop: null,
  ad_gateway_status: { running: true, port: 18888 },
  ad_failover_status: {
    profile_id: "prof_default",
    running: true,
    primary_provider_id: "prov_1",
    current_provider_id: "prov_1",
    on_backup: false,
    all_providers_failed: false,
    providers: [],
  },
  ad_failover_history: [],
  ad_list_mcp_servers: [
    {
      id: "filesystem",
      name: "本地文件系统",
      description: "安全地只读/读写工作区文件访问",
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-filesystem"],
      envKeys: [],
      isBuiltin: true,
    },
  ],
  ad_list_skills: [
    {
      id: "skill_code_review",
      name: "代码质量审查",
      description: "自动对 Git diff 进行合规与最佳实践审查",
      source: "builtin",
      enabled: true,
    },
  ],
  ad_list_prompts: [
    {
      id: "prompt_security",
      name: "安全防护规范",
      content: "请严格遵循安全原则，禁止硬编码密码与危险操作。",
      variables: [],
      scope: "global",
    },
  ],
  ad_query_history: [
    {
      id: "sess_1",
      client: "Codex CLI",
      title: "优化 Rust Gateway 路由",
      messageCount: 14,
      totalTokens: 3420,
      createdAt: "2026-08-18T10:00:00Z",
      updatedAt: "2026-08-18T10:30:00Z",
    },
  ],
  export_history: '{"sessions": []}',
  ad_export_history: '{"sessions": []}',
  ad_create_encrypted_backup: "C:\\backups\\ai-deck-2026.history-backup",
  ad_restore_encrypted_backup: null,
  ad_inject_status: {
    stage: "NativeReady",
    channel: "NativeUserScript",
    native: {
      available: true,
      installed: true,
      enabled: true,
      healthy: true,
      restart_required: false,
      script_hash: "a1b2c3d4e5f6",
    },
    message: "Ready",
  },
  ad_inject_install_native: {
    stage: "NativeReady",
    channel: "NativeUserScript",
    native: {
      available: true,
      installed: true,
      enabled: true,
      healthy: true,
      restart_required: false,
      script_hash: "a1b2c3d4e5f6",
    },
    message: "Installed",
  },
  ad_inject_uninstall_native: {
    stage: "NativeReady",
    channel: "NativeUserScript",
    native: {
      available: false,
      installed: false,
      enabled: false,
      healthy: false,
      restart_required: false,
      script_hash: "",
    },
    message: "Uninstalled",
  },
  ad_inject_repair: {
    stage: "NativeReady",
    channel: "NativeUserScript",
    native: {
      available: true,
      installed: true,
      enabled: true,
      healthy: true,
      restart_required: false,
      script_hash: "a1b2c3d4e5f6",
    },
    message: "Repaired",
  },
  ad_tray_status: { status: "idle" },
  ad_autolaunch_status: { enabled: true, method: "Registry" },
  ad_set_autolaunch: null,
  ad_detect_proxy: {
    tools: [
      { name: "Clash Verge", detected: true, port: 7890, running: true },
    ],
    activeProxy: "127.0.0.1:7890",
    httpProxy: "http://127.0.0.1:7890",
    httpsProxy: "http://127.0.0.1:7890",
  },
  ad_run_diagnostics: {
    items: [
      {
        category: "Gateway",
        level: "ok",
        message: "本地网关端口 18888 正常监听",
        impact: "",
        suggestion: "",
      },
    ],
    errors: 0,
    warnings: 0,
    okCount: 1,
    timestamp: "2026-08-18T12:00:00Z",
  },
  ad_check_update: { available: false, version: null },
  ad_get_logs: ["[INFO] PolyDeck core initialized", "[INFO] Gateway listening on 127.0.0.1:18888"],
  ad_detect_importable: [],
  ad_import_from_provider_deck: null,
  ad_force_chinese_status: {
    enabled: false,
    targets: [
      {
        target: "Claude Code",
        path: "C:\\Users\\admin\\.claude\\CLAUDE.md",
        rulePresent: false,
        changed: false,
        shadowedBy: null,
        error: null,
      },
      {
        target: "Codex",
        path: "C:\\Users\\admin\\.codex\\AGENTS.md",
        rulePresent: false,
        changed: false,
        shadowedBy: "C:\\Users\\admin\\.codex\\AGENTS.override.md",
        error: null,
      },
    ],
  },
  ad_set_force_chinese: (args?: Record<string, unknown>) => {
    const enabled = Boolean(args?.enabled);
    return {
      enabled,
      targets: [
        {
          target: "Claude Code",
          path: "C:\\Users\\admin\\.claude\\CLAUDE.md",
          rulePresent: enabled,
          changed: true,
          shadowedBy: null,
          error: null,
        },
        {
          target: "Codex",
          path: "C:\\Users\\admin\\.codex\\AGENTS.md",
          rulePresent: enabled,
          changed: true,
          shadowedBy: "C:\\Users\\admin\\.codex\\AGENTS.override.md",
          error: null,
        },
      ],
    };
  },
  ad_tool_truthfulness_status: {
    enabled: false,
    targets: [
      {
        target: "Claude Code",
        path: "C:\\Users\\admin\\.claude\\CLAUDE.md",
        rulePresent: false,
        changed: false,
        shadowedBy: null,
        error: null,
      },
      {
        target: "Codex",
        path: "C:\\Users\\admin\\.codex\\AGENTS.md",
        rulePresent: false,
        changed: false,
        shadowedBy: null,
        error: null,
      },
    ],
  },
  ad_set_tool_truthfulness: (args?: Record<string, unknown>) => {
    const enabled = Boolean(args?.enabled);
    return {
      enabled,
      targets: [
        {
          target: "Claude Code",
          path: "C:\\Users\\admin\\.claude\\CLAUDE.md",
          rulePresent: enabled,
          changed: true,
          shadowedBy: null,
          error: null,
        },
        {
          target: "Codex",
          path: "C:\\Users\\admin\\.codex\\AGENTS.md",
          rulePresent: enabled,
          changed: true,
          shadowedBy: null,
          error: null,
        },
      ],
    };
  },
};

const mockInvoke = async (cmd: string, args?: Record<string, unknown>) => {
  if (cmd in mockResponses) {
    const val = mockResponses[cmd];
    if (typeof val === "function") {
      return val(args);
    }
    return val;
  }
  return null;
};

vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));

/// Override one command's mock for a single test. Returns a restore function;
/// call it in a `finally` or the override leaks into every later test.
export function setMockResponse(cmd: string, value: unknown): () => void {
  const had = cmd in mockResponses;
  const previous = mockResponses[cmd];
  mockResponses[cmd] = value;
  return () => {
    if (had) {
      mockResponses[cmd] = previous;
    } else {
      delete mockResponses[cmd];
    }
  };
}

