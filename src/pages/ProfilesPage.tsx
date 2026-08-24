import { useEffect, useState, useCallback } from "react";
import { useAtom } from "jotai";
import {
  AGNES_BASE_URL_CN,
  AGNES_BASE_URL_GLOBAL,
  AGNES_DEFAULT_MODEL,
} from "@/domain/agnes";
import { profilesAtom, templatesAtom, clientsAtom } from "@/state/profile";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { backend } from "@/services/backend";
import type {
  Profile,
  ProviderConfig,
  ProtocolKind,
  CodexToolCompat,
  ReasoningConfidence,
  ThinkingSupport,
  ModelInfo,
  ChatTestResult,
  RateLimitSettings,
} from "@/domain/profile";
import {
  UserCheck,
  Plus,
  Trash2,
  CheckCircle2,
  Server,
  Zap,
  ShieldCheck,
  RotateCw,
  Sparkles,
  ChevronRight,
  Pencil,
  X,
  Sliders,
  Laptop,
  Radio,
  Key,
  ListFilter,
  Activity,
  AlertCircle,
  Save,
  ArrowRight,
  MessageSquare,
  Copy,
  Gauge,
  Timer,
  Eye,
  EyeOff,
} from "lucide-react";

const KNOWN_CLIENTS = [
  { id: "codex", name: "Codex CLI" },
  { id: "claude-code", name: "Claude Code" },
  { id: "claude-desktop", name: "Claude Desktop" },
  { id: "hermes", name: "Hermes" },
  { id: "cursor", name: "Cursor" },
  { id: "windsurf", name: "Windsurf" },
  { id: "cherry-studio", name: "Cherry Studio" },
  { id: "chatbox", name: "Chatbox" },
  { id: "vscode", name: "VS Code (Cline / Continue)" },
  { id: "aider", name: "Aider CLI" },
];

/**
 * Claude Code's three model tiers.
 *
 * `defaultDisplayName` mirrors `DEFAULT_*_DISPLAY_NAME` in
 * `crates/core/src/profile_switch.rs`, which is what actually gets written when
 * the field is left blank — keep the two in step when bumping generations. Only
 * shown as placeholder text, so a drift misleads but never misconfigures.
 */
const ALIAS_TIERS = [
  { alias: "opus", label: "Opus", field: "opusModel", displayField: "opusDisplayName", defaultDisplayName: "claude-opus-5" },
  { alias: "sonnet", label: "Sonnet", field: "sonnetModel", displayField: "sonnetDisplayName", defaultDisplayName: "claude-sonnet-5" },
  { alias: "haiku", label: "Haiku", field: "haikuModel", displayField: "haikuDisplayName", defaultDisplayName: "claude-haiku-4-5" },
] as const;

