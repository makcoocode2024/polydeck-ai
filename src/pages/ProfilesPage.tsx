import { useEffect, useState, useCallback } from "react";
import { useAtom } from "jotai";
import { profilesAtom, templatesAtom, clientsAtom } from "@/state/profile";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Tabs } from "@/components/ui/tabs";
import { backend } from "@/services/backend";
import type {
  Profile,
  ProviderConfig,
  ProtocolKind,
  CodexToolCompat,
  ReasoningConfidence,
  ThinkingSupport,
  RelayChatCompat,
  ChatTestResult,
  RateLimitSettings,
  ClientBindingView,
  ClaudeCodeParams,
  ClaudeEnvPreview,
} from "@/domain/profile";
import {
  resolveClaudeCodeParams,
  CLAUDE_CODE_TOKEN_MIN,
  CLAUDE_CODE_TOKEN_MAX,
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
  AlertTriangle,
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

import {
  ALIAS_TIERS,
  AUTO_CONFIG_CLIENTS,
  KNOWN_CLIENTS,
  PROVIDER_PRESETS,
  type ProbeState,
} from "./profiles/constants";

export default function ProfilesPage() {
  const [profiles, setProfiles] = useAtom(profilesAtom);
  const [templates, setTemplates] = useAtom(templatesAtom);
  const [detectedClients, setDetectedClients] = useAtom(clientsAtom);
  const [newProfileName, setNewProfileName] = useState("");
  const [loading, setLoading] = useState(profiles.length === 0);
  const [refreshing, setRefreshing] = useState(false);
  const [creating, setCreating] = useState(false);
  const [selectedProfileId, setSelectedProfileId] = useState<string | null>(
    () => profiles[0]?.id || null
  );
  /// Which client follows which profile. Several profiles can be in use at once, so
  /// there is no single "active" one to preselect.
  const [bindings, setBindings] = useState<ClientBindingView[]>([]);

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
  const [editTab, setEditTab] = useState<"basics" | "providers" | "clients" | "params">("basics");
  const [editParams, setEditParams] = useState<ClaudeCodeParams>({
    maxOutputTokens: null,
    maxThinkingTokens: null,
    disableAutoupdater: false,
  });
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

  // What Claude Code's settings file has in effect, read back from disk. Kept
  // separate from `editParams`: that is what would be written, this is what is
  // actually there, and the file is merged so the two can disagree.
  const [envPreview, setEnvPreview] = useState<{
    loading: boolean;
    data?: ClaudeEnvPreview;
    error?: string;
  }>({ loading: false });

  const loadEnvPreview = useCallback(async () => {
    setEnvPreview({ loading: true });
    try {
      setEnvPreview({ loading: false, data: await backend.readClaudeEnvPreview() });
    } catch (e) {
      setEnvPreview({ loading: false, error: String(e) });
    }
  }, []);

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
      const bListPromise = backend.listClientBindings().catch(() => []);

      const [pList, tList, cList, bList] = await Promise.all([
        pListPromise,
        tListPromise,
        cListPromise,
        bListPromise,
      ]);

      setProfiles(pList);
      if (tList.length > 0) setTemplates(tList);
      if (cList.length > 0) setDetectedClients(cList);
      // `?? []` because an older build without this command returns null rather
      // than rejecting, and a null here makes every binding lookup throw mid-render.
      setBindings(bList ?? []);

      setSelectedProfileId((prev) => {
        if (prev && pList.some((p) => p.id === prev)) return prev;
        // Prefer whichever profile the most clients follow, so the inspector opens
        // on something in use rather than an arbitrary first row.
        const counts = new Map<string, number>();
        bList.forEach((b) => counts.set(b.profileId, (counts.get(b.profileId) ?? 0) + 1));
        const mostBound = [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
        return (mostBound && pList.some((p) => p.id === mostBound) ? mostBound : pList[0]?.id) || null;
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

  // Read the settings file when the params tab comes into view, not when the modal
  // opens: most visits never reach this tab, and the file changes underneath us on
  // every activation, so a value fetched earlier would already be stale.
  useEffect(() => {
    if (editingProfile && editTab === "params") {
      loadEnvPreview();
    }
  }, [editingProfile, editTab, loadEnvPreview]);

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

  /// Bind clients to a profile. `clients` omitted means the profile's whole target
  /// list, which is what the 激活 button does; naming one client moves just that one.
  const handleActivate = async (id: string, clients?: string[]) => {
    const profile = profiles.find((p) => p.id === id);
    const moving = clients ?? profile?.clients ?? [];
    // Optimistic: move the named clients onto this profile and drop them from
    // wherever they were, mirroring what bind_clients does server-side.
    setBindings((prev) => [
      ...prev.filter((b) => !moving.includes(b.clientId)),
      ...moving.map((clientId) => ({
        clientId,
        profileId: id,
        profileName: profile?.name ?? null,
        gatewayEnabled: profile?.gatewayEnabled ?? false,
        boundAt: new Date().toISOString(),
      })),
    ]);
    setSelectedProfileId(id);
    setPrimaryTestResult(null);
    setPrimaryChatResult(null);

    try {
      await backend.activateProfile(id, clients);
      const [updatedList, updatedBindings] = await Promise.all([
        backend.listProfiles(),
        backend.listClientBindings(),
      ]);
      setProfiles(updatedList);
      setBindings(updatedBindings ?? []);
    } catch (err) {
      alert(`激活失败: ${err instanceof Error ? err.message : String(err)}`);
      backend.listClientBindings().then(setBindings).catch(() => {});
      backend.listProfiles().then((list) => setProfiles(list)).catch(() => {});
    }
  };

  const handleDeactivate = async (clients: string[]) => {
    setBindings((prev) => prev.filter((b) => !clients.includes(b.clientId)));
    try {
      await backend.deactivateClients(clients);
      setBindings((await backend.listClientBindings()) ?? []);
    } catch (err) {
      alert(`解绑失败: ${err instanceof Error ? err.message : String(err)}`);
      backend.listClientBindings().then(setBindings).catch(() => {});
    }
  };

  const handleCopyConnection = async (client: string) => {
    try {
      const info = await backend.clientConnectionInfo(client);
      await navigator.clipboard.writeText(
        `Base URL: ${info.baseUrl}\nAPI Key: ${info.token}`
      );
      alert(`已复制 ${client} 的地址与令牌，粘贴到它自己的设置里即可。`);
    } catch (err) {
      alert(`读取连接信息失败: ${err instanceof Error ? err.message : String(err)}`);
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
    setEditParams({
      maxOutputTokens: profile.claudeCodeParams?.maxOutputTokens ?? null,
      maxThinkingTokens: profile.claudeCodeParams?.maxThinkingTokens ?? null,
      disableAutoupdater: Boolean(profile.claudeCodeParams?.disableAutoupdater),
    });
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
    setEnvPreview({ loading: false });
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
      extraHeaders: {},
    };
    setEditProviders((prev) => [...prev, newProv]);
  };

  const handleRemoveProvider = (index: number) => {
    setEditProviders((prev) => {
      const updated = prev.filter((_, i) => i !== index);
      if (updated.length > 0 && !updated.some((p) => p.isPrimary)) {
        updated[0] = { ...updated[0], isPrimary: true };
      }
      return updated;
    });
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
    setEditProviders((prev) =>
      prev.map((p, i) => ({
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
    // Functional form, not `editProviders.map(...)`: several handlers call this
    // twice for one event (the protocol select also writes codexCompat). Reading
    // the array from the closure makes both calls start from the same stale copy,
    // so the second silently discards the first.
    setEditProviders((prev) =>
      prev.map((p, i) => (i === index ? { ...p, [field]: value } : p))
    );
  };

  const handleApplyPresetToProvider = (index: number, presetName: string) => {
    const preset = PROVIDER_PRESETS.find((p) => p.name === presetName);
    if (!preset) return;
    setEditProviders((prev) =>
      prev.map((p, i) =>
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
      // Only a decided verdict overwrites; `auto` means the probe could not tell
      // and must not clobber a setting the user chose by hand.
      if (res.relayChatCompat && res.relayChatCompat !== "auto") {
        handleUpdateProviderField(index, "relayChatCompat", res.relayChatCompat);
      }
      if (res.models && res.models.length > 0) {
        const ids = res.models.map((m) => m.id);
        setEditProviders((prev) =>
          prev.map((p, i) => {
            if (i !== index) return p;
            // A default carried over from a preset or an earlier probe is not
            // necessarily served here. Left in place, the picker below matches no
            // option and renders as unselected while the text field still shows
            // the old name, so the profile saves a model this provider rejects.
            const keep = ids.includes((p.defaultModel || "").trim());
            const defaultModel = keep ? p.defaultModel : ids[0];
            // Keep the chosen model's reported ceiling so the parameter panel can
            // recommend a measured value instead of only its built-in table.
            const reported = res.models?.find((m) => m.id === defaultModel)?.maxOutputTokens;
            return {
              ...p,
              models: ids,
              defaultModel,
              probedMaxOutputTokens: reported == null ? null : Number(reported),
            };
          })
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

  const RELAY_CHAT_COMPAT_HINTS: Record<RelayChatCompat, string> = {
    auto: "测试连接时自动判定。多数中转站的非流式返回不规范，探测到后会自动改用流式缓冲，无需手动设置。",
    buffered: "非流式请求改为流式发出，缓冲后拼成标准 JSON 返回。仅在自动判定失效、非流式回答异常时选它。",
    direct: "始终按原样发送非流式请求。仅在自动判定误判、你确认上游非流式正常时选它。",
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
    for (const [label, value] of [
      ["最大输出 Token", editParams.maxOutputTokens],
      ["最大思考 Token", editParams.maxThinkingTokens],
    ] as const) {
      if (value != null && (value < CLAUDE_CODE_TOKEN_MIN || value > CLAUDE_CODE_TOKEN_MAX)) {
        alert(`${label} 需在 ${CLAUDE_CODE_TOKEN_MIN} ~ ${CLAUDE_CODE_TOKEN_MAX} 之间`);
        setEditTab("params");
        return;
      }
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
        claudeCodeParams: editParams,
      });
      if (activate) {
        await backend.activateProfile(editingProfile.id);
      }
      setEditingProfile(null);
      setProfiles((prev) => prev.map((p) => (p.id === updated.id ? updated : p)));
      backend.listProfiles().then((list) => setProfiles(list)).catch(() => {});
      if (activate) {
        backend.listClientBindings().then(setBindings).catch(() => {});
      }
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
        primary.acceptInvalidCerts,
        undefined,
        profile.id
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
        prov.acceptInvalidCerts,
        undefined,
        editingProfile?.id || undefined
      );
      console.log('[Cline test] profileId=', editingProfile?.id, 'key_len=', key.length);
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

  // Display names for a profile's client ids. Falls back to the raw id, which is
  // what the inspector already does: a backend that grows a new client would
  // otherwise render an empty slot.
  const clientNames = (ids: string[] | undefined) =>
    (ids ?? []).map((id) => allClientOptions.find((o) => o.id === id)?.name || id);

  const clientName = (id: string) => allClientOptions.find((o) => o.id === id)?.name || id;
  /// Clients currently following a profile, as opposed to `profile.clients`, which
  /// is the set it *would* bind when activated.
  const boundClients = (profileId: string) =>
    bindings.filter((b) => b.profileId === profileId).map((b) => b.clientId);
  const bindingFor = (clientId: string) => bindings.find((b) => b.clientId === clientId);

  /// Every client this profile's chips must show: the ones it targets, plus any
  /// bound to it that the target list omits.
  ///
  /// Rendering the target list alone hid a binding the user could not reach: a
  /// client bound here but dropped from the list showed no chip, so there was
  /// nothing to click, while deletion still refused on account of it.
  const chipClients = (profile: Profile) => {
    const target = profile.clients ?? [];
    const extra = boundClients(profile.id).filter((cid) => !target.includes(cid));
    return [...target, ...extra];
  };

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
                        {boundClients(p.id).length > 0 && (
                          <Badge
                            variant="success"
                            className="text-2xs px-1.5 py-0 shrink-0"
                            title={clientNames(boundClients(p.id)).join("、")}
                          >
                            <CheckCircle2 className="h-2.5 w-2.5 mr-1" />
                            {boundClients(p.id).length} 个已绑定
                          </Badge>
                        )}
                      </div>
                      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-2xs text-muted-foreground">
                        <span>{p.providers?.length ?? 0} 个 Provider</span>
                        <span>·</span>
                        {(() => {
                          const names = clientNames(p.clients);
                          if (names.length === 0) return <span>0 个客户端</span>;
                          // Capped at three: this card is about 440px wide and the
                          // action cluster beside it does not yield space, so the
                          // full list goes in the tooltip instead of wrapping.
                          const shown = names.slice(0, 3).join("、");
                          const rest = names.length - 3;
                          return (
                            <span title={names.join("、")}>
                              {names.length} 个客户端 · {shown}
                              {rest > 0 ? ` +${rest}` : ""}
                            </span>
                          );
                        })()}
                        <span>·</span>
                        <span>{p.gatewayEnabled !== false ? "网关开启" : "网关关闭"}</span>
                      </div>
                    </div>

                    <div className="flex items-center gap-1.5 shrink-0" onClick={(e) => e.stopPropagation()}>
                      {/* Shown even when some clients already follow this profile:
                          it binds the whole target list, so it still has work to do
                          when only part of that list is bound. */}
                      {(p.clients?.length ?? 0) > 0 &&
                        boundClients(p.id).length < (p.clients?.length ?? 0) && (
                          <Button
                            variant="outline"
                            size="sm"
                            className="h-7 text-xs px-2"
                            onClick={() => handleActivate(p.id)}
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
                      {/* Coloured at rest. As a plain ghost it was pixel-identical
                          to the edit button beside it until the pointer arrived. */}
                      <Button
                        variant="destructive-ghost"
                        size="icon-xs"
                        onClick={() => handleDelete(p.id, p.name)}
                        title="删除该方案"
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
                      {boundClients(selectedProfile.id).length > 0 && (
                        <Badge variant="success">
                          {boundClients(selectedProfile.id).length} 个客户端使用中
                        </Badge>
                      )}
                    </div>
                    <p className="text-2xs text-muted-foreground font-mono truncate">ID: {selectedProfile.id}</p>
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
                      <Activity className={`h-3.5 w-3.5 mr-1 ${testingPrimary ? "animate-pulse text-warning" : "text-info"}`} />
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
                    {(selectedProfile.clients?.length ?? 0) >
                      boundClients(selectedProfile.id).length && (
                      <Button
                        size="sm"
                        onClick={() => handleActivate(selectedProfile.id)}
                        className="text-xs"
                      >
                        绑定全部客户端
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
                        ? "bg-success-surface border-success/25 text-success"
                        : "bg-warning-surface border-warning/25 text-warning"
                    }`}
                  >
                    {primaryTestResult.success ? (
                      <CheckCircle2 className="h-4 w-4 shrink-0 text-success" />
                    ) : (
                      <AlertCircle className="h-4 w-4 shrink-0 text-warning" />
                    )}
                    <div className="flex-1 truncate">{primaryTestResult.message}</div>
                  </div>
                )}

                
                {/* Primary Real Chat Result Banner */}
                {primaryChatResult && (
                  <div
                    className={`p-3.5 rounded-xl border text-xs space-y-2 animate-in fade-in duration-150 ${
                      primaryChatResult.success
                        ? "bg-success-surface border-success/25 text-foreground"
                        : "bg-destructive/10 border-destructive/30 text-destructive"
                    }`}
                  >
                    <div className="flex items-center justify-between font-medium">
                      <div className="flex items-center gap-2">
                        {primaryChatResult.success ? (
                          <CheckCircle2 className="h-4 w-4 text-success shrink-0" />
                        ) : (
                          <AlertCircle className="h-4 w-4 text-destructive shrink-0" />
                        )}
                        <span className={primaryChatResult.success ? "text-success font-semibold" : "font-semibold"}>
                          {primaryChatResult.success
                            ? `真实对话测试成功 (耗时: ${"latencyMs" in primaryChatResult ? primaryChatResult.latencyMs : 0}ms)`
                            : "真实对话测试失败"}
                        </span>
                      </div>
                      {"model" in primaryChatResult && primaryChatResult.model && (
                        <Badge variant="outline" className="text-2xs font-mono">
                          模型: {primaryChatResult.model}
                        </Badge>
                      )}
                    </div>

                    {"reply" in primaryChatResult && primaryChatResult.reply && (
                      <div className="p-3 rounded-lg bg-background/90 border border-border/80 text-foreground text-xs leading-relaxed font-mono whitespace-pre-wrap select-text shadow-inner">
                        <div className="text-2xs text-muted-foreground mb-1 flex items-center gap-1 font-sans font-medium">
                          <MessageSquare className="h-3 w-3 text-info" />
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
                              {pr.isPrimary && <Badge variant="default" className="text-2xs">Primary 主节点</Badge>}
                              <Badge variant="outline" className="text-2xs font-mono">
  {pr.protocol === "responses" ? "OpenAI (/v1/responses 原生)" :
   pr.protocol === "openai" ? "OpenAI (/v1/chat/completions 兼容)" :
   pr.protocol === "anthropic" ? "Anthropic (/v1/messages)" :
   pr.protocol === "gemini" ? "Gemini (generateContent)" :
   pr.protocol === "azure" ? "Azure OpenAI" : pr.protocol}
</Badge>
                            </div>
                            <Badge variant="info" className="text-2xs">{pr.defaultModel}</Badge>
                          </div>
                          <div className="text-2xs text-muted-foreground font-mono truncate">
                            BaseURL: {pr.baseUrl}
                          </div>
                          <div className="flex flex-wrap gap-2 text-2xs text-muted-foreground pt-1">
                            <span>工具兼容: <b className="text-foreground">{pr.codexCompat || "auto"}</b></span>
                            <span>·</span>
                            <span>思考推理: <b className="text-foreground">{pr.reasoningConfidence || "unknown"}</b></span>
                            <span>·</span>
                            {pr.rateLimit?.enabled ? (
                              <span className="text-info font-medium flex items-center gap-1" data-testid={`inspector-ratelimit-badge-${pr.id}`}>
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
                                <span className="text-warning font-medium">允许自签名证书</span>
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
                      <Zap className="h-3.5 w-3.5 text-warning" /> 本地网关加速
                    </div>
                    <p className="text-2xs text-muted-foreground">
                      {selectedProfile.gatewayEnabled !== false ? "启用 (智能模型重写与流适配)" : "未启用"}
                    </p>
                  </div>
                  <div className="p-3 rounded-lg border bg-muted/20 space-y-1">
                    <div className="text-xs font-medium flex items-center gap-1.5">
                      <ShieldCheck className="h-3.5 w-3.5 text-success" /> 多节点故障转移
                    </div>
                    <p className="text-2xs text-muted-foreground">
                      {selectedProfile.failoverEnabled ? "启用 (熔断器与自动切换)" : "未启用"}
                    </p>
                  </div>
                </div>

                {/* Bound Clients */}
                <div className="pt-2 border-t">
                  <h4 className="text-xs font-semibold uppercase text-muted-foreground tracking-wider mb-2 flex items-center gap-1.5">
                    <Laptop className="h-3.5 w-3.5 text-primary" />
                    目标客户端 ({selectedProfile.clients?.length ?? 0})
                  </h4>
                  {chipClients(selectedProfile).length === 0 ? (
                    <p className="text-xs text-muted-foreground">
                      此方案未选择任何客户端，激活不会生效。请先在「编辑方案 → 客户端绑定」里勾选。
                    </p>
                  ) : (
                    <>
                      <div className="flex flex-wrap gap-2">
                        {chipClients(selectedProfile).map((cid) => {
                          const binding = bindingFor(cid);
                          const here = binding?.profileId === selectedProfile.id;
                          const elsewhereName = binding && !here ? binding.profileName : null;
                          return (
                            <button
                              key={cid}
                              type="button"
                              data-testid={`client-chip-${cid}`}
                              onClick={() =>
                                here
                                  ? handleDeactivate([cid])
                                  : handleActivate(selectedProfile.id, [cid])
                              }
                              title={
                                here
                                  ? "点击解绑"
                                  : elsewhereName
                                  ? `当前跟随「${elsewhereName}」，点击改绑到本方案`
                                  : "点击绑定到本方案"
                              }
                              className={`text-xs py-1 px-2.5 rounded-md border transition-colors ${
                                here
                                  ? "bg-success-surface border-success/25 text-success"
                                  : elsewhereName
                                  ? "bg-transparent border-border text-muted-foreground hover:border-primary"
                                  : "bg-transparent border-dashed border-border text-muted-foreground hover:border-primary"
                              }`}
                            >
                              {here && <CheckCircle2 className="h-3 w-3 mr-1 inline-block" />}
                              {clientName(cid)}
                              {elsewhereName ? ` → ${elsewhereName}` : ""}
                            </button>
                          );
                        })}
                      </div>
                      <p className="text-2xs text-muted-foreground mt-2">
                        绿色为正跟随本方案；带箭头的正跟随别的方案，点一下即可改过来。
                      </p>
                      {chipClients(selectedProfile).length >
                        (selectedProfile.clients?.length ?? 0) && (
                        <p className="text-2xs text-warning mt-1">
                          有客户端仍跟随本方案，但已不在目标列表里。点它即可解绑——删除方案前必须先解开。
                        </p>
                      )}
                    </>
                  )}
                </div>

                {/* Clients PolyDeck cannot write a config file for */}
                {(() => {
                  const manual = boundClients(selectedProfile.id).filter(
                    (cid) => !AUTO_CONFIG_CLIENTS.includes(cid)
                  );
                  if (manual.length === 0) return null;
                  return (
                    <div className="pt-2 border-t">
                      <h4 className="text-xs font-semibold uppercase text-muted-foreground tracking-wider mb-2 flex items-center gap-1.5">
                        <AlertTriangle className="h-3.5 w-3.5 text-warning" />
                        需要手动填写 ({manual.length})
                      </h4>
                      <p className="text-2xs text-muted-foreground mb-2">
                        这些客户端没有 PolyDeck 能改写的配置文件。绑定已生效、网关会正常路由，
                        但地址和令牌需要你自己粘进它们的设置里。
                      </p>
                      <div className="space-y-2">
                        {manual.map((cid) => (
                          <div
                            key={cid}
                            className="flex items-center gap-2 flex-wrap p-2 rounded-lg border bg-muted/20"
                          >
                            <span className="text-xs font-medium">{clientName(cid)}</span>
                            <Button
                              variant="outline"
                              size="sm"
                              className="h-6 text-2xs px-2"
                              data-testid={`copy-conn-${cid}`}
                              onClick={() => handleCopyConnection(cid)}
                            >
                              复制地址与令牌
                            </Button>
                          </div>
                        ))}
                      </div>
                    </div>
                  );
                })()}
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
            <Sparkles className="h-4 w-4 text-warning" />
            <h2 className="text-lg font-bold">内置方案模板快速创建</h2>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            {templates.map((tpl) => (
              <Card key={tpl.id} className="border-border/60 hover:border-primary/50 transition-all flex flex-col justify-between">
                <CardHeader className="pb-2">
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-sm font-semibold">{tpl.name}</CardTitle>
                    <Badge variant="outline" className="text-2xs font-mono">{tpl.provider?.protocol}</Badge>
                  </div>
                  <p className="text-xs text-muted-foreground line-clamp-2">{tpl.description}</p>
                </CardHeader>
                <CardContent className="pt-0">
                  <div className="text-2xs text-muted-foreground font-mono mb-3">
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
          {/* 5xl, not 3xl: four tabs of dense two-column form did not fit 768px,
              and the Provider tab was the worst of them. 1024px still clears the
              ~1040px of usable width a 1280px window leaves after the sidebar. */}
          <div className="bg-card text-card-foreground border rounded-xl shadow-lg w-full max-w-5xl max-h-[90vh] flex flex-col overflow-hidden animate-slide-up">
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
            <Tabs
              items={[
                { id: "basics", label: "基础设置" },
                { id: "providers", label: "Provider 节点", count: editProviders.length },
                { id: "clients", label: "客户端绑定", count: editClients.length },
                { id: "params", label: "Claude Code 参数" },
              ]}
              value={editTab}
              onChange={setEditTab}
              className="px-6 pt-3 shrink-0 bg-background"
            />

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
                          <Zap className="h-3.5 w-3.5 text-warning" />
                          启用本地网关加速与流式转译 (Gateway)
                        </div>
                        <p className="text-2xs text-muted-foreground">
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
                          <ShieldCheck className="h-3.5 w-3.5 text-success" />
                          启用多节点智能故障转移 (Failover)
                        </div>
                        <p className="text-2xs text-muted-foreground">
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
                                  <Badge variant="default" className="text-2xs">主节点 (Primary)</Badge>
                                ) : (
                                  <Button
                                    type="button"
                                    variant="ghost"
                                    size="sm"
                                    className="h-6 text-2xs px-2 text-muted-foreground hover:text-primary"
                                    onClick={() => handleSetPrimaryProvider(index)}
                                  >
                                    设为主节点
                                  </Button>
                                )}
                              </div>

                              <Button
                                type="button"
                                variant="destructive-ghost"
                                size="icon-xs"
                                onClick={() => handleRemoveProvider(index)}
                                title="删除该节点"
                              >
                                <Trash2 className="h-3.5 w-3.5" />
                              </Button>
                            </div>

                            {/* Preset Quick-Fill Bar */}
                            <div className="flex items-center gap-2 p-2 rounded-lg bg-muted/20 text-xs">
                              <span className="text-2xs text-muted-foreground shrink-0 flex items-center gap-1">
                                <Sparkles className="h-3 w-3 text-warning" />
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
                                <label className="text-2xs font-medium text-muted-foreground">节点名称</label>
                                <Input
                                  value={prov.name}
                                  onChange={(e) => handleUpdateProviderField(index, "name", e.target.value)}
                                  placeholder="例如: OpenAI 官方节点"
                                  className="text-xs h-8"
                                />
                              </div>
                              <div className="space-y-1">
                                <div className="flex items-center justify-between">
                                  <label className="text-2xs font-medium text-muted-foreground">协议类型 (Protocol)</label>
                                  <span className="text-2xs text-primary/80 font-mono">
                                    {prov.protocol === "responses" ? "/v1/responses" :
                                     prov.protocol === "openai" ? "/v1/chat/completions" :
                                     prov.protocol === "anthropic" ? "/v1/messages" :
                                     prov.protocol === "gemini" ? "generateContent" :
                                     prov.protocol === "azure" ? "azure deployments" : "auto"}
                                  </span>
                                </div>
                                <select
                                  data-testid={`protocol-select-${index}`}
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
                              <label className="text-2xs font-medium text-muted-foreground">Base URL 接口地址</label>
                              <Input
                                value={prov.baseUrl}
                                onChange={(e) => handleUpdateProviderField(index, "baseUrl", e.target.value)}
                                placeholder="https://api.openai.com/v1"
                                className="text-xs h-8 font-mono"
                              />
                            </div>

                            {/* Inputs Row 3: API Key & Probe Test Action */}
                            {/* Cline provider: show login button instead of API key input */}
                            {prov.baseUrl.includes("cline.bot") ? (
                            <div className="p-3 rounded-lg border bg-muted/15 space-y-2.5">
                              <div className="flex items-center justify-between flex-wrap gap-2">
                                <label className="text-2xs font-medium text-muted-foreground flex items-center gap-1">
                                  <Key className="h-3 w-3 text-primary" />
                                  Cline 账号登录（自动管理 Token）
                                </label>
                                <div className="flex items-center gap-2">
                                  <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    onClick={async () => {
                                      if (!editingProfile) return;
                                      try {
                                        const auth = await backend.clineStartDeviceAuth();
                                        window.open(auth.verification_uri_complete || auth.verification_uri, "_blank");
                                        setNodeChatStates((prev) => ({
                                          ...prev,
                                          [index]: { loading: true, result: { success: true, reply: `请在浏览器中完成授权...\n用户码: ${auth.user_code}`, latencyMs: 0, model: "", protocol: "openai" as const } },
                                        }));
                                        const result = await backend.clineCompleteDeviceAuth(
                                          editingProfile.id, auth.device_code, auth.expires_in, auth.interval
                                        );
                                        setNodeChatStates((prev) => ({
                                          ...prev,
                                          [index]: { loading: false, result: { success: true, reply: `登录成功！${result.email || ""}`, latencyMs: 0, model: "", protocol: "openai" as const } },
                                        }));
                                      } catch (err) {
                                        setNodeChatStates((prev) => ({
                                          ...prev,
                                          [index]: { loading: false, result: { success: false, message: `Cline 登录失败: ${err instanceof Error ? err.message : String(err)}` } },
                                        }));
                                      }
                                    }}
                                    disabled={nodeChatStates[index]?.loading}
                                    className="h-6 text-2xs px-2 text-primary hover:bg-primary/10 border-primary/30"
                                  >
                                    <UserCheck className={`h-3 w-3 mr-1 ${nodeChatStates[index]?.loading ? "animate-spin" : ""}`} />
                                    {nodeChatStates[index]?.loading ? "等待授权..." : "登录 Cline 账号"}
                                  </Button>
                                  <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    onClick={() => handleChatTestProviderNode(index)}
                                    disabled={nodeChatStates[index]?.loading || !prov.baseUrl.trim()}
                                    className="h-6 text-2xs px-2 border-primary/40 text-primary hover:bg-primary/10"
                                  >
                                    <MessageSquare className={`h-3 w-3 mr-1 ${nodeChatStates[index]?.loading ? "animate-spin" : ""}`} />
                                    {nodeChatStates[index]?.loading ? "测试中..." : "真实对话测试"}
                                  </Button>
                                </div>
                              </div>
                              <p className="text-2xs text-muted-foreground">
                                点击登录后会打开浏览器进行 GitHub 授权，授权完成后 Token 自动保存并自动刷新，无需手动管理 API Key
                              </p>
                            </div>
                            ) : (
                            <div className="p-3 rounded-lg border bg-muted/15 space-y-2.5">
                                                            <div className="flex items-center justify-between flex-wrap gap-2">
                                <label className="text-2xs font-medium text-muted-foreground flex items-center gap-1">
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
                                    className="h-6 text-2xs px-2 text-primary hover:bg-primary/10 border-primary/30"
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
                                    className="h-6 text-2xs px-2 border-primary/40 text-primary hover:bg-primary/10"
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
                                      ? "bg-success-surface border-success/25 text-foreground"
                                      : "bg-destructive/10 border-destructive/20 text-destructive"
                                  }`}
                                >
                                  <div className="flex items-center justify-between font-medium">
                                    <div className="flex items-center gap-1.5">
                                      {nodeChatStates[index].result.success ? (
                                        <CheckCircle2 className="h-3.5 w-3.5 text-success" />
                                      ) : (
                                        <AlertCircle className="h-3.5 w-3.5 text-destructive" />
                                      )}
                                      <span className={nodeChatStates[index].result.success ? "text-success font-semibold" : ""}>
                                        {nodeChatStates[index].result.success
                                          ? `对话成功 (${"latencyMs" in nodeChatStates[index].result ? nodeChatStates[index].result.latencyMs : 0}ms)`
                                          : "对话测试失败"}
                                      </span>
                                    </div>
                                    {"model" in nodeChatStates[index].result && nodeChatStates[index].result.model && (
                                      <span className="font-mono text-2xs opacity-80">
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
                                  className={`p-2 rounded text-2xs flex items-center gap-2 ${
                                    pState.success
                                      ? "bg-success-surface text-success border border-success/25"
                                      : "bg-warning-surface text-warning border border-warning/25"
                                  }`}
                                >
                                  {pState.success ? (
                                    <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-success" />
                                  ) : (
                                    <AlertCircle className="h-3.5 w-3.5 shrink-0 text-warning" />
                                  )}
                                  <span className="flex-1">{pState.message}</span>
                                </div>
                              )}
                            </div>
                            )}

                            {/* Inputs Row 4: Default Model & Quick Model Picker */}
                            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                              <div className="space-y-1">
                                <label className="text-2xs font-medium text-muted-foreground">默认模型 (Default Model)</label>
                                <Input
                                  data-testid={`provider-default-model-input-${index}`}
                                  value={prov.defaultModel}
                                  onChange={(e) => handleUpdateProviderField(index, "defaultModel", e.target.value)}
                                  placeholder="gpt-4o / deepseek-chat"
                                  className="text-xs h-8 font-mono"
                                />
                              </div>

                              {pState?.models && pState.models.length > 0 ? (
                                <div className="space-y-1">
                                  <label className="text-2xs font-medium text-muted-foreground flex items-center gap-1">
                                    <ListFilter className="h-3 w-3 text-primary" />
                                    从探测到的模型列表中选择
                                  </label>
                                  <select
                                    data-testid={`provider-default-model-select-${index}`}
                                    value={
                                      pState.models.some((m) => m.id === prov.defaultModel)
                                        ? prov.defaultModel
                                        : ""
                                    }
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
                                  <label className="text-2xs font-medium text-muted-foreground">Codex 工具兼容模式</label>
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
                                  <label className="text-2xs font-medium text-muted-foreground">Codex 工具兼容模式</label>
                                  <select
                                    data-testid={`codex-compat-select-${index}`}
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
                                <label className="text-2xs font-medium text-muted-foreground">思考推理识别 (Reasoning)</label>
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
                              <span className="text-2xs text-muted-foreground">
                                允许无效或自签名 SSL 证书 (适用于局域网或本地反代服务)
                              </span>
                            </label>

                            {/* Extra Headers (key-value pairs) */}
                            <div className="space-y-2 pt-1">
                              <div className="flex items-center justify-between">
                                <label className="text-2xs font-medium text-muted-foreground">
                                  自定义请求头 (Extra Headers)
                                </label>
                                <button
                                  type="button"
                                  className="text-2xs text-primary hover:underline"
                                  onClick={() => {
                                    const headers = { ...(prov.extraHeaders || {}) };
                                    headers[`X-Header-${Object.keys(headers).length + 1}`] = "";
                                    handleUpdateProviderField(index, "extraHeaders", headers);
                                  }}
                                >
                                  + 添加
                                </button>
                              </div>
                              {Object.keys(prov.extraHeaders || {}).length > 0 && (
                                <div className="space-y-1.5">
                                  {Object.entries(prov.extraHeaders || {}).map(([headerKey, headerValue], hIdx) => (
                                    <div key={hIdx} className="flex items-center gap-1.5">
                                      <input
                                        value={headerKey}
                                        onChange={(e) => {
                                          const entries = Object.entries(prov.extraHeaders || {});
                                          entries[hIdx] = [e.target.value, entries[hIdx][1]];
                                          handleUpdateProviderField(index, "extraHeaders", Object.fromEntries(entries));
                                        }}
                                        placeholder="Header name"
                                        className="flex-1 h-7 text-xs font-mono rounded-md border border-input bg-background px-2 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                                      />
                                      <input
                                        value={headerValue}
                                        onChange={(e) => {
                                          const updated = { ...(prov.extraHeaders || {}) };
                                          updated[headerKey] = e.target.value;
                                          handleUpdateProviderField(index, "extraHeaders", updated);
                                        }}
                                        placeholder="Value"
                                        className="flex-1 h-7 text-xs font-mono rounded-md border border-input bg-background px-2 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                                      />
                                      <button
                                        type="button"
                                        className="text-xs text-destructive hover:underline px-1"
                                        onClick={() => {
                                          const updated = { ...(prov.extraHeaders || {}) };
                                          delete updated[headerKey];
                                          handleUpdateProviderField(index, "extraHeaders", updated);
                                        }}
                                      >
                                        ✕
                                      </button>
                                    </div>
                                  ))}
                                </div>
                              )}
                              <p className="text-2xs text-muted-foreground">
                                部分服务商要求携带产品标识头 (如 HTTP-Referer、X-Title、User-Agent)，不要在此放 API Key
                              </p>
                            </div>

                            {/* Relay non-streaming compatibility. Probe-written;
                                shown so a wrong verdict can be corrected. */}
                            <div className="space-y-1 pt-1">
                              <label
                                className="text-2xs font-medium text-muted-foreground"
                                htmlFor={`provider-${index}-relay-chat-compat`}
                              >
                                中转站非流式兼容 (Non-streaming Compatibility)
                              </label>
                              <select
                                id={`provider-${index}-relay-chat-compat`}
                                value={prov.relayChatCompat || "auto"}
                                onChange={(e) =>
                                  handleUpdateProviderField(
                                    index,
                                    "relayChatCompat",
                                    e.target.value as RelayChatCompat
                                  )
                                }
                                className="w-full h-8 text-xs rounded-md border border-input bg-background px-2.5 py-1 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                              >
                                <option value="auto">自动 (按探测结果，推荐)</option>
                                <option value="buffered">强制流式缓冲 (中转站返回不规范时)</option>
                                <option value="direct">强制直连非流式 (不做任何转换)</option>
                              </select>
                              <p className="text-2xs text-muted-foreground">
                                {RELAY_CHAT_COMPAT_HINTS[prov.relayChatCompat || "auto"]}
                              </p>
                            </div>


                            {/* Claude Code Aliases & Thinking Section */}
                            <div className="p-3.5 rounded-lg border bg-muted/10 space-y-3">
                              <div className="flex items-center gap-2">
                                <Sparkles className="h-4 w-4 text-purple-500" />
                                <div>
                                  <div className="text-xs font-semibold flex items-center gap-1.5">
                                    Claude Code 别名映射与思考深度 (Model Aliases & Reasoning)
                                  </div>
                                  <div className="text-2xs text-muted-foreground">
                                    为 Claude Code 的 opus/sonnet/haiku 别名指定目标模型与显示名，并配置思考深度及 1M 长上下文
                                  </div>
                                </div>
                              </div>

                              {/* Multi-tier Aliases: Opus / Sonnet / Haiku.
                                  Three columns only from `lg`: at `sm` each tier
                                  input got ~200px and its label wrapped. */}
                              <div className="grid grid-cols-1 lg:grid-cols-3 gap-3">
                                {ALIAS_TIERS.map((tier) => (
                                  <div key={tier.field} className="space-y-1">
                                    <label
                                      className="text-2xs font-medium text-muted-foreground"
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
                                <div className="text-2xs font-medium text-muted-foreground pt-1.5">
                                  Claude Code 显示名 (Display Names)
                                </div>
                                <div className="text-2xs text-muted-foreground">
                                  写入 <code className="font-mono">~/.claude.json</code> 的模型名。Claude
                                  Code 只对认识的名字启用对应的上下文长度与计费，裸别名 opus/sonnet/haiku
                                  在 <code className="font-mono">/model</code> 选择器、
                                  <code className="font-mono">--model</code> 参数和 subagent
                                  frontmatter 里解析不一致。留空使用默认的最新模型名。仅网关开启时生效。
                                </div>
                                <div className="grid grid-cols-1 lg:grid-cols-3 gap-3">
                                  {ALIAS_TIERS.map((tier) => (
                                    <div key={tier.displayField} className="space-y-1">
                                      <label
                                        className="text-2xs font-medium text-muted-foreground"
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
                                  <label className="text-2xs font-medium text-muted-foreground">思考深度等级 (Thinking Effort Level)</label>
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
                                  <p className="text-2xs text-muted-foreground">
                                    仅在下方「思考块签名」为“支持”时才会实际注入。
                                  </p>
                                </div>

                                <div className="space-y-1">
                                  <label className="text-2xs font-medium text-muted-foreground">思考块签名 (Thinking Signature)</label>
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
                                    <p className="text-2xs text-muted-foreground">
                                      {thinkingProbeStates[index]?.message}
                                    </p>
                                  )}
                                </div>

                                <div className="space-y-1">
                                  <label className="text-2xs font-medium text-muted-foreground">1M 长上下文支持 (1M Context)</label>
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
                                  <Gauge className="h-4 w-4 text-info" />
                                  <div>
                                    <div className="text-xs font-semibold flex items-center gap-1.5">
                                      请求速率与 Token 限流 (RPM / TPM)
                                    </div>
                                    <div className="text-2xs text-muted-foreground">
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
                                    className="h-6 text-2xs px-2 text-info border-info/25 hover:bg-info-surface"
                                    title="通过上游响应头探测或智能算法计算推荐限流阈值"
                                  >
                                    <Sparkles className={`h-3 w-3 mr-1 ${rateLimitProbeStates[index]?.loading ? "animate-spin" : "text-warning"}`} />
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
                                  className={`p-2 rounded text-2xs flex items-center gap-2 ${
                                    rateLimitProbeStates[index].success
                                      ? "bg-info-surface text-info border border-info/25"
                                      : "bg-warning-surface text-warning border border-warning/25"
                                  }`}
                                >
                                  {rateLimitProbeStates[index].success ? (
                                    <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-info" />
                                  ) : (
                                    <AlertCircle className="h-3.5 w-3.5 shrink-0 text-warning" />
                                  )}
                                  <span className="flex-1 leading-tight">{rateLimitProbeStates[index].message}</span>
                                </div>
                              )}

                              {/* RPM and TPM Inputs */}
                              <div className={`grid grid-cols-1 sm:grid-cols-2 gap-3 transition-opacity ${prov.rateLimit?.enabled ? "opacity-100" : "opacity-60"}`}>
                                <div className="space-y-1">
                                  <div className="flex items-center justify-between">
                                    <label className="text-2xs font-medium text-muted-foreground flex items-center gap-1">
                                      <Timer className="h-3 w-3 text-info" />
                                      每分钟请求数 (RPM)
                                    </label>
                                    <span className="text-2xs text-muted-foreground font-mono">Requests/min</span>
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
                                    <label className="text-2xs font-medium text-muted-foreground flex items-center gap-1">
                                      <Gauge className="h-3 w-3 text-info" />
                                      每分钟 Token 数 (TPM)
                                    </label>
                                    <span className="text-2xs text-muted-foreground font-mono">Tokens/min</span>
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
                                  <span className="text-2xs text-muted-foreground">
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
                              <div className="text-2xs font-mono text-muted-foreground truncate">
                                ID: {client.id}
                              </div>
                            </div>
                          </div>

                          {client.installed ? (
                            <Badge variant="success" className="text-2xs px-1.5 py-0 shrink-0">
                              已安装
                            </Badge>
                          ) : (
                            <Badge variant="outline" className="text-2xs px-1.5 py-0 text-muted-foreground shrink-0">
                              未检测到
                            </Badge>
                          )}
                        </label>
                      );
                    })}
                  </div>
                </div>
              )}

              {editTab === "params" && (() => {
                const primary =
                  editProviders.find((p) => p.isPrimary) || editProviders[0];
                const resolved = resolveClaudeCodeParams(primary, editParams);
                const outOfRange = (v: number | null | undefined) =>
                  v != null && (v < CLAUDE_CODE_TOKEN_MIN || v > CLAUDE_CODE_TOKEN_MAX);
                const parseTokens = (raw: string): number | null => {
                  const t = raw.trim();
                  if (!t) return null;
                  const n = Number(t);
                  return Number.isFinite(n) ? Math.trunc(n) : null;
                };
                return (
                  <div className="p-6 space-y-5">
                    <div className="text-2xs text-muted-foreground leading-relaxed">
                      参数按当前模型能力自动预填充。你可以手动修改，改过之后不会被自动探测覆盖。
                      留空即表示跟随自动检测。
                    </div>

                    <div className="flex items-center gap-2 text-xs">
                      <span className="text-muted-foreground">模型能力</span>
                      {resolved.thinkingSupported ? (
                        <span className="px-2 py-0.5 rounded-full text-2xs bg-success-surface text-success">
                          ✅ 支持思考
                        </span>
                      ) : (
                        <span className="px-2 py-0.5 rounded-full text-2xs bg-muted text-muted-foreground">
                          ❌ 无思考能力
                        </span>
                      )}
                      {primary?.defaultModel && (
                        <span className="font-mono text-2xs text-muted-foreground">
                          {primary.defaultModel}
                        </span>
                      )}
                    </div>
                    {!resolved.thinkingSupported && primary?.thinkingSupport === "unsigned" && (
                      <div className="text-2xs text-warning leading-relaxed">
                        该上游返回的思考块没有签名，客户端无法保存这一轮对话。按不支持处理，
                        否则整个会话都会失败。
                      </div>
                    )}
                    {primary && (primary.thinkingSupport ?? "unprobed") === "unprobed" && (
                      <div className="text-2xs text-muted-foreground leading-relaxed">
                        尚未探测思考能力。到「Provider 节点」里对该节点做一次思考能力检测，
                        这里才会给出针对性的推荐值。
                      </div>
                    )}

                    <div className="space-y-1.5">
                      <label className="text-xs font-medium">最大输出 Token</label>
                      <div className="flex gap-2">
                        <input
                          type="number"
                          className="flex-1 px-2.5 py-1.5 text-xs rounded-md border bg-background"
                          placeholder={`自动（${resolved.maxOutputTokens}）`}
                          value={editParams.maxOutputTokens ?? ""}
                          onChange={(e) =>
                            setEditParams((p) => ({
                              ...p,
                              maxOutputTokens: parseTokens(e.target.value),
                            }))
                          }
                        />
                        <select
                          className="px-2 py-1.5 text-xs rounded-md border bg-background"
                          value=""
                          onChange={(e) => {
                            const v = e.target.value;
                            if (!v) return;
                            setEditParams((p) => ({ ...p, maxOutputTokens: Number(v) }));
                          }}
                        >
                          <option value="">预设…</option>
                          <option value="8192">8192</option>
                          <option value="32768">32768</option>
                          <option value="65536">65536</option>
                          <option value="131072">131072</option>
                          <option value="262144">262144</option>
                        </select>
                      </div>
                      {outOfRange(editParams.maxOutputTokens) && (
                        <div className="text-2xs text-destructive">
                          需在 {CLAUDE_CODE_TOKEN_MIN} ~ {CLAUDE_CODE_TOKEN_MAX} 之间
                        </div>
                      )}
                    </div>

                    <div className="space-y-1.5">
                      <label
                        className={`text-xs font-medium ${
                          resolved.thinkingSupported ? "" : "text-muted-foreground"
                        }`}
                      >
                        最大思考 Token
                      </label>
                      <input
                        type="number"
                        disabled={!resolved.thinkingSupported}
                        className="w-full px-2.5 py-1.5 text-xs rounded-md border bg-background disabled:opacity-50 disabled:cursor-not-allowed"
                        placeholder={
                          resolved.thinkingSupported
                            ? `自动（${resolved.maxThinkingTokens}）`
                            : ""
                        }
                        value={
                          resolved.thinkingSupported ? (editParams.maxThinkingTokens ?? "") : ""
                        }
                        onChange={(e) =>
                          setEditParams((p) => ({
                            ...p,
                            maxThinkingTokens: parseTokens(e.target.value),
                          }))
                        }
                      />
                      {!resolved.thinkingSupported ? (
                        <div className="text-2xs text-muted-foreground">
                          当前模型不支持深度思考，该参数无效
                        </div>
                      ) : (
                        outOfRange(editParams.maxThinkingTokens) && (
                          <div className="text-2xs text-destructive">
                            需在 {CLAUDE_CODE_TOKEN_MIN} ~ {CLAUDE_CODE_TOKEN_MAX} 之间
                          </div>
                        )
                      )}
                    </div>

                    <label className="flex items-center gap-2 text-xs cursor-pointer">
                      <input
                        type="checkbox"
                        checked={editParams.disableAutoupdater}
                        onChange={(e) =>
                          setEditParams((p) => ({
                            ...p,
                            disableAutoupdater: e.target.checked,
                          }))
                        }
                      />
                      <span>禁用自动更新</span>
                    </label>

                    <div className="space-y-1.5">
                      <label className="text-xs font-medium text-muted-foreground">
                        ANTHROPIC_BASE_URL
                      </label>
                      <div className="px-2.5 py-1.5 text-xs rounded-md border bg-muted/40 font-mono text-muted-foreground">
                        {editGatewayEnabled
                          ? "http://127.0.0.1:18888"
                          : (primary?.baseUrl || "—")}
                      </div>
                      <div className="text-2xs text-muted-foreground">
                        由网关开关决定，切换档案时自动写入，这里只做回显。
                      </div>
                    </div>

                    {(() => {
                      // The file belongs to whichever profile Claude Code currently
                      // follows. Comparing this tab's settings against it only means
                      // something when that profile is this one; otherwise every row
                      // would "differ" for the entirely normal reason that the file
                      // describes somebody else.
                      const claudeBinding = bindingFor("claude-code");
                      const boundHere = claudeBinding?.profileId === editingProfile?.id;
                      const expected: Record<string, string | null> = boundHere
                        ? {
                            CLAUDE_CODE_MAX_OUTPUT_TOKENS: String(resolved.maxOutputTokens),
                            MAX_THINKING_TOKENS:
                              resolved.maxThinkingTokens != null
                                ? String(resolved.maxThinkingTokens)
                                : null,
                            DISABLE_AUTOUPDATER: editParams.disableAutoupdater ? "1" : null,
                            // Only in gateway mode is the written value known exactly.
                            // Pointed at a provider, the backend strips a `/v1` suffix
                            // first, and duplicating that here would flag a difference
                            // that is not one.
                            ...(editGatewayEnabled
                              ? { ANTHROPIC_BASE_URL: "http://127.0.0.1:18888" }
                              : {}),
                          }
                        : {};
                      const onDisk = new Map(
                        (envPreview.data?.entries ?? []).map((e) => [e.key, e])
                      );
                      // A key this tab would remove but the file still has counts as a
                      // difference: that is the stale-leftover case worth surfacing.
                      const strayKeys = Object.entries(expected)
                        .filter(([key, want]) => want === null && onDisk.has(key))
                        .map(([key]) => key);
                      const differs = (key: string, value: string) =>
                        key in expected && expected[key] !== null && expected[key] !== value;

                      return (
                        <div className="space-y-1.5 pt-1 border-t">
                          <div className="flex items-center justify-between gap-2">
                            <label className="text-xs font-medium text-muted-foreground">
                              当前生效的 env（读自 settings.json）
                            </label>
                            <Button
                              type="button"
                              variant="ghost"
                              size="sm"
                              onClick={loadEnvPreview}
                              disabled={envPreview.loading}
                              className="h-6 px-2 text-2xs"
                            >
                              <RotateCw
                                className={`h-3 w-3 mr-1 ${
                                  envPreview.loading ? "animate-spin" : ""
                                }`}
                              />
                              重新读取
                            </Button>
                          </div>

                          {envPreview.error ? (
                            <div className="px-2.5 py-1.5 text-2xs rounded-md border border-destructive/40 bg-destructive/5 text-destructive">
                              读取失败：{envPreview.error}
                            </div>
                          ) : envPreview.loading && !envPreview.data ? (
                            <div className="px-2.5 py-1.5 text-2xs rounded-md border bg-muted/40 text-muted-foreground">
                              读取中…
                            </div>
                          ) : !envPreview.data ? null : !envPreview.data.exists ? (
                            <div className="px-2.5 py-1.5 text-2xs rounded-md border bg-muted/40 text-muted-foreground">
                              Claude Code 还没有配置文件。激活本方案时会创建它。
                            </div>
                          ) : envPreview.data.entries.length === 0 ? (
                            <div className="px-2.5 py-1.5 text-2xs rounded-md border bg-muted/40 text-muted-foreground">
                              配置文件存在，但 env 是空的。激活本方案后这里才会有值。
                            </div>
                          ) : (
                            <div className="rounded-md border bg-muted/40 divide-y max-h-56 overflow-y-auto">
                              {envPreview.data.entries.map((entry) => {
                                const mismatch = differs(entry.key, entry.value);
                                return (
                                  <div
                                    key={entry.key}
                                    className="flex items-start gap-2 px-2.5 py-1 text-2xs font-mono"
                                  >
                                    <span
                                      className={`shrink-0 ${
                                        mismatch ? "text-warning" : "text-muted-foreground"
                                      }`}
                                    >
                                      {entry.key}
                                    </span>
                                    <span
                                      className={`ml-auto text-right break-all ${
                                        entry.masked
                                          ? "text-muted-foreground italic"
                                          : mismatch
                                            ? "text-warning"
                                            : ""
                                      }`}
                                    >
                                      {entry.value}
                                      {mismatch && (
                                        <span className="not-italic"> ≠ {expected[entry.key]}</span>
                                      )}
                                    </span>
                                  </div>
                                );
                              })}
                            </div>
                          )}

                          {strayKeys.length > 0 && (
                            <div className="text-2xs text-warning leading-relaxed">
                              {strayKeys.join("、")} 还留在文件里，但按本页设置不该写入。
                              重新激活本方案会清掉它。
                            </div>
                          )}

                          <div className="text-2xs text-muted-foreground leading-relaxed">
                            {boundHere ? (
                              <>
                                Claude Code 当前跟随本方案。改完参数要按「保存并激活」才会写进文件。
                              </>
                            ) : claudeBinding ? (
                              <>
                                这个文件属于 Claude Code 当前跟随的方案「
                                {claudeBinding.profileName || claudeBinding.profileId}
                                」，与本页设置不同是正常的，因此不做对照。
                              </>
                            ) : (
                              <>
                                Claude Code 还没有绑定任何方案，文件里是上一次绑定留下的内容，因此不做对照。
                              </>
                            )}
                            {envPreview.data?.path && (
                              <> 路径：<span className="font-mono">{envPreview.data.path}</span></>
                            )}
                          </div>
                        </div>
                      );
                    })()}
                  </div>
                );
              })()}
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