const PROVIDER_PRESETS = [
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

interface ProbeState {
  loading: boolean;
  success?: boolean;
  message?: string;
  models?: ModelInfo[];
  latency?: number;
}

export default function ProfilesPage() {
  const [profiles, setProfiles] = useAtom(profilesAtom);
  const [templates, setTemplates] = useAtom(templatesAtom);
  const [detectedClients, setDetectedClients] = useAtom(clientsAtom);
  const [newProfileName, setNewProfileName] = useState("");
  const [loading, setLoading] = useState(profiles.length === 0);
  const [refreshing, setRefreshing] = useState(false);
  const [creating, setCreating] = useState(false);
  const [selectedProfileId, setSelectedProfileId] = useState<string | null>(() => {
    const active = profiles.find((p) => p.isActive);
    return active ? active.id : profiles[0]?.id || null;
  });

  // Probe status on right-side details inspector
  const [testingPrimary, setTestingPrimary] = useState(false);
  const [primaryTestResult, setPrimaryTestResult] = useState<{
    success: boolean;
    message: string;
    latency?: number;
  } | null>(null);
  const [testingPrimaryChat, setTestingPrimaryChat] = useState(false);
  const [primaryChatResult, setPrimaryChatResult] = useState<ChatTestResult | {
    success: boolean;
    message: string;
    latencyMs?: number;
  } | null>(null);

  // Edit Modal State
  const [editingProfile, setEditingProfile] = useState<Profile | null>(null);
  const [editTab, setEditTab] = useState<"basics" | "providers" | "clients">("basics");
  const [editName, setEditName] = useState("");
  const [editGatewayEnabled, setEditGatewayEnabled] = useState(true);
  const [editFailoverEnabled, setEditFailoverEnabled] = useState(false);
  const [editProviders, setEditProviders] = useState<ProviderConfig[]>([]);
  const [editClients, setEditClients] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [duplicatingId, setDuplicatingId] = useState<string | null>(null);

  // Per-provider probe and api key states inside Edit Modal
  const [providerKeys, setProviderKeys] = useState<Record<number, string>>({});
  const [showProviderKeys, setShowProviderKeys] = useState<Record<number, boolean>>({});
  const [probeStates, setProbeStates] = useState<Record<number, ProbeState>>({});
  const [rateLimitProbeStates, setRateLimitProbeStates] = useState<Record<number, {
    loading: boolean;
    success?: boolean;
    message?: string;
  }>>({});
  const [nodeChatStates, setNodeChatStates] = useState<Record<number, {
    loading: boolean;
    result?: ChatTestResult | { success: boolean; message: string; latencyMs?: number };
  }>>({});
  const [thinkingProbeStates, setThinkingProbeStates] = useState<Record<number, {
    loading: boolean;
    message?: string;
  }>>({});

  const loadData = useCallback(async (isManual = false) => {
    if (isManual) {
      setRefreshing(true);
    } else if (profiles.length === 0) {
      setLoading(true);
    }

    try {
      // Fast parallel fetch without blocking profile list
      const pListPromise = backend.listProfiles();
      const tListPromise = templates.length === 0 ? backend.getProfileTemplates().catch(() => []) : Promise.resolve(templates);
      const cListPromise = detectedClients.length === 0 ? backend.detectClients().catch(() => []) : Promise.resolve(detectedClients);

      const [pList, tList, cList] = await Promise.all([pListPromise, tListPromise, cListPromise]);

      setProfiles(pList);
      if (tList.length > 0) setTemplates(tList);
      if (cList.length > 0) setDetectedClients(cList);

      setSelectedProfileId((prev) => {
        if (prev && pList.some((p) => p.id === prev)) return prev;
        const active = pList.find((p) => p.isActive);
        return active ? active.id : pList[0]?.id || null;
      });
    } catch (err) {
      console.error("Failed to load profiles:", err);
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [profiles.length, templates, detectedClients, setProfiles, setTemplates, setDetectedClients]);

  useEffect(() => {
    loadData(false);
  }, [loadData]);

  const handleCreateProfile = async (name?: string) => {
    const targetName = (name ?? newProfileName).trim();
    if (!targetName) return;
    setCreating(true);
    try {
      const created = await backend.createProfile(targetName);
      setNewProfileName("");
      if (created?.id) {
        setProfiles((prev) => {
          const exists = prev.some((p) => p.id === created.id);
          return exists ? prev.map((p) => (p.id === created.id ? created : p)) : [...prev, created];
        });
        setSelectedProfileId(created.id);
      }
      backend.listProfiles().then((list) => setProfiles(list)).catch(() => {});
    } catch (err) {
      alert(`创建失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setCreating(false);
    }
  };

  const handleSwitch = async (id: string) => {
    // Optimistic UI update
    setProfiles((prev) =>
      prev.map((p) => ({
        ...p,
        isActive: p.id === id,
      }))
    );
    setSelectedProfileId(id);
    setPrimaryTestResult(null);
    setPrimaryChatResult(null);

    try {
      await backend.switchProfile(id);
      const updatedList = await backend.listProfiles();
      setProfiles(updatedList);
    } catch (err) {
      alert(`切换失败: ${err instanceof Error ? err.message : String(err)}`);
      backend.listProfiles().then((list) => setProfiles(list)).catch(() => {});
    }
  };

  const handleDuplicate = async (id: string, _name?: string) => {
    setDuplicatingId(id);
    try {
      const dup = await backend.duplicateProfile(id);
      if (dup?.id) {
        setProfiles((prev) => {
          const exists = prev.some((p) => p.id === dup.id);
          return exists ? prev.map((p) => (p.id === dup.id ? dup : p)) : [...prev, dup];
        });
        setSelectedProfileId(dup.id);
        setPrimaryTestResult(null);
        setPrimaryChatResult(null);
      }
      const list = await backend.listProfiles().catch(() => []);
      if (list && Array.isArray(list) && list.length > 0) setProfiles(list);
    } catch (err) {
      alert(`复制方案失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setDuplicatingId(null);
    }
  };

  const handleDelete = async (id: string, name: string) => {
    if (!confirm(`确定要删除方案 "${name}" 吗？关联的路由规则将被移除。`)) return;
    // Optimistic UI update
    setProfiles((prev) => prev.filter((p) => p.id !== id));
    if (selectedProfileId === id) {
      setSelectedProfileId(null);
    }

    try {
      await backend.deleteProfile(id);
      const updatedList = await backend.listProfiles();
      setProfiles(updatedList);
    } catch (err) {
      alert(`删除失败: ${err instanceof Error ? err.message : String(err)}`);
      backend.listProfiles().then((list) => setProfiles(list)).catch(() => {});
    }
  };

  const openEditModal = async (profile: Profile) => {
    setEditingProfile(profile);
    setEditTab("basics");
    setEditName(profile.name);
    setEditGatewayEnabled(profile.gatewayEnabled !== false);
    setEditFailoverEnabled(Boolean(profile.failoverEnabled));
    setEditProviders(JSON.parse(JSON.stringify(profile.providers || [])));
    setEditClients(JSON.parse(JSON.stringify(profile.clients || [])));
    setProbeStates({});
    setRateLimitProbeStates({});
    setNodeChatStates({});
    setShowProviderKeys({});

    try {
      const savedKey = await backend.getProfileApiKey(profile.id);
      if (savedKey) {
        const initialKeys: Record<number, string> = {};
        const provs = profile.providers || [];
        provs.forEach((_, idx) => {
          initialKeys[idx] = savedKey;
        });
        setProviderKeys(initialKeys);
      } else {
        setProviderKeys({});
      }
    } catch {
      setProviderKeys({});
    }
  };

  const closeEditModal = () => {
    setEditingProfile(null);
    setProviderKeys({});
    setShowProviderKeys({});
    setProbeStates({});
    setRateLimitProbeStates({});
    setNodeChatStates({});
  };
  const handleAddProvider = () => {
    const newProv: ProviderConfig = {
      id: `prov_${Date.now()}`,
      name: `Provider ${editProviders.length + 1}`,
      baseUrl: "https://api.openai.com/v1",
      protocol: "openai",
      defaultModel: "gpt-4o",
      models: ["gpt-4o"],
      isPrimary: editProviders.length === 0,
      codexCompat: "responses_custom",
      reasoningConfidence: "validated",
      acceptInvalidCerts: false,
      maxPricePerRequest: null,
    };
    setEditProviders([...editProviders, newProv]);
  };

  const handleRemoveProvider = (index: number) => {
    const updated = editProviders.filter((_, i) => i !== index);
    if (updated.length > 0 && !updated.some((p) => p.isPrimary)) {
      updated[0].isPrimary = true;
    }
    setEditProviders(updated);
    setProviderKeys((prev) => {
      const next = { ...prev };
      delete next[index];
      return next;
    });
    setShowProviderKeys((prev) => {
      const next = { ...prev };
      delete next[index];
      return next;
    });
    setProbeStates((prev) => {
      const next = { ...prev };
      delete next[index];
      return next;
    });
    setRateLimitProbeStates((prev) => {
      const next = { ...prev };
      delete next[index];
      return next;
    });
    setNodeChatStates((prev) => {
      const next = { ...prev };
      delete next[index];
      return next;
    });
  };

  const handleSetPrimaryProvider = (index: number) => {
    setEditProviders(
      editProviders.map((p, i) => ({
        ...p,
        isPrimary: i === index,
      }))
    );
  };

  const handleUpdateProviderField = <K extends keyof ProviderConfig>(
    index: number,
    field: K,
    value: ProviderConfig[K]
  ) => {
    setEditProviders(
      editProviders.map((p, i) => (i === index ? { ...p, [field]: value } : p))
    );
  };

  const handleApplyPresetToProvider = (index: number, presetName: string) => {
    const preset = PROVIDER_PRESETS.find((p) => p.name === presetName);
    if (!preset) return;
    setEditProviders(
      editProviders.map((p, i) =>
        i === index
          ? {
              ...p,
              name: preset.name,
              baseUrl: preset.baseUrl,
              protocol: preset.protocol,
              defaultModel: preset.defaultModel,
              codexCompat: preset.codexCompat,
              reasoningConfidence: preset.reasoningConfidence,
            }
          : p
      )
    );
    setProbeStates((prev) => {
      const next = { ...prev };
      delete next[index];
      return next;
    });
  };

  const handleProbeProviderNode = async (index: number) => {
    const prov = editProviders[index];
    if (!prov || !prov.baseUrl.trim()) {
      setProbeStates((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          success: false,
          message: "请先填写 Base URL 接口地址！",
        },
      }));
      return;
    }

    setProbeStates((prev) => ({
      ...prev,
      [index]: {
        loading: true,
        message: "正在探测节点连通性并校验 API Key...",
      },
    }));

    const key = (providerKeys[index] || "").trim();
    const start = performance.now();
    try {
      const res = await backend.probeProvider(prov.baseUrl.trim(), key, prov.acceptInvalidCerts);
      const latency = Math.round(performance.now() - start);

      if (!res || res.protocol === "unknown") {
        setProbeStates((prev) => ({
          ...prev,
          [index]: {
            loading: false,
            success: false,
            latency,
            message: "探测失败：未能识别有效协议或 API Key 鉴权失败",
          },
        }));
        return;
      }

      handleUpdateProviderField(index, "protocol", res.protocol);
      if (res.codexCompat && res.codexCompat !== "unknown") {
        handleUpdateProviderField(index, "codexCompat", res.codexCompat);
      }
      if (res.models && res.models.length > 0) {
        handleUpdateProviderField(
          index,
          "models",
          res.models.map((m) => m.id)
        );
      }

      setProbeStates((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          success: true,
          latency,
          models: res.models || [],
          message: `探测与鉴权成功 (${latency}ms)：协议识别为 [${res.protocol.toUpperCase()}]，获取到 ${res.models?.length ?? 0} 个模型。`,
        },
      }));
    } catch (err) {
      const errDetail = err instanceof Error ? err.message : String(err);
      setProbeStates((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          success: false,
          message: `探测失败: ${errDetail}`,
        },
      }));
    }
  };

  const handleUpdateRateLimit = <K extends keyof RateLimitSettings>(
    index: number,
    field: K,
    value: RateLimitSettings[K]
  ) => {
    setEditProviders((prev) =>
      prev.map((p, i) => {
        if (i !== index) return p;
        const current: RateLimitSettings = p.rateLimit || {
          enabled: false,
          rpm: 60,
          tpm: 100_000,
          adaptive: true,
        };
        return {
          ...p,
          rateLimit: {
            ...current,
            [field]: value,
          },
        };
      })
    );
  };

  const THINKING_SUPPORT_LABELS: Record<ThinkingSupport, string> = {
    unprobed: "尚未探测 — 不会注入思考",
    signed: "支持带签名思考 — 可以注入",
    unsigned: "返回思考但缺签名 — 不能注入",
    absent: "不返回思考块 — 不会注入",
  };

  const handleProbeThinkingSupport = async (index: number) => {
    const prov = editProviders[index];
    if (!editingProfile || !prov) return;

    setThinkingProbeStates((prev) => ({
      ...prev,
      [index]: { loading: true, message: "正在向上游发送带 thinking 的请求，检查返回的思考块是否带 signature..." },
    }));

    try {
      const support = await backend.probeThinkingSupport(editingProfile.id, prov.id);
      // The command already persisted this; mirror it into the open editor so the
      // form does not overwrite it when saved.
      setEditProviders((prev) =>
        prev.map((p, i) => (i === index ? { ...p, thinkingSupport: support } : p))
      );
      setThinkingProbeStates((prev) => ({
        ...prev,
        [index]: { loading: false, message: THINKING_SUPPORT_LABELS[support] },
      }));
    } catch (e) {
      setThinkingProbeStates((prev) => ({
        ...prev,
        [index]: { loading: false, message: `探测失败: ${String(e)}` },
      }));
    }
  };

  const handleProbeRateLimits = async (index: number) => {
    const prov = editProviders[index];
    if (!prov || !prov.baseUrl.trim()) {
      setRateLimitProbeStates((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          success: false,
          message: "请先填写 Base URL 接口地址！",
        },
      }));
      return;
    }

    setRateLimitProbeStates((prev) => ({
      ...prev,
      [index]: {
        loading: true,
        message: "正在向上游服务商发送探测请求，分析速率限制响应头与安全阈值...",
      },
    }));

    const key = (providerKeys[index] || "").trim();
    try {
      const rec = await backend.probeRateLimits(
        prov.baseUrl.trim(),
        key,
        prov.defaultModel?.trim(),
        prov.acceptInvalidCerts
      );

      const currentRateLimit: RateLimitSettings = prov.rateLimit || {
        enabled: true,
        rpm: 60,
        tpm: 100_000,
        adaptive: true,
      };

      const updatedRateLimit: RateLimitSettings = {
        ...currentRateLimit,
        enabled: true,
        rpm: rec.recommendedRpm,
        tpm: rec.recommendedTpm,
      };

      handleUpdateProviderField(index, "rateLimit", updatedRateLimit);

      setRateLimitProbeStates((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          success: true,
          message: `${rec.message} (已回填推荐值: RPM=${rec.recommendedRpm}, TPM=${rec.recommendedTpm}，保存后生效)`,
        },
      }));
    } catch (err) {
      const errDetail = err instanceof Error ? err.message : String(err);
      setRateLimitProbeStates((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          success: false,
          message: `探测速率限制失败: ${errDetail}`,
        },
      }));
    }
  };

  const handleToggleClient = (clientId: string) => {
    if (editClients.includes(clientId)) {
      setEditClients(editClients.filter((c) => c !== clientId));
    } else {
      setEditClients([...editClients, clientId]);
    }
  };

  const handleSaveProfile = async (activate = false) => {
    if (!editingProfile) return;
    if (!editName.trim()) {
      alert("方案名称不能为空！");
      return;
    }
    setSaving(true);
    try {
      const primaryKey = providerKeys[0]?.trim();
      if (primaryKey) {
        await backend.setProfileApiKey(editingProfile.id, primaryKey).catch(() => {});
      }
      const updated = await backend.updateProfile(editingProfile.id, {
        name: editName.trim(),
        gatewayEnabled: editGatewayEnabled,
        failoverEnabled: editFailoverEnabled,
        providers: editProviders,
        clients: editClients,
      });
      if (activate) {
        await backend.switchProfile(editingProfile.id);
      }
      setEditingProfile(null);
      setProfiles((prev) =>
        prev.map((p) =>
          p.id === updated.id
            ? { ...updated, isActive: activate ? true : p.isActive }
            : activate
            ? { ...p, isActive: false }
            : p
        )
      );
      backend.listProfiles().then((list) => setProfiles(list)).catch(() => {});
    } catch (err) {
      alert(`保存修改失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setSaving(false);
    }
  };


  const handleTestPrimaryChat = async (profile: Profile) => {
    const primary = profile.providers?.find((p) => p.isPrimary) || profile.providers?.[0];
    if (!primary) {
      setPrimaryChatResult({
        success: false,
        message: "未找到可用 Provider 节点，请在编辑中添加节点。",
      });
      openEditModal(profile);
      return;
    }
    setTestingPrimaryChat(true);
    setPrimaryChatResult(null);
    const start = performance.now();
    try {
      const savedKey = await backend.getProfileApiKey(profile.id).catch(() => null);
      const res = await backend.testProviderChat(
        primary.baseUrl,
        savedKey || "",
        primary.defaultModel || "gpt-4o",
        primary.protocol,
        primary.acceptInvalidCerts
      );
      setPrimaryChatResult(res);
    } catch (err) {
      const latencyMs = Math.round(performance.now() - start);
      setPrimaryChatResult({
        success: false,
        latencyMs,
        message: `对话测试失败: ${err instanceof Error ? err.message : String(err)}`,
      });
    } finally {
      setTestingPrimaryChat(false);
    }
  };

  const handleChatTestProviderNode = async (index: number) => {
    const prov = editProviders[index];
    if (!prov || !prov.baseUrl.trim()) {
      setNodeChatStates((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          result: { success: false, message: "请先填写 Base URL 接口地址！" },
        },
      }));
      return;
    }

    setNodeChatStates((prev) => ({
      ...prev,
      [index]: { loading: true },
    }));

    const key = (providerKeys[index] || "").trim();
    const start = performance.now();
    try {
      const res = await backend.testProviderChat(
        prov.baseUrl.trim(),
        key,
        prov.defaultModel?.trim() || "gpt-4o",
        prov.protocol,
        prov.acceptInvalidCerts
      );
      setNodeChatStates((prev) => ({
        ...prev,
        [index]: { loading: false, result: res },
      }));
    } catch (err) {
      const latencyMs = Math.round(performance.now() - start);
      setNodeChatStates((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          result: {
            success: false,
            latencyMs,
            message: `对话测试失败: ${err instanceof Error ? err.message : String(err)}`,
          },
        },
      }));
    }
  };
  const handleTestPrimaryProvider = async (profile: Profile) => {
    const primary = profile.providers?.find((p) => p.isPrimary) || profile.providers?.[0];
    if (!primary) {
      setPrimaryTestResult({
        success: false,
        message: "未找到可用 Provider 节点，请在编辑中添加节点。",
      });
      openEditModal(profile);
      return;
    }
    setTestingPrimary(true);
    setPrimaryTestResult(null);
    setPrimaryChatResult(null);
    const start = performance.now();
    try {
      const savedKey = await backend.getProfileApiKey(profile.id).catch(() => null);
      const res = await backend.probeProvider(primary.baseUrl, savedKey || "", primary.acceptInvalidCerts);
      const latency = Math.round(performance.now() - start);
      if (!res || res.protocol === "unknown") {
        setPrimaryTestResult({
          success: false,
          latency,
          message: "主节点探测失败：无法识别有效的大模型协议或 API Key 无效",
        });
        return;
      }
      setPrimaryTestResult({
        success: true,
        latency,
        message: `主节点连通与鉴权正常 (${latency}ms) - 识别为 ${res.protocol.toUpperCase()} 协议，已就绪`,
      });
    } catch (err) {
      const latency = Math.round(performance.now() - start);
      setPrimaryTestResult({
        success: false,
        latency,
        message: `主节点测试失败: ${err instanceof Error ? err.message : String(err)}`,
      });
    } finally {
      setTestingPrimary(false);
    }
  };

  const selectedProfile = profiles.find((p) => p.id === selectedProfileId);

  // Combine known and detected client list for selection
  const allClientOptions = (() => {
    const map = new Map<string, string>();
    KNOWN_CLIENTS.forEach((c) => map.set(c.id, c.name));
    detectedClients.forEach((c) => map.set(c.id, c.name));
    return Array.from(map.entries()).map(([id, name]) => ({
      id,
      name,
      installed: detectedClients.some((d) => d.id === id && d.installed),
    }));
  })();

  return (
    <div className="space-y-8 max-w-6xl mx-auto pb-12">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <Badge variant="info" className="px-3 py-1">
              <UserCheck className="h-3 w-3 mr-1" />
              方案管理
            </Badge>
            <span className="text-xs text-muted-foreground">共 {profiles.length} 个配置方案</span>
          </div>
          <h1 className="text-3xl font-extrabold tracking-tight mt-1">配置方案 (Profiles)</h1>
          <p className="text-muted-foreground text-sm">
            每个方案可包含多个大模型 Provider、智能重写与流适配、MCP 服务及多客户端配置。
          </p>
        </div>

        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={() => loadData(true)} disabled={refreshing || loading} className="text-xs">
            <RotateCw className={`h-3.5 w-3.5 mr-1 ${refreshing || loading ? "animate-spin" : ""}`} />
            刷新
          </Button>
        </div>
      </div>

      {/* Quick Create input bar */}
      <Card className="border-border/60 shadow-sm">
        <CardContent className="p-4 flex flex-col sm:flex-row gap-3 items-center">
          <Input
            value={newProfileName}
            onChange={(e) => setNewProfileName(e.target.value)}
            placeholder="输入新方案名称，例如: DeepSeek 代码推理专用..."
            className="text-xs flex-1"
            onKeyDown={(e) => e.key === "Enter" && handleCreateProfile()}
          />
          <Button
            onClick={() => handleCreateProfile()}
            disabled={creating || !newProfileName.trim()}
            className="text-xs shrink-0 w-full sm:w-auto"
          >
            <Plus className="h-3.5 w-3.5 mr-1" />
            新建空白方案
          </Button>
        </CardContent>
      </Card>
      {/* Main layout: Profiles List & Profile Details */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-6">
        {/* Left column: List of profiles */}
        <div className="lg:col-span-5 space-y-3">
          <div className="text-xs font-semibold text-muted-foreground uppercase tracking-wider px-1">
            方案列表
          </div>

          {profiles.length === 0 ? (
            <Card>
              <CardContent className="p-8 text-center text-muted-foreground text-xs">
                暂无配置方案，请在上方输入名称创建，或从下方内置模板快速生成。
              </CardContent>
            </Card>
          ) : (
            profiles.map((p) => {
              const isSelected = p.id === selectedProfileId;
              return (
                <div
                  key={p.id}
                  onClick={() => {
                    setSelectedProfileId(p.id);
                    setPrimaryTestResult(null);
    setPrimaryChatResult(null);
                  }}
                  className={`p-4 rounded-xl border transition-all cursor-pointer select-none ${
                    isSelected
                      ? "border-primary bg-primary/5 shadow-sm"
                      : "border-border hover:border-border/80 bg-card/60"
                  }`}
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="space-y-1 min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="font-semibold text-sm truncate">{p.name}</span>
                        {p.isActive && (
                          <Badge variant="success" className="text-[10px] px-1.5 py-0 shrink-0">
                            <CheckCircle2 className="h-2.5 w-2.5 mr-1" />
                            当前激活
                          </Badge>
                        )}
                      </div>
                      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground">
                        <span>{p.providers?.length ?? 0} 个 Provider</span>
                        <span>·</span>
                        <span>{p.clients?.length ?? 0} 个客户端</span>
                        <span>·</span>
                        <span>{p.gatewayEnabled !== false ? "网关开启" : "网关关闭"}</span>
                      </div>
                    </div>

                    <div className="flex items-center gap-1.5 shrink-0" onClick={(e) => e.stopPropagation()}>
                      {!p.isActive && (
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-7 text-xs px-2"
                          onClick={() => handleSwitch(p.id)}
                        >
                          激活
                        </Button>
                      )}
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-7 text-xs px-2 text-foreground hover:text-primary"
                        disabled={duplicatingId === p.id}
                        onClick={() => handleDuplicate(p.id, p.name)}
                        title="复制此方案"
                      >
                        <Copy className={`h-3 w-3 mr-1 ${duplicatingId === p.id ? "animate-spin" : ""}`} />
                        {duplicatingId === p.id ? "复制中..." : "复制"}
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-7 text-xs px-2 text-foreground hover:text-primary"
                        onClick={() => openEditModal(p)}
                      >
                        <Pencil className="h-3 w-3 mr-1" />
                        编辑
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 w-7 p-0 text-muted-foreground hover:text-destructive"
                        onClick={() => handleDelete(p.id, p.name)}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  </div>
                </div>
              );
            })
          )}
        </div>

        {/* Right column: Selected Profile Inspector */}
        <div className="lg:col-span-7 space-y-4">
          <div className="text-xs font-semibold text-muted-foreground uppercase tracking-wider px-1">
            方案详情配置
          </div>

          {selectedProfile ? (
            <Card className="border-border/60 shadow-sm">
              <CardHeader className="pb-3 border-b">
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                  <div className="space-y-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <CardTitle className="text-lg truncate">{selectedProfile.name}</CardTitle>
                      {selectedProfile.isActive && <Badge variant="success">激活中</Badge>}
                    </div>
                    <p className="text-[11px] text-muted-foreground font-mono truncate">ID: {selectedProfile.id}</p>
                  </div>
                  <div className="flex items-center gap-2 shrink-0 flex-wrap">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => handleTestPrimaryProvider(selectedProfile)}
                      disabled={testingPrimary}
                      className="text-xs"
                      title="探测主节点网络连通性与协议"
                    >
                      <Activity className={`h-3.5 w-3.5 mr-1 ${testingPrimary ? "animate-pulse text-amber-500" : "text-sky-500"}`} />
                      {testingPrimary ? "探测中..." : "连通探测"}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => handleTestPrimaryChat(selectedProfile)}
                      disabled={testingPrimaryChat}
                      className="text-xs border-primary/40 text-primary hover:bg-primary/10"
                      title="向主节点发送真实测试消息以验证模型对话回复"
                    >
                      <MessageSquare className={`h-3.5 w-3.5 mr-1 ${testingPrimaryChat ? "animate-spin" : ""}`} />
                      {testingPrimaryChat ? "对话中..." : "真实对话测试"}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => handleDuplicate(selectedProfile.id, selectedProfile.name)}
                      disabled={duplicatingId === selectedProfile.id}
                      className="text-xs text-foreground hover:text-primary"
                      title="复制当前方案"
                    >
                      <Copy className={`h-3.5 w-3.5 mr-1 ${duplicatingId === selectedProfile.id ? "animate-spin" : ""}`} />
                      {duplicatingId === selectedProfile.id ? "复制中..." : "复制方案"}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => openEditModal(selectedProfile)}
                      className="text-xs"
                    >
                      <Pencil className="h-3.5 w-3.5 mr-1" />
                      编辑方案
                    </Button>
                    {!selectedProfile.isActive && (
                      <Button size="sm" onClick={() => handleSwitch(selectedProfile.id)} className="text-xs">
                        设为激活方案
                      </Button>
                    )}
                  </div>
                </div>
              </CardHeader>
              <CardContent className="p-6 space-y-6">
                {/* Primary Probe Test Banner if available */}
                {primaryTestResult && (
                  <div
                    className={`p-3 rounded-lg border flex items-center gap-2.5 text-xs animate-in fade-in duration-150 ${
                      primaryTestResult.success
                        ? "bg-emerald-500/10 border-emerald-500/30 text-emerald-600 dark:text-emerald-400"
                        : "bg-amber-500/10 border-amber-500/30 text-amber-600 dark:text-amber-400"
                    }`}
                  >
                    {primaryTestResult.success ? (
                      <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-500" />
                    ) : (
                      <AlertCircle className="h-4 w-4 shrink-0 text-amber-500" />
                    )}
                    <div className="flex-1 truncate">{primaryTestResult.message}</div>
                  </div>
                )}

                
                {/* Primary Real Chat Result Banner */}
                {primaryChatResult && (
                  <div
                    className={`p-3.5 rounded-xl border text-xs space-y-2 animate-in fade-in duration-150 ${
                      primaryChatResult.success
                        ? "bg-emerald-500/5 border-emerald-500/30 text-foreground"
                        : "bg-destructive/10 border-destructive/30 text-destructive"
                    }`}
                  >
                    <div className="flex items-center justify-between font-medium">
                      <div className="flex items-center gap-2">
                        {primaryChatResult.success ? (
                          <CheckCircle2 className="h-4 w-4 text-emerald-500 shrink-0" />
                        ) : (
                          <AlertCircle className="h-4 w-4 text-destructive shrink-0" />
                        )}
                        <span className={primaryChatResult.success ? "text-emerald-600 dark:text-emerald-400 font-semibold" : "font-semibold"}>
                          {primaryChatResult.success
                            ? `真实对话测试成功 (耗时: ${"latencyMs" in primaryChatResult ? primaryChatResult.latencyMs : 0}ms)`
                            : "真实对话测试失败"}
                        </span>
                      </div>
                      {"model" in primaryChatResult && primaryChatResult.model && (
                        <Badge variant="outline" className="text-[10px] font-mono">
                          模型: {primaryChatResult.model}
                        </Badge>
                      )}
                    </div>

                    {"reply" in primaryChatResult && primaryChatResult.reply && (
                      <div className="p-3 rounded-lg bg-background/90 border border-border/80 text-foreground text-xs leading-relaxed font-mono whitespace-pre-wrap select-text shadow-inner">
                        <div className="text-[10px] text-muted-foreground mb-1 flex items-center gap-1 font-sans font-medium">
                          <MessageSquare className="h-3 w-3 text-sky-400" />
                          主节点模型回复：
                        </div>
                        {primaryChatResult.reply}
                      </div>
                    )}

                    {"message" in primaryChatResult && !primaryChatResult.success && (
                      <div className="text-xs leading-relaxed opacity-95">{primaryChatResult.message}</div>
                    )}
                  </div>
                )}

                {/* Providers Section */}
                <div>
                  <h4 className="text-xs font-semibold uppercase text-muted-foreground tracking-wider mb-3 flex items-center gap-1.5">
                    <Server className="h-3.5 w-3.5 text-primary" />
                    绑定的 Provider 服务商 ({selectedProfile.providers?.length ?? 0})
                  </h4>

                  {(!selectedProfile.providers || selectedProfile.providers.length === 0) ? (
                    <div className="p-4 rounded-lg bg-muted/30 border text-xs text-muted-foreground text-center">
                      该方案暂未配置 Provider，点击右上角「编辑方案」即可添加节点。
                    </div>
                  ) : (
                    <div className="space-y-3">
                      {selectedProfile.providers.map((pr) => (
                        <div key={pr.id} className="p-3.5 rounded-lg border bg-card/40 space-y-2">
                          <div className="flex items-center justify-between gap-2">
                            <div className="flex items-center gap-2 flex-wrap">
                              <span className="font-semibold text-xs">{pr.name}</span>
                              {pr.isPrimary && <Badge variant="default" className="text-[10px]">Primary 主节点</Badge>}
                              <Badge variant="outline" className="text-[10px] font-mono">
  {pr.protocol === "responses" ? "OpenAI (/v1/responses 原生)" :
   pr.protocol === "openai" ? "OpenAI (/v1/chat/completions 兼容)" :
   pr.protocol === "anthropic" ? "Anthropic (/v1/messages)" :
   pr.protocol === "gemini" ? "Gemini (generateContent)" :
   pr.protocol === "azure" ? "Azure OpenAI" : pr.protocol}
</Badge>
                            </div>
                            <Badge variant="info" className="text-[10px]">{pr.defaultModel}</Badge>
                          </div>
                          <div className="text-[11px] text-muted-foreground font-mono truncate">
                            BaseURL: {pr.baseUrl}
                          </div>
                          <div className="flex flex-wrap gap-2 text-[10px] text-muted-foreground pt-1">
                            <span>工具兼容: <b className="text-foreground">{pr.codexCompat || "auto"}</b></span>
                            <span>·</span>
                            <span>思考推理: <b className="text-foreground">{pr.reasoningConfidence || "unknown"}</b></span>
                            <span>·</span>
                            {pr.rateLimit?.enabled ? (
                              <span className="text-sky-500 font-medium flex items-center gap-1" data-testid={`inspector-ratelimit-badge-${pr.id}`}>
                                <Gauge className="h-3 w-3" />
                                限流: {pr.rateLimit.rpm} RPM / {pr.rateLimit.tpm >= 1000 ? `${Math.round(pr.rateLimit.tpm / 1000)}k` : pr.rateLimit.tpm} TPM {pr.rateLimit.adaptive ? '(自适应)' : ''}
                              </span>
                            ) : (
                              <span className="text-muted-foreground opacity-70" data-testid={`inspector-ratelimit-badge-${pr.id}`}>
                                未设速率限制
                              </span>
                            )}
                            {pr.acceptInvalidCerts && (
                              <>
                                <span>·</span>
                                <span className="text-amber-500 font-medium">允许自签名证书</span>
                              </>
                            )}
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </div>

                {/* Features & Options */}
                <div className="grid grid-cols-2 gap-3 pt-2">
                  <div className="p-3 rounded-lg border bg-muted/20 space-y-1">
                    <div className="text-xs font-medium flex items-center gap-1.5">
                      <Zap className="h-3.5 w-3.5 text-amber-500" /> 本地网关加速
                    </div>
                    <p className="text-[11px] text-muted-foreground">
                      {selectedProfile.gatewayEnabled !== false ? "启用 (智能模型重写与流适配)" : "未启用"}
                    </p>
                  </div>
                  <div className="p-3 rounded-lg border bg-muted/20 space-y-1">
                    <div className="text-xs font-medium flex items-center gap-1.5">
                      <ShieldCheck className="h-3.5 w-3.5 text-emerald-500" /> 多节点故障转移
                    </div>
                    <p className="text-[11px] text-muted-foreground">
                      {selectedProfile.failoverEnabled ? "启用 (熔断器与自动切换)" : "未启用"}
                    </p>
                  </div>
                </div>

                {/* Bound Clients */}
                <div className="pt-2 border-t">
                  <h4 className="text-xs font-semibold uppercase text-muted-foreground tracking-wider mb-2 flex items-center gap-1.5">
                    <Laptop className="h-3.5 w-3.5 text-primary" />
                    关联客户端 ({selectedProfile.clients?.length ?? 0})
                  </h4>
                  {(!selectedProfile.clients || selectedProfile.clients.length === 0) ? (
                    <p className="text-xs text-muted-foreground">全局生效 / 未指定绑定特定客户端</p>
                  ) : (
                    <div className="flex flex-wrap gap-2">
                      {selectedProfile.clients.map((cid) => {
                        const opt = allClientOptions.find((c) => c.id === cid);
                        return (
                          <Badge key={cid} variant="secondary" className="text-xs py-1 px-2.5 font-normal">
                            {opt ? opt.name : cid}
                          </Badge>
                        );
                      })}
                    </div>
                  )}
                </div>
              </CardContent>
            </Card>
          ) : (
            <Card>
              <CardContent className="p-12 text-center text-muted-foreground text-xs">
                请从左侧选择一个方案查看详情。
              </CardContent>
            </Card>
          )}
        </div>
      </div>

      {/* Builtin Templates Section */}
      {templates.length > 0 && (
        <div className="space-y-4 pt-4 border-t">
          <div className="flex items-center gap-2">
            <Sparkles className="h-4 w-4 text-amber-500" />
            <h2 className="text-lg font-bold">内置方案模板快速创建</h2>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            {templates.map((tpl) => (
              <Card key={tpl.id} className="border-border/60 hover:border-primary/50 transition-all flex flex-col justify-between">
                <CardHeader className="pb-2">
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-sm font-semibold">{tpl.name}</CardTitle>
                    <Badge variant="outline" className="text-[10px] font-mono">{tpl.provider?.protocol}</Badge>
                  </div>
                  <p className="text-xs text-muted-foreground line-clamp-2">{tpl.description}</p>
                </CardHeader>
                <CardContent className="pt-0">
                  <div className="text-[11px] text-muted-foreground font-mono mb-3">
                    默认模型: {tpl.provider?.defaultModel}
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    className="w-full text-xs"
                    onClick={() => handleCreateProfile(tpl.name)}
                    disabled={creating}
                  >
                    从该模板创建
                    <ChevronRight className="h-3 w-3 ml-1" />
                  </Button>
                </CardContent>
              </Card>
            ))}
          </div>
        </div>
      )}
      {/* Profile Edit Modal Dialog */}
      {editingProfile && (
        <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-4">
          <div className="bg-card text-card-foreground border rounded-xl shadow-2xl w-full max-w-3xl max-h-[90vh] flex flex-col overflow-hidden">
            {/* Modal Header */}
            <div className="px-6 py-4 border-b flex items-center justify-between bg-muted/20 shrink-0">
              <div className="flex items-center gap-2">
                <div className="p-1.5 rounded-md bg-primary/10 text-primary">
                  <Sliders className="h-4 w-4" />
                </div>
                <div>
                  <h3 className="font-bold text-base">编辑配置方案</h3>
                  <p className="text-xs text-muted-foreground">修改方案基础配置、Provider 节点连通探测与客户端关联</p>
                </div>
              </div>
              <button
                type="button"
                onClick={closeEditModal}
                className="rounded-md p-1 text-muted-foreground hover:text-foreground hover:bg-muted/50 transition-colors"
              >
                <X className="h-4 w-4" />
              </button>
            </div>

            {/* Modal Tabs */}
            <div className="px-6 pt-3 border-b flex gap-4 shrink-0 bg-background">
              <button
                type="button"
                onClick={() => setEditTab("basics")}
                className={`pb-2.5 text-xs font-medium border-b-2 transition-all ${
                  editTab === "basics"
                    ? "border-primary text-primary font-semibold"
                    : "border-transparent text-muted-foreground hover:text-foreground"
                }`}
              >
                基础设置
              </button>
              <button
                type="button"
                onClick={() => setEditTab("providers")}
                className={`pb-2.5 text-xs font-medium border-b-2 transition-all flex items-center gap-1.5 ${
                  editTab === "providers"
                    ? "border-primary text-primary font-semibold"
                    : "border-transparent text-muted-foreground hover:text-foreground"
                }`}
              >
                <span>Provider 节点</span>
                <span className="px-1.5 py-0.2 rounded-full text-[10px] bg-muted font-mono">
                  {editProviders.length}
                </span>
              </button>
              <button
                type="button"
                onClick={() => setEditTab("clients")}
                className={`pb-2.5 text-xs font-medium border-b-2 transition-all flex items-center gap-1.5 ${
                  editTab === "clients"
                    ? "border-primary text-primary font-semibold"
                    : "border-transparent text-muted-foreground hover:text-foreground"
                }`}
              >
                <span>客户端绑定</span>
                <span className="px-1.5 py-0.2 rounded-full text-[10px] bg-muted font-mono">
                  {editClients.length}
                </span>
              </button>
            </div>

            {/* Modal Body (Scrollable) */}
            <div className="p-6 overflow-y-auto flex-1 space-y-5">
              {/* Tab 1: Basics */}
              {editTab === "basics" && (
                <div className="space-y-4">
                  <div className="space-y-1.5">
                    <label className="text-xs font-medium text-foreground">方案名称 (Name)</label>
                    <Input
                      value={editName}
                      onChange={(e) => setEditName(e.target.value)}
                      placeholder="输入方案名称"
                      className="text-xs"
                    />
                  </div>

                  <div className="space-y-3 pt-2">
                    <div className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                      功能策略
                    </div>

                    <label className="flex items-start gap-3 p-3.5 rounded-lg border bg-muted/10 hover:bg-muted/20 cursor-pointer transition-all">
                      <input
                        type="checkbox"
                        checked={editGatewayEnabled}
                        onChange={(e) => setEditGatewayEnabled(e.target.checked)}
                        className="mt-0.5 rounded border-input text-primary focus:ring-primary h-4 w-4"
                      />
                      <div className="space-y-0.5 flex-1">
                        <div className="text-xs font-medium text-foreground flex items-center gap-1.5">
                          <Zap className="h-3.5 w-3.5 text-amber-500" />
                          启用本地网关加速与流式转译 (Gateway)
                        </div>
                        <p className="text-[11px] text-muted-foreground">
                          通过本地 18888 智能网关处理请求，自动进行 Responses 协议转译与思考推理流适配。
                        </p>
                      </div>
                    </label>

                    <label className="flex items-start gap-3 p-3.5 rounded-lg border bg-muted/10 hover:bg-muted/20 cursor-pointer transition-all">
                      <input
                        type="checkbox"
                        checked={editFailoverEnabled}
                        onChange={(e) => setEditFailoverEnabled(e.target.checked)}
                        className="mt-0.5 rounded border-input text-primary focus:ring-primary h-4 w-4"
                      />
                      <div className="space-y-0.5 flex-1">
                        <div className="text-xs font-medium text-foreground flex items-center gap-1.5">
                          <ShieldCheck className="h-3.5 w-3.5 text-emerald-500" />
                          启用多节点智能故障转移 (Failover)
                        </div>
                        <p className="text-[11px] text-muted-foreground">
                          当主 Provider 节点触发连续超时或 5xx 错误时，熔断器自动无缝切换至备用可用节点。
                        </p>
                      </div>
                    </label>
                  </div>
                </div>
              )}

              {/* Tab 2: Providers */}
              {editTab === "providers" && (
                <div className="space-y-4">
                  <div className="flex items-center justify-between gap-2">
                    <p className="text-xs text-muted-foreground">
                      配置大模型 API 节点，支持在线连通性探测、协议自动识别与模型列表拉取。
                    </p>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={handleAddProvider}
                      className="text-xs h-7 shrink-0"
                    >
                      <Plus className="h-3 w-3 mr-1" />
                      添加 Provider 节点
                    </Button>
                  </div>

                  {editProviders.length === 0 ? (
                    <div className="p-6 rounded-lg border border-dashed text-center text-xs text-muted-foreground space-y-2">
                      <p>当前方案尚未添加任何 Provider 节点。</p>
                      <Button type="button" size="sm" variant="outline" onClick={handleAddProvider} className="text-xs">
                        立即添加第一个节点
                      </Button>
                    </div>
                  ) : (
                    <div className="space-y-5">
                      {editProviders.map((prov, index) => {
                        const pState = probeStates[index];
                        // Suggestions for the alias-target fields. They stay
                        // suggestions rather than a closed list: a provider may
                        // serve models that probing never reported.
                        const knownModelIds = Array.from(
                          new Set([...(prov.models || []), ...(pState?.models?.map((m) => m.id) || [])])
                        ).filter(Boolean);
                        return (
                          <div key={prov.id || index} className="p-4 rounded-xl border bg-card/60 space-y-3.5 shadow-sm">
                            {/* Provider Item Top */}
                            <div className="flex items-center justify-between gap-2 border-b pb-2.5">
                              <div className="flex items-center gap-2 flex-wrap">
                                <span className="text-xs font-bold text-muted-foreground">#{index + 1}</span>
                                <span className="text-xs font-semibold">{prov.name || "未命名节点"}</span>
                                {prov.isPrimary ? (
                                  <Badge variant="default" className="text-[10px]">主节点 (Primary)</Badge>
                                ) : (
                                  <Button
                                    type="button"
                                    variant="ghost"
                                    size="sm"
                                    className="h-6 text-[10px] px-2 text-muted-foreground hover:text-primary"
                                    onClick={() => handleSetPrimaryProvider(index)}
                                  >
                                    设为主节点
                                  </Button>
                                )}
                              </div>

                              <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                className="h-6 w-6 p-0 text-muted-foreground hover:text-destructive"
                                onClick={() => handleRemoveProvider(index)}
                                title="删除该节点"
                              >
                                <Trash2 className="h-3.5 w-3.5" />
                              </Button>
                            </div>

                            {/* Preset Quick-Fill Bar */}
                            <div className="flex items-center gap-2 p-2 rounded-lg bg-muted/20 text-xs">
                              <span className="text-[11px] text-muted-foreground shrink-0 flex items-center gap-1">
                                <Sparkles className="h-3 w-3 text-amber-500" />
                                快捷填入预设:
                              </span>
                              <select
                                defaultValue=""
                                onChange={(e) => {
                                  if (e.target.value) {
                                    handleApplyPresetToProvider(index, e.target.value);
                                    e.target.value = "";
                                  }
                                }}
                                className="h-7 text-xs rounded border border-input bg-background px-2 text-foreground focus:outline-none focus:ring-1 focus:ring-primary flex-1 max-w-xs"
                              >
                                <option value="" disabled>选择预设服务商配置...</option>
                                {PROVIDER_PRESETS.map((pst) => (
                                  <option key={pst.name} value={pst.name}>
                                    {pst.name} ({pst.defaultModel})
                                  </option>
                                ))}
                              </select>
                            </div>

                            {/* Inputs Row 1: Name & Protocol */}
                            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                              <div className="space-y-1">
                                <label className="text-[11px] font-medium text-muted-foreground">节点名称</label>
                                <Input
                                  value={prov.name}
                                  onChange={(e) => handleUpdateProviderField(index, "name", e.target.value)}
                                  placeholder="例如: OpenAI 官方节点"
                                  className="text-xs h-8"
                                />
                              </div>
                              <div className="space-y-1">
                                <div className="flex items-center justify-between">
                                  <label className="text-[11px] font-medium text-muted-foreground">协议类型 (Protocol)</label>
                                  <span className="text-[10px] text-primary/80 font-mono">
                                    {prov.protocol === "responses" ? "/v1/responses" :
                                     prov.protocol === "openai" ? "/v1/chat/completions" :
                                     prov.protocol === "anthropic" ? "/v1/messages" :
                                     prov.protocol === "gemini" ? "generateContent" :
                                     prov.protocol === "azure" ? "azure deployments" : "auto"}
                                  </span>
                                </div>
                                <select
                                  value={prov.protocol}
                                  onChange={(e) =>
                                    (() => {
                                      const p = e.target.value as ProtocolKind;
                                      handleUpdateProviderField(index, "protocol", p);
                                      if (p === "responses") {
                                        handleUpdateProviderField(index, "codexCompat", "responses_custom");
                                      } else if (p === "openai") {
                                        handleUpdateProviderField(index, "codexCompat", "chat_function");
                                      }
                                    })()
                                  }
                                  className="w-full h-8 text-xs rounded-md border border-input bg-background px-2.5 py-1 text-foreground focus:outline-none focus:ring-1 focus:ring-primary font-medium"
                                >
                                  <option value="responses">OpenAI Responses 原生协议 (/v1/responses - OpenAI 官方 / Codex 原生)</option>
                                  <option value="openai">OpenAI Chat 兼容协议 (/v1/chat/completions - Hermes / DeepSeek / Ollama)</option>
                                  <option value="anthropic">Anthropic Claude 原生 (/v1/messages - Claude Code / Desktop)</option>
                                  <option value="gemini">Google Gemini 原生 (v1beta generateContent)</option>
                                  <option value="azure">Azure OpenAI 专用终点</option>
                                  <option value="unknown">未知 / 自动探测协议</option>
                                </select>
                              </div>
                            </div>

                            {/* Inputs Row 2: BaseURL */}
                            <div className="space-y-1">
                              <label className="text-[11px] font-medium text-muted-foreground">Base URL 接口地址</label>
                              <Input
                                value={prov.baseUrl}
                                onChange={(e) => handleUpdateProviderField(index, "baseUrl", e.target.value)}
                                placeholder="https://api.openai.com/v1"
                                className="text-xs h-8 font-mono"
                              />
                            </div>

                            {/* Inputs Row 3: API Key & Probe Test Action */}
                            <div className="p-3 rounded-lg border bg-muted/15 space-y-2.5">
                                                            <div className="flex items-center justify-between flex-wrap gap-2">
                                <label className="text-[11px] font-medium text-muted-foreground flex items-center gap-1">
                                  <Key className="h-3 w-3 text-primary" />
                                  API Key 探测与对话测试 (可选)
                                </label>
                                <div className="flex items-center gap-2">
                                  <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    onClick={() => handleProbeProviderNode(index)}
                                    disabled={pState?.loading || !prov.baseUrl.trim()}
                                    className="h-6 text-[11px] px-2 text-primary hover:bg-primary/10 border-primary/30"
                                  >
                                    <Radio className={`h-3 w-3 mr-1 ${pState?.loading ? "animate-spin" : ""}`} />
                                    {pState?.loading ? "正在探测..." : "探测连通与模型"}
                                  </Button>
                                  <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    onClick={() => handleChatTestProviderNode(index)}
                                    disabled={nodeChatStates[index]?.loading || !prov.baseUrl.trim()}
                                    className="h-6 text-[11px] px-2 border-primary/40 text-primary hover:bg-primary/10"
                                  >
                                    <MessageSquare className={`h-3 w-3 mr-1 ${nodeChatStates[index]?.loading ? "animate-spin" : ""}`} />
                                    {nodeChatStates[index]?.loading ? "测试中..." : "真实对话测试"}
                                  </Button>
                                </div>
                              </div>

                              <div className="relative flex items-center">
                                <Input
                                  type={showProviderKeys[index] ? "text" : "password"}
                                  value={providerKeys[index] || ""}
                                  onChange={(e) =>
                                    setProviderKeys({ ...providerKeys, [index]: e.target.value })
                                  }
                                  placeholder="sk-... (已自动填充已保存的 API Key，可点击眼睛图标查看或修改)"
                                  className="text-xs h-8 font-mono pr-8"
                                  data-testid={`provider-key-input-${index}`}
                                />
                                <button
                                  type="button"
                                  onClick={() =>
                                    setShowProviderKeys((prev) => ({
                                      ...prev,
                                      [index]: !prev[index],
                                    }))
                                  }
                                  className="absolute right-2 text-muted-foreground hover:text-foreground transition-colors p-0.5 rounded focus:outline-none"
                                  title={showProviderKeys[index] ? "隐藏 API Key" : "显示 API Key"}
                                  data-testid={`provider-key-toggle-${index}`}
                                >
                                  {showProviderKeys[index] ? (
                                    <EyeOff className="h-3.5 w-3.5" />
                                  ) : (
                                    <Eye className="h-3.5 w-3.5" />
                                  )}
                                </button>
                              </div>

                              
                              {/* Node Real Chat Feedback */}
                              {nodeChatStates[index]?.result && (
                                <div
                                  className={`p-2.5 rounded-lg border text-xs space-y-1.5 ${
                                    nodeChatStates[index].result.success
                                      ? "bg-emerald-500/10 border-emerald-500/20 text-foreground"
                                      : "bg-destructive/10 border-destructive/20 text-destructive"
                                  }`}
                                >
                                  <div className="flex items-center justify-between font-medium">
                                    <div className="flex items-center gap-1.5">
                                      {nodeChatStates[index].result.success ? (
                                        <CheckCircle2 className="h-3.5 w-3.5 text-emerald-500" />
                                      ) : (
                                        <AlertCircle className="h-3.5 w-3.5 text-destructive" />
                                      )}
                                      <span className={nodeChatStates[index].result.success ? "text-emerald-600 dark:text-emerald-400 font-semibold" : ""}>
                                        {nodeChatStates[index].result.success
                                          ? `对话成功 (${"latencyMs" in nodeChatStates[index].result ? nodeChatStates[index].result.latencyMs : 0}ms)`
                                          : "对话测试失败"}
                                      </span>
                                    </div>
                                    {"model" in nodeChatStates[index].result && nodeChatStates[index].result.model && (
                                      <span className="font-mono text-[10px] opacity-80">
                                        {nodeChatStates[index].result.model}
                                      </span>
                                    )}
                                  </div>
                                  {"reply" in nodeChatStates[index].result && nodeChatStates[index].result.reply && (
                                    <div className="p-2 rounded bg-background/80 border text-foreground text-xs leading-relaxed font-mono whitespace-pre-wrap select-text">
                                      {nodeChatStates[index].result.reply}
                                    </div>
                                  )}
                                  {"message" in nodeChatStates[index].result && !nodeChatStates[index].result.success && (
                                    <div className="text-xs opacity-90">{nodeChatStates[index].result.message}</div>
                                  )}
                                </div>
                              )}

                              {/* Probe Feedback Message */}
                              {pState?.message && (
                                <div
                                  className={`p-2 rounded text-[11px] flex items-center gap-2 ${
                                    pState.success
                                      ? "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20"
                                      : "bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20"
                                  }`}
                                >
                                  {pState.success ? (
                                    <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-emerald-500" />
                                  ) : (
                                    <AlertCircle className="h-3.5 w-3.5 shrink-0 text-amber-500" />
                                  )}
                                  <span className="flex-1">{pState.message}</span>
                                </div>
                              )}
                            </div>

                            {/* Inputs Row 4: Default Model & Quick Model Picker */}
                            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                              <div className="space-y-1">
                                <label className="text-[11px] font-medium text-muted-foreground">默认模型 (Default Model)</label>
                                <Input
                                  value={prov.defaultModel}
                                  onChange={(e) => handleUpdateProviderField(index, "defaultModel", e.target.value)}
                                  placeholder="gpt-4o / deepseek-chat"
                                  className="text-xs h-8 font-mono"
                                />
                              </div>

                              {pState?.models && pState.models.length > 0 ? (
                                <div className="space-y-1">
                                  <label className="text-[11px] font-medium text-muted-foreground flex items-center gap-1">
                                    <ListFilter className="h-3 w-3 text-primary" />
                                    从探测到的模型列表中选择
                                  </label>
                                  <select
                                    value={prov.defaultModel}
                                    onChange={(e) => handleUpdateProviderField(index, "defaultModel", e.target.value)}
                                    className="w-full h-8 text-xs rounded-md border border-input bg-background px-2.5 py-1 text-foreground focus:outline-none focus:ring-1 focus:ring-primary font-mono"
                                  >
                                    <option value="" disabled>选择已探测模型...</option>
                                    {pState.models.map((m) => (
                                      <option key={m.id} value={m.id}>
                                        {m.name || m.id}
                                      </option>
                                    ))}
                                  </select>
                                </div>
                              ) : (
                                <div className="space-y-1">
                                  <label className="text-[11px] font-medium text-muted-foreground">Codex 工具兼容模式</label>
                                  <select
                                    value={prov.codexCompat || "responses_custom"}
                                    onChange={(e) =>
                                      handleUpdateProviderField(index, "codexCompat", e.target.value as CodexToolCompat)
                                    }
                                    className="w-full h-8 text-xs rounded-md border border-input bg-background px-2.5 py-1 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                                  >
                                    <option value="responses_custom">Responses 原生自定义工具</option>
                                    <option value="responses_function">Responses 函数调用包装</option>
                                    <option value="chat_function">Chat Completions Function</option>
                                    <option value="none">无工具调用</option>
                                    <option value="unknown">自动探测</option>
                                  </select>
                                </div>
                              )}
                            </div>

                            {/* Inputs Row 5: Tool & Reasoning Options */}
                            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-1">
                              {pState?.models && pState.models.length > 0 && (
                                <div className="space-y-1">
                                  <label className="text-[11px] font-medium text-muted-foreground">Codex 工具兼容模式</label>
                                  <select
                                    value={prov.codexCompat || "responses_custom"}
                                    onChange={(e) =>
                                      handleUpdateProviderField(index, "codexCompat", e.target.value as CodexToolCompat)
                                    }
                                    className="w-full h-8 text-xs rounded-md border border-input bg-background px-2.5 py-1 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                                  >
                                    <option value="responses_custom">Responses 原生自定义工具</option>
                                    <option value="responses_function">Responses 函数调用包装</option>
                                    <option value="chat_function">Chat Completions Function</option>
                                    <option value="none">无工具调用</option>
                                    <option value="unknown">自动探测</option>
                                  </select>
                                </div>
                              )}

                              <div className="space-y-1">
                                <label className="text-[11px] font-medium text-muted-foreground">思考推理识别 (Reasoning)</label>
                                <select
                                  value={prov.reasoningConfidence || "unknown"}
                                  onChange={(e) =>
                                    handleUpdateProviderField(
                                      index,
                                      "reasoningConfidence",
                                      e.target.value as ReasoningConfidence
                                    )
                                  }
                                  className="w-full h-8 text-xs rounded-md border border-input bg-background px-2.5 py-1 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                                >
                                  <option value="unknown">未知 (未探测)</option>
                                  <option value="declared">声明支持 (Declared)</option>
                                  <option value="validated">已验证 (Validated)</option>
                                  <option value="verified">原生完全认证 (Verified)</option>
                                </select>
                              </div>
                            </div>

                            {/* Checkbox: Accept invalid certs */}
                            <label className="flex items-center gap-2 pt-1 cursor-pointer select-none">
                              <input
                                type="checkbox"
                                checked={Boolean(prov.acceptInvalidCerts)}
                                onChange={(e) =>
                                  handleUpdateProviderField(index, "acceptInvalidCerts", e.target.checked)
                                }
                                className="rounded border-input text-primary focus:ring-primary h-3.5 w-3.5"
                              />
                              <span className="text-[11px] text-muted-foreground">
                                允许无效或自签名 SSL 证书 (适用于局域网或本地反代服务)
                              </span>
                            </label>

                            
                            {/* Claude Code Aliases & Thinking Section */}
                            <div className="p-3.5 rounded-lg border bg-muted/10 space-y-3">
                              <div className="flex items-center gap-2">
                                <Sparkles className="h-4 w-4 text-purple-500" />
                                <div>
                                  <div className="text-xs font-semibold flex items-center gap-1.5">
                                    Claude Code 别名映射与思考深度 (Model Aliases & Reasoning)
                                  </div>
                                  <div className="text-[10px] text-muted-foreground">
                                    为 Claude Code 的 opus/sonnet/haiku 别名指定目标模型与显示名，并配置思考深度及 1M 长上下文
                                  </div>
                                </div>
                              </div>

                              {/* Multi-tier Aliases: Opus / Sonnet / Haiku */}
                              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                                {ALIAS_TIERS.map((tier) => (
                                  <div key={tier.field} className="space-y-1">
                                    <label
                                      className="text-[11px] font-medium text-muted-foreground"
                                      htmlFor={`provider-${index}-${tier.field}`}
                                    >
                                      {tier.label} 别名映射 ({tier.alias})
                                    </label>
                                    <input
                                      id={`provider-${index}-${tier.field}`}
                                      list={`provider-${index}-model-options`}
                                      value={prov[tier.field] || ""}
                                      onChange={(e) =>
                                        handleUpdateProviderField(index, tier.field, e.target.value || null)
                                      }
                                      placeholder="自动推断 (默认最优匹配)"
                                      spellCheck={false}
                                      autoComplete="off"
                                      className="w-full h-8 text-xs rounded-md border border-input bg-background px-2.5 py-1 text-foreground focus:outline-none focus:ring-1 focus:ring-primary font-mono"
                                    />
                                  </div>
                                ))}
                              </div>
                              {/* Shared suggestion list: the fields accept any
                                  model id, including ones probing never saw. */}
                              <datalist id={`provider-${index}-model-options`}>
                                {knownModelIds.map((mId) => (
                                  <option key={mId} value={mId} />
                                ))}
                              </datalist>

                              {/* Display names Claude Code shows for each tier */}
                              <div className="space-y-1.5 pt-1 border-t">
                                <div className="text-[11px] font-medium text-muted-foreground pt-1.5">
                                  Claude Code 显示名 (Display Names)
                                </div>
                                <div className="text-[10px] text-muted-foreground">
                                  写入 <code className="font-mono">~/.claude.json</code> 的模型名。Claude
                                  Code 只对认识的名字启用对应的上下文长度与计费，裸别名 opus/sonnet/haiku
                                  在 <code className="font-mono">/model</code> 选择器、
                                  <code className="font-mono">--model</code> 参数和 subagent
                                  frontmatter 里解析不一致。留空使用默认的最新模型名。仅网关开启时生效。
                                </div>
                                <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                                  {ALIAS_TIERS.map((tier) => (
                                    <div key={tier.displayField} className="space-y-1">
                                      <label
                                        className="text-[11px] font-medium text-muted-foreground"
                                        htmlFor={`provider-${index}-${tier.displayField}`}
                                      >
                                        {tier.label} 显示名
                                      </label>
                                      <input
                                        id={`provider-${index}-${tier.displayField}`}
                                        value={prov[tier.displayField] || ""}
                                        onChange={(e) =>
                                          handleUpdateProviderField(
                                            index,
                                            tier.displayField,
                                            e.target.value || null
                                          )
                                        }
                                        placeholder={tier.defaultDisplayName}
                                        spellCheck={false}
                                        autoComplete="off"
                                        className="w-full h-8 text-xs rounded-md border border-input bg-background px-2.5 py-1 text-foreground focus:outline-none focus:ring-1 focus:ring-primary font-mono"
                                      />
                                    </div>
                                  ))}
                                </div>
                              </div>

                              {/* Thinking Effort & 1M Context */}
                              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                <div className="space-y-1">
                                  <label className="text-[11px] font-medium text-muted-foreground">思考深度等级 (Thinking Effort Level)</label>
                                  <select
                                    value={prov.defaultEffortLevel || ""}
                                    onChange={(e) =>
                                      handleUpdateProviderField(index, "defaultEffortLevel", e.target.value || null)
                                    }
                                    className="w-full h-8 text-xs rounded-md border border-input bg-background px-2.5 py-1 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                                  >
                                    <option value="">不注入思考 (默认)</option>
                                    <option value="none">关闭推理思考 (none)</option>
                                    <option value="low">快速轻度推理 (low - 2048 tokens)</option>
                                    <option value="medium">平衡推理模式 (medium - 8192 tokens)</option>
                                    <option value="high">深度复杂推理 (high - 16384 tokens)</option>
                                    <option value="xhigh">极限深度推理 (xhigh - 32768 tokens)</option>
                                    <option value="max">最大极限推理 (max - 63999 tokens)</option>
                                  </select>
                                  <p className="text-[10px] text-muted-foreground">
                                    仅在下方「思考块签名」为“支持”时才会实际注入。
                                  </p>
                                </div>

                                <div className="space-y-1">
                                  <label className="text-[11px] font-medium text-muted-foreground">思考块签名 (Thinking Signature)</label>
                                  <div className="flex items-center gap-2">
                                    <span className="text-xs text-foreground flex-1 truncate">
                                      {THINKING_SUPPORT_LABELS[prov.thinkingSupport || "unprobed"]}
                                    </span>
                                    <Button
                                      type="button"
                                      variant="outline"
                                      size="sm"
                                      className="h-8 text-xs shrink-0"
                                      disabled={thinkingProbeStates[index]?.loading || !prov.baseUrl.trim()}
                                      onClick={() => handleProbeThinkingSupport(index)}
                                    >
                                      重新探测
                                    </Button>
                                  </div>
                                  {thinkingProbeStates[index]?.message && (
                                    <p className="text-[10px] text-muted-foreground">
                                      {thinkingProbeStates[index]?.message}
                                    </p>
                                  )}
                                </div>

                                <div className="space-y-1">
                                  <label className="text-[11px] font-medium text-muted-foreground">1M 长上下文支持 (1M Context)</label>
                                  <select
                                    value={prov.supports1mContext === true ? "true" : prov.supports1mContext === false ? "false" : ""}
                                    onChange={(e) => {
                                      const val = e.target.value === "true" ? true : e.target.value === "false" ? false : null;
                                      handleUpdateProviderField(index, "supports1mContext", val);
                                    }}
                                    className="w-full h-8 text-xs rounded-md border border-input bg-background px-2.5 py-1 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                                  >
                                    <option value="">自动探测 (未指定)</option>
                                    <option value="true">支持 1M 长上下文 (Enabled - 1000K 上下文)</option>
                                    <option value="false">不支持 (Disabled - 自动剥离 [1m] 后缀)</option>
                                  </select>
                                </div>
                              </div>
                            </div>

                            {/* Rate Limiting & 429 Protection Section */}
                            <div
                              className="p-3.5 rounded-lg border bg-muted/15 space-y-3"
                              data-testid={`provider-ratelimit-section-${index}`}
                            >
                              <div className="flex items-center justify-between flex-wrap gap-2">
                                <div className="flex items-center gap-2">
                                  <Gauge className="h-4 w-4 text-sky-500" />
                                  <div>
                                    <div className="text-xs font-semibold flex items-center gap-1.5">
                                      请求速率与 Token 限流 (RPM / TPM)
                                    </div>
                                    <div className="text-[10px] text-muted-foreground">
                                      本地令牌桶平滑排队，防止 Agent 并发爆发触发上游 429 封禁
                                    </div>
                                  </div>
                                </div>

                                <div className="flex items-center gap-3">
                                  <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    onClick={() => handleProbeRateLimits(index)}
                                    disabled={rateLimitProbeStates[index]?.loading || !prov.baseUrl.trim()}
                                    data-testid={`provider-auto-probe-ratelimit-btn-${index}`}
                                    className="h-6 text-[11px] px-2 text-sky-600 dark:text-sky-400 border-sky-500/30 hover:bg-sky-500/10"
                                    title="通过上游响应头探测或智能算法计算推荐限流阈值"
                                  >
                                    <Sparkles className={`h-3 w-3 mr-1 ${rateLimitProbeStates[index]?.loading ? "animate-spin" : "text-amber-500"}`} />
                                    {rateLimitProbeStates[index]?.loading ? "探测限流中..." : "自动探测，填充推荐值"}
                                  </Button>

                                  <label className="flex items-center gap-1.5 cursor-pointer select-none text-xs font-medium">
                                    <input
                                      type="checkbox"
                                      checked={Boolean(prov.rateLimit?.enabled)}
                                      onChange={(e) => handleUpdateRateLimit(index, "enabled", e.target.checked)}
                                      data-testid={`provider-ratelimit-toggle-${index}`}
                                      className="rounded border-input text-primary focus:ring-primary h-3.5 w-3.5"
                                    />
                                    <span className={prov.rateLimit?.enabled ? "text-primary font-semibold" : "text-muted-foreground"}>
                                      启用限流
                                    </span>
                                  </label>
                                </div>
                              </div>

                              {/* Rate Limit Probe Status Feedback Message */}
                              {rateLimitProbeStates[index]?.message && (
                                <div
                                  data-testid={`provider-ratelimit-probe-msg-${index}`}
                                  className={`p-2 rounded text-[11px] flex items-center gap-2 ${
                                    rateLimitProbeStates[index].success
                                      ? "bg-sky-500/10 text-sky-700 dark:text-sky-300 border border-sky-500/20"
                                      : "bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20"
                                  }`}
                                >
                                  {rateLimitProbeStates[index].success ? (
                                    <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-sky-500" />
                                  ) : (
                                    <AlertCircle className="h-3.5 w-3.5 shrink-0 text-amber-500" />
                                  )}
                                  <span className="flex-1 leading-tight">{rateLimitProbeStates[index].message}</span>
                                </div>
                              )}

                              {/* RPM and TPM Inputs */}
                              <div className={`grid grid-cols-1 sm:grid-cols-2 gap-3 transition-opacity ${prov.rateLimit?.enabled ? "opacity-100" : "opacity-60"}`}>
                                <div className="space-y-1">
                                  <div className="flex items-center justify-between">
                                    <label className="text-[11px] font-medium text-muted-foreground flex items-center gap-1">
                                      <Timer className="h-3 w-3 text-sky-500" />
                                      每分钟请求数 (RPM)
                                    </label>
                                    <span className="text-[10px] text-muted-foreground font-mono">Requests/min</span>
                                  </div>
                                  <Input
                                    type="number"
                                    min={1}
                                    max={10000}
                                    value={prov.rateLimit?.rpm ?? 60}
                                    onChange={(e) => handleUpdateRateLimit(index, "rpm", Math.max(1, parseInt(e.target.value, 10) || 1))}
                                    placeholder="60"
                                    data-testid={`provider-rpm-input-${index}`}
                                    className="text-xs h-8 font-mono"
                                    disabled={!prov.rateLimit?.enabled}
                                  />
                                </div>

                                <div className="space-y-1">
                                  <div className="flex items-center justify-between">
                                    <label className="text-[11px] font-medium text-muted-foreground flex items-center gap-1">
                                      <Gauge className="h-3 w-3 text-sky-500" />
                                      每分钟 Token 数 (TPM)
                                    </label>
                                    <span className="text-[10px] text-muted-foreground font-mono">Tokens/min</span>
                                  </div>
                                  <Input
                                    type="number"
                                    min={100}
                                    max={100000000}
                                    step={1000}
                                    value={prov.rateLimit?.tpm ?? 100000}
                                    onChange={(e) => handleUpdateRateLimit(index, "tpm", Math.max(100, parseInt(e.target.value, 10) || 100))}
                                    placeholder="100000"
                                    data-testid={`provider-tpm-input-${index}`}
                                    className="text-xs h-8 font-mono"
                                    disabled={!prov.rateLimit?.enabled}
                                  />
                                </div>
                              </div>

                              {/* Adaptive Throttling Toggle */}
                              <div className={`pt-1 transition-opacity ${prov.rateLimit?.enabled ? "opacity-100" : "opacity-60"}`}>
                                <label className="flex items-center gap-2 cursor-pointer select-none">
                                  <input
                                    type="checkbox"
                                    checked={prov.rateLimit?.adaptive !== false}
                                    onChange={(e) => handleUpdateRateLimit(index, "adaptive", e.target.checked)}
                                    data-testid={`provider-adaptive-ratelimit-toggle-${index}`}
                                    disabled={!prov.rateLimit?.enabled}
                                    className="rounded border-input text-primary focus:ring-primary h-3.5 w-3.5"
                                  />
                                  <span className="text-[11px] text-muted-foreground">
                                    启用 429 智能自适应动态调速 (捕获上游 429 错误时自动按退避指数压低令牌速率并在网关内部平滑排队重试)
                                  </span>
                                </label>
                              </div>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              )}

              {/* Tab 3: Clients */}
              {editTab === "clients" && (
                <div className="space-y-4">
                  <p className="text-xs text-muted-foreground">
                    选择切换到该方案时，自动同步和分发配置的目标 AI 开发客户端：
                  </p>

                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    {allClientOptions.map((client) => {
                      const isChecked = editClients.includes(client.id);
                      return (
                        <label
                          key={client.id}
                          className={`flex items-center justify-between p-3 rounded-xl border transition-all cursor-pointer select-none ${
                            isChecked
                              ? "border-primary bg-primary/5 shadow-sm"
                              : "border-border bg-card/60 hover:border-border/80"
                          }`}
                        >
                          <div className="flex items-center gap-2.5 min-w-0">
                            <input
                              type="checkbox"
                              checked={isChecked}
                              onChange={() => handleToggleClient(client.id)}
                              className="rounded border-input text-primary focus:ring-primary h-4 w-4"
                            />
                            <div className="min-w-0">
                              <div className="text-xs font-semibold truncate">{client.name}</div>
                              <div className="text-[10px] font-mono text-muted-foreground truncate">
                                ID: {client.id}
                              </div>
                            </div>
                          </div>

                          {client.installed ? (
                            <Badge variant="success" className="text-[10px] px-1.5 py-0 shrink-0">
                              已安装
                            </Badge>
                          ) : (
                            <Badge variant="outline" className="text-[10px] px-1.5 py-0 text-muted-foreground shrink-0">
                              未检测到
                            </Badge>
                          )}
                        </label>
                      );
                    })}
                  </div>
                </div>
              )}
            </div>

            {/* Modal Footer */}
            <div className="px-6 py-3.5 border-t bg-muted/20 flex flex-wrap items-center justify-end gap-2.5 shrink-0">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={closeEditModal}
                disabled={saving}
                className="text-xs"
              >
                取消
              </Button>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                onClick={() => handleSaveProfile(false)}
                disabled={saving || !editName.trim()}
                className="text-xs"
              >
                {saving ? (
                  <>
                    <RotateCw className="h-3.5 w-3.5 mr-1 animate-spin" />
                    保存中...
                  </>
                ) : (
                  <>
                    <Save className="h-3.5 w-3.5 mr-1" />
                    仅保存方案
                  </>
                )}
              </Button>
              <Button
                type="button"
                size="sm"
                onClick={() => handleSaveProfile(true)}
                disabled={saving || !editName.trim()}
                className="text-xs min-w-[120px]"
              >
                {saving ? (
                  <>
                    <RotateCw className="h-3.5 w-3.5 mr-1 animate-spin" />
                    激活中...
                  </>
                ) : (
                  <>
                    <ArrowRight className="h-3.5 w-3.5 mr-1" />
                    保存并立即激活
                  </>
                )}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}