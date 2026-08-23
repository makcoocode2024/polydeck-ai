import { useState, useEffect, useMemo } from "react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { backend } from "@/services/backend";
import type { DetectedClient } from "@/domain/client";
import type { ModelInfo, ProviderConfig, ProtocolKind, CodexToolCompat, ChatTestResult } from "@/domain/profile";
import {
  AGNES_BASE_URL_CN,
  AGNES_BASE_URL_GLOBAL,
  AGNES_CONSOLE_URL,
  AGNES_DEFAULT_MODEL,
  AGNES_FREE_TIER_RPM,
  AGNES_MODELS,
  AGNES_MODEL_IDS,
  AGNES_PRO_BUDGET_WARNING,
  AGNES_ROUTE_KEY_SCOPE_NOTE,
  AGNES_ROUTES,
  type AgnesRoute,
} from "@/domain/agnes";
import {
  Zap,
  CheckCircle2,
  AlertCircle,
  Sparkles,
  Server,
  Key,
  Globe,
  Radio,
  ArrowRight,
  Copy,
  Check,
  RotateCw,
  ListFilter,
  Save,
  Cpu,
  Layers,
  MessageSquare,
  Network,
  CheckSquare,
  Square,
  Eye,
  EyeOff,
  Boxes,
} from "lucide-react";

interface PresetProvider {
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

const PRESETS: PresetProvider[] = [
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

﻿
const PROTOCOLS: { id: ProtocolKind; name: string; desc: string; defaultModel: string; defaultUrl: string }[] = [
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

const CORE_CLIENT_IDS = ["codex-cli", "claude-code", "claude-desktop", "hermes"];

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
function codexNeedsGateway(compat: CodexToolCompat): boolean {
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
function getSmartClients(
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

export default function QuickSetupPage() {
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [baseUrl, setBaseUrl] = useState("https://api.example.com/v1");
  const [model, setModel] = useState("gpt-4o");
  const [profileName, setProfileName] = useState("自定义方案");
  const [detectedClients, setDetectedClients] = useState<DetectedClient[]>([]);
  const [version, setVersion] = useState("2.0.0");
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ success: boolean; message: string; latency?: number } | null>(null);
  const [testingChat, setTestingChat] = useState(false);
  const [chatResult, setChatResult] = useState<ChatTestResult | { success: false; message: string; latencyMs?: number } | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveStatus, setSaveStatus] = useState<"idle" | "saved" | "activated">("idle");
  const [copied, setCopied] = useState(false);

  // Model probing and gateway smart detection
  const [fetchingModels, setFetchingModels] = useState(false);
  const [availableModels, setAvailableModels] = useState<ModelInfo[]>([]);
  const [fetchMessage, setFetchMessage] = useState<string | null>(null);
  const [fetchSuccess, setFetchSuccess] = useState<boolean | null>(null);

  const [currentProtocol, setCurrentProtocol] = useState<ProtocolKind>("openai");
  const [codexCompat, setCodexCompat] = useState<CodexToolCompat>("responses_custom");
  const [gatewayEnabled, setGatewayEnabled] = useState<boolean>(true);
  const [gatewayReason, setGatewayReason] = useState<string>(
    "本地网关提供统一端口转发、协议转译、多客户端配置分发与凭据托管"
  );
  const [selectedClients, setSelectedClients] = useState<string[]>(CORE_CLIENT_IDS);

  // Agnes panel: which route is armed, if any. Null means the user has not
  // picked Agnes, so the panel stays collapsed and nothing below is touched.
  const [agnesRouteId, setAgnesRouteId] = useState<AgnesRoute["id"] | null>(null);
  const [agnesModel, setAgnesModel] = useState<string>(AGNES_DEFAULT_MODEL);

  /**
   * Detect clients once. This used to depend on `currentProtocol` and re-derive
   * `selectedClients` on every change, which silently discarded the user's
   * choice whenever a probe adjusted the protocol — an Agnes profile would come
   * out with `claude-code` dropped and its tier slots therefore inert.
   *
   * The selection now changes only on an explicit action: picking a preset or an
   * Agnes route, or using the selection buttons.
   */
  useEffect(() => {
    backend.getVersion().then(setVersion).catch(() => {});
    backend.detectClients().then((clients) => {
      setDetectedClients(clients);
      setSelectedClients(getSmartClients("openai", clients, true));
    }).catch(() => {});
  }, []);

  /**
   * Probed models the user can actually pick as a chat model. For an armed Agnes
   * route this drops the image and video entries `/v1/models` also returns,
   * which answer on other endpoints and would 400 as a chat model.
   */
  const selectableModels = useMemo(() => {
    if (!agnesRouteId) return availableModels;
    return availableModels.filter((m) => AGNES_MODEL_IDS.includes(m.id));
  }, [availableModels, agnesRouteId]);

  const detectedProviderType = useMemo(() => {
    const key = apiKey.trim();
    if (!key) return null;
    if (key.startsWith("sk-ant-")) return "Anthropic";
    if (key.startsWith("sk-or-")) return "OpenRouter";
    if (key.startsWith("xai-")) return "xAI Grok";
    if (key.startsWith("AIzaSy")) return "Google Gemini";
    if (key.startsWith("gsk_")) return "Groq";
    if (key.startsWith("sk-proj-") || key.startsWith("sk-")) return "OpenAI / Compatible";
    return "Custom API Key";
  }, [apiKey]);

  const handleSelectPreset = (preset: PresetProvider) => {
    // The two Agnes entries route through the dedicated panel so the button row
    // and the panel cannot disagree about which host is armed.
    const agnesRoute = AGNES_ROUTES.find((r) => r.baseUrl === preset.baseUrl);
    if (agnesRoute) {
      handleSelectAgnesRoute(agnesRoute, preset.defaultModel);
      return;
    }
    setAgnesRouteId(null);
    setBaseUrl(preset.baseUrl);
    setModel(preset.defaultModel);
    setProfileName(preset.name + " 方案");
    setCurrentProtocol(preset.protocol);
    if (preset.codexCompat) {
      setCodexCompat(preset.codexCompat);
    }
    setTestResult(null);
    setChatResult(null);
    setSaveStatus("idle");
    setAvailableModels([]);
    setFetchMessage(null);
    setFetchSuccess(null);
    setSelectedClients(getSmartClients(preset.protocol, detectedClients, gatewayEnabled));
  };

  /**
   * Arm one of the two Agnes routes.
   *
   * Protocol is pinned to `openai` rather than `responses`. Agnes does serve
   * `/v1/responses`, but only Chat Completions accepts tool types beyond plain
   * `function`, and `openai` is what leaves the gateway free to bridge. The
   * probe will offer to upgrade this to `responses` if the user runs it; that
   * stays safe because the gateway forces the bridge for non-native tool types
   * before consulting the protocol at all.
   */
  const handleSelectAgnesRoute = (route: AgnesRoute, model: string = agnesModel) => {
    setAgnesRouteId(route.id);
    setAgnesModel(model);
    setBaseUrl(route.baseUrl);
    setModel(model);
    setProfileName("Agnes " + route.label + " 方案");
    setCurrentProtocol("openai");
    setCodexCompat("chat_function");
    setGatewayEnabled(true);
    setGatewayReason(
      "Agnes 仅在 Chat Completions 上提供完整工具调用，Codex 的自定义工具需要本地网关桥接；Claude Code 经网关映射 Claude 名到 Agnes 模型（已启用）"
    );
    setTestResult(null);
    setChatResult(null);
    setSaveStatus("idle");
    setAvailableModels([]);
    setFetchMessage(null);
    setFetchSuccess(null);
    // Gateway is switched on just above, so the Claude clients are reachable.
    setSelectedClients(getSmartClients("openai", detectedClients, true));
  };

  const handleSelectAgnesModel = (modelId: string) => {
    setAgnesModel(modelId);
    setModel(modelId);
    setSaveStatus("idle");
  };

  const handleSelectProtocol = (proto: ProtocolKind) => {
    setCurrentProtocol(proto);
    const matched = PROTOCOLS.find((p) => p.id === proto);
    if (matched) {
      if (
        baseUrl === "https://api.example.com/v1" ||
        baseUrl === "https://api.openai.com/v1" ||
        baseUrl === "https://api.anthropic.com/v1" ||
        baseUrl === "https://generativelanguage.googleapis.com/v1beta" ||
        baseUrl === "https://your-resource.openai.azure.com"
      ) {
        setBaseUrl(matched.defaultUrl);
      }
      if (
        model === "gpt-4o" ||
        model === "claude-3-7-sonnet-20250219" ||
        model === "gemini-2.5-pro"
      ) {
        setModel(matched.defaultModel);
      }
    }
  };

  const handleSelectAllCoreClients = () => {
    setSelectedClients(CORE_CLIENT_IDS);
  };

  const handleSelectAllClients = () => {
    const allIds = Array.from(new Set([...CORE_CLIENT_IDS, ...detectedClients.map((c) => c.id)]));
    setSelectedClients(allIds);
  };

  const handleSelectSmartClients = () => {
    setSelectedClients(getSmartClients(currentProtocol, detectedClients, gatewayEnabled));
  };

  const handleClearClients = () => {
    setSelectedClients([]);
  };

﻿
  const handleFetchModels = async () => {
    if (!baseUrl.trim()) {
      setFetchSuccess(false);
      setFetchMessage("请先填写 API 基础地址 (Base URL)");
      return;
    }

    setFetchingModels(true);
    setFetchMessage(null);
    setFetchSuccess(null);

    try {
      const probeRes = await backend.probeProvider(baseUrl.trim(), apiKey.trim());
      if (probeRes) {
        if (probeRes.protocol && probeRes.protocol !== "unknown") {
          // Deliberately does not re-derive `selectedClients`. Probing reports
          // what the upstream speaks; it is not a signal that the user's client
          // choice should be thrown away. Use the 智能推荐 button for that.
          setCurrentProtocol(probeRes.protocol);
        }
        if (probeRes.codexCompat && probeRes.codexCompat !== "unknown") {
          setCodexCompat(probeRes.codexCompat);
        }

        // Smart gateway decision
        if (probeRes.codexCompat === "chat_function") {
          setGatewayEnabled(true);
          setGatewayReason("上游仅支持 Chat Completions 协议，Codex 需要本地网关进行 Responses 协议桥接（已自动开启）");
        } else if (probeRes.codexCompat === "responses_function") {
          // Serves Responses but refused a custom tool while probing, so Codex
          // cannot reach it directly — `apply_patch` is a custom tool.
          setGatewayEnabled(true);
          setGatewayReason(
            "上游支持 Responses，但拒绝 Codex 的 custom 类型工具（如 apply_patch）；网关会将这类请求桥接到 Chat Completions（已自动开启）"
          );
        } else if (probeRes.codexCompat === "responses_custom") {
          setGatewayReason("上游原生支持 Responses 与 Chat 协议，含 custom 工具，支持客户端原生直连或网关代理转发");
        }

        if (Array.isArray(probeRes.models) && probeRes.models.length > 0) {
          setAvailableModels(probeRes.models);
          setFetchSuccess(true);
          setFetchMessage("成功获取到 " + probeRes.models.length + " 个可用模型，请在下方下拉菜单选择或继续编辑。");
        } else {
          setAvailableModels([]);
          setFetchSuccess(true);
          setFetchMessage("未能自动获取到模型列表，请在上方编辑框中手动填写模型标识（如 gpt-4o, claude-3-7-sonnet 等）。");
        }
      }
    } catch (err) {
      setAvailableModels([]);
      setFetchSuccess(false);
      const errDetail = err instanceof Error ? err.message : String(err);
      setFetchMessage("获取模型失败（" + errDetail + "），请在上方编辑框中手动填写模型标识。");
    } finally {
      setFetchingModels(false);
    }
  };

  const handleSelectModelDropdown = (selectedId: string) => {
    if (selectedId) {
      setModel(selectedId);
    }
  };

  const handleTestConnection = async () => {
    if (!baseUrl.trim()) {
      setTestResult({
        success: false,
        message: "请先填写 API 基础地址 (Base URL)",
      });
      return;
    }
    setTesting(true);
    setTestResult(null);
    const start = performance.now();
    try {
      const probeRes = await backend.probeProvider(baseUrl.trim(), apiKey.trim());
      const latency = Math.round(performance.now() - start);

      if (!probeRes || probeRes.protocol === "unknown") {
        setTestResult({
          success: false,
          latency,
          message: "连接与鉴权测试失败：无法识别有效的大模型服务协议或 API Key 无效",
        });
        return;
      }

      setCurrentProtocol(probeRes.protocol);
      if (probeRes.codexCompat && probeRes.codexCompat !== "unknown") {
        setCodexCompat(probeRes.codexCompat);
      }

      if (codexNeedsGateway(probeRes.codexCompat)) {
        setGatewayEnabled(true);
        setGatewayReason(
          probeRes.codexCompat === "chat_function"
            ? "上游仅支持 Chat Completions，Codex 必须通过网关桥接（已自动启用）"
            : "上游支持 Responses，但拒绝 Codex 的 custom 类型工具（如 apply_patch）；网关会桥接这类请求（已自动启用）"
        );
      }

      if (Array.isArray(probeRes.models) && probeRes.models.length > 0) {
        setAvailableModels(probeRes.models);
      }

      const modelCount = probeRes.models?.length ?? 0;
      setTestResult({
        success: true,
        message: "服务接口与 API Key 验证正常 [" + probeRes.protocol.toUpperCase() + "]" + (modelCount > 0 ? ("，已获取 " + modelCount + " 个可用模型") : "，接口已就绪"),
        latency: Math.max(latency, 12),
      });
    } catch (err) {
      const latency = Math.round(performance.now() - start);
      setTestResult({
        success: false,
        latency,
        message: err instanceof Error ? err.message : "连接与鉴权测试失败，请检查网络地址与 API Key",
      });
    } finally {
      setTesting(false);
    }
  };

  const handleTestChat = async () => {
    if (!baseUrl.trim()) {
      setChatResult({ success: false, message: "请先填写 API 基础地址 (Base URL)" });
      return;
    }
    setTestingChat(true);
    setChatResult(null);
    const start = performance.now();
    try {
      const res = await backend.testProviderChat(
        baseUrl.trim(),
        apiKey.trim(),
        model.trim() || "gpt-4o",
        currentProtocol,
        false
      );
      setChatResult(res);
    } catch (err) {
      const latencyMs = Math.round(performance.now() - start);
      setChatResult({
        success: false,
        latencyMs,
        message: err instanceof Error ? err.message : String(err),
      });
    } finally {
      setTestingChat(false);
    }
  };

  const handleToggleClient = (clientId: string) => {
    if (selectedClients.includes(clientId)) {
      setSelectedClients(selectedClients.filter((c) => c !== clientId));
    } else {
      setSelectedClients([...selectedClients, clientId]);
    }
  };

  const handleSaveProfile = async (activate: boolean) => {
    const targetName = profileName.trim();
    if (!targetName) return;
    setSaving(true);
    try {
      const created = await backend.createProfile(targetName);
      if (created?.id) {
        const armedAgnesRoute = AGNES_ROUTES.find((r) => r.id === agnesRouteId);
        const chosenModel = model.trim() || "gpt-4o";
        // `/v1/models` is not a list of chat models. Agnes returns its image and
        // video models there too, and they only answer on /v1/images/generations
        // and /v1/videos — saving them verbatim put `agnes-video-2.5` in Codex's
        // model catalogue as a selectable chat model, where it can only fail.
        const probedIds = availableModels.map((m) => m.id);
        const usableIds = armedAgnesRoute
          ? probedIds.filter((id) => AGNES_MODEL_IDS.includes(id))
          : probedIds;
        const prov: ProviderConfig = {
          id: "prov_" + Date.now(),
          name: targetName + " 节点",
          baseUrl: baseUrl.trim(),
          protocol: currentProtocol,
          defaultModel: chosenModel,
          models: usableIds.length > 0 ? usableIds : [chosenModel],
          isPrimary: true,
          codexCompat,
          reasoningConfidence: "validated",
          acceptInvalidCerts: false,
          maxPricePerRequest: null,
          // Agnes exposes no RateLimit-* headers and its free tier 429s at 20
          // RPM, so the generic 60 would sit three times over the real ceiling.
          rateLimit: armedAgnesRoute
            ? { enabled: true, rpm: AGNES_FREE_TIER_RPM, tpm: 100000, adaptive: true }
            : { enabled: true, rpm: 60, tpm: 100000, adaptive: true },
          // Claude Code's /model picker only lists ids starting with `claude-`,
          // so pointing all three tiers at the Agnes model is what makes it
          // reachable there. Display names stay unset so profile_switch writes
          // its built-in Anthropic ids, which is what gives Claude Code a real
          // context window and price.
          ...(armedAgnesRoute
            ? {
                opusModel: chosenModel,
                sonnetModel: chosenModel,
                haikuModel: chosenModel,
              }
            : {}),
        };

        if (apiKey.trim()) {
          await backend.setProfileApiKey(created.id, apiKey.trim()).catch(() => {});
        }

        await backend.updateProfile(created.id, {
          name: targetName,
          gatewayEnabled,
          failoverEnabled: false,
          providers: [prov],
          clients: selectedClients,
        });

        if (activate) {
          await backend.switchProfile(created.id);
          if (gatewayEnabled) {
            try {
              await backend.gatewayStart();
            } catch {
              // already running
            }
          }
          setSaveStatus("activated");
        } else {
          setSaveStatus("saved");
        }
      }
    } catch (err) {
      alert("保存失败: " + (err instanceof Error ? err.message : String(err)));
    } finally {
      setSaving(false);
    }
  };

  const copyProxyUrl = () => {
    navigator.clipboard.writeText("http://127.0.0.1:18888/v1");
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

﻿
  return (
    <div className="max-w-4xl mx-auto space-y-8 pb-12">
      {/* Hero Title */}
      <div className="space-y-2">
        <div className="flex items-center gap-2">
          <Badge variant="info" className="px-3 py-1">
            <Zap className="h-3 w-3 mr-1" />
            快速向导 v{version}
          </Badge>
          <span className="text-xs text-muted-foreground">三步完成 AI 开发客户端统一代理接入</span>
        </div>
        <h1 className="text-3xl font-extrabold tracking-tight">快速配置 PolyDeck</h1>
        <p className="text-muted-foreground text-sm leading-relaxed">
          PolyDeck 通过智能协议感知与本地网关代理 OpenAI、Anthropic 及各类推理模型，提供智能协议转换、多客户端配置分发与凭据安全托管。
        </p>
      </div>

      {/* Step 1: Provider Setup */}
      <Card className="border-border/60 shadow-sm">
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <div className="h-6 w-6 rounded-full bg-primary text-primary-foreground text-xs flex items-center justify-center font-bold">
                1
              </div>
              <CardTitle className="text-lg">配置大模型服务商与 API Key</CardTitle>
            </div>
            {detectedProviderType && (
              <Badge variant="info">
                <Sparkles className="h-3 w-3 mr-1 text-sky-400" />
                识别到: {detectedProviderType}
              </Badge>
            )}
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* Quick presets */}
          <div>
            <label className="text-xs text-muted-foreground mb-1.5 block">常用服务商快捷模板</label>
            <div className="flex flex-wrap gap-2">
              {PRESETS.map((p) => (
                <Button
                  key={p.name}
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => handleSelectPreset(p)}
                  className="text-xs h-8 hover:border-primary transition-all"
                >
                  <Server className="h-3 w-3 mr-1 text-muted-foreground" />
                  {p.name}
                </Button>
              ))}
            </div>
          </div>

          {/* Agnes dedicated panel */}
          <div
            className={
              "rounded-xl border p-3.5 space-y-3 transition-all " +
              (agnesRouteId
                ? "border-primary/60 bg-primary/5 ring-1 ring-primary/30"
                : "border-border bg-muted/10")
            }
            data-testid="agnes-panel"
          >
            <div className="flex items-start justify-between gap-3 flex-wrap">
              <div className="space-y-1 min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  <Boxes className="h-4 w-4 text-primary shrink-0" />
                  <span className="text-sm font-semibold">Agnes AI 专区</span>
                  <Badge variant="success" className="text-[10px] px-1.5 py-0">
                    Flash 现价免费
                  </Badge>
                  <Badge variant="outline" className="text-[10px] px-1.5 py-0">
                    三协议齐备
                  </Badge>
                </div>
                <p className="text-[11px] text-muted-foreground leading-relaxed">
                  一键完成 Claude Code 与 Codex 接入，无需 CC-Switch 或 Codex++。选择线路后网关自动启用并映射 Claude 三档模型。
                </p>
              </div>
              <a
                href={AGNES_CONSOLE_URL}
                target="_blank"
                rel="noreferrer noopener"
                className="text-[11px] text-primary hover:underline whitespace-nowrap shrink-0 focus:outline-none focus-visible:ring-1 focus-visible:ring-ring rounded"
              >
                获取 API Key ↗
              </a>
            </div>

            {/* Route choice */}
            <div>
              <label className="text-[11px] text-muted-foreground mb-1.5 block">
                接入线路（按你的网络环境选择）
              </label>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                {AGNES_ROUTES.map((route) => {
                  const isActive = agnesRouteId === route.id;
                  return (
                    <button
                      key={route.id}
                      type="button"
                      onClick={() => handleSelectAgnesRoute(route)}
                      aria-pressed={isActive}
                      data-testid={"agnes-route-" + route.id}
                      className={
                        "p-2.5 rounded-lg border text-left transition-all " +
                        (isActive
                          ? "border-primary bg-primary/10 ring-1 ring-primary"
                          : "border-border bg-card/60 hover:border-border/80 hover:bg-muted/30")
                      }
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="text-xs font-semibold">{route.label}</span>
                        {isActive && <CheckCircle2 className="h-3.5 w-3.5 text-primary shrink-0" />}
                      </div>
                      <div className="text-[10px] text-muted-foreground mt-0.5">{route.hint}</div>
                      <div className="text-[10px] font-mono text-muted-foreground/80 truncate mt-0.5">
                        {route.baseUrl}
                      </div>
                    </button>
                  );
                })}
              </div>
              <div className="text-[11px] flex items-start gap-1.5 px-2.5 py-1.5 mt-2 rounded-md border bg-amber-500/10 border-amber-500/20 text-amber-600 dark:text-amber-400">
                <AlertCircle className="h-3.5 w-3.5 mt-0.5 shrink-0" />
                <span className="leading-snug">{AGNES_ROUTE_KEY_SCOPE_NOTE}</span>
              </div>
            </div>

            {/* Model choice, only once a route is armed */}
            {agnesRouteId && (
              <div className="space-y-2 animate-in fade-in duration-200">
                <label className="text-[11px] text-muted-foreground block">模型</label>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                  {AGNES_MODELS.map((m) => {
                    const isActive = agnesModel === m.id;
                    return (
                      <button
                        key={m.id}
                        type="button"
                        onClick={() => handleSelectAgnesModel(m.id)}
                        aria-pressed={isActive}
                        data-testid={"agnes-model-" + m.id}
                        className={
                          "p-2 rounded-lg border text-left transition-all " +
                          (isActive
                            ? "border-primary bg-primary/10 ring-1 ring-primary"
                            : "border-border bg-card/60 hover:border-border/80 hover:bg-muted/30")
                        }
                      >
                        <div className="flex items-center gap-1.5 flex-wrap">
                          <span className="text-xs font-medium">{m.label}</span>
                          {m.free ? (
                            <Badge variant="success" className="text-[9px] px-1 py-0">
                              免费
                            </Badge>
                          ) : (
                            <Badge variant="outline" className="text-[9px] px-1 py-0">
                              计费
                            </Badge>
                          )}
                          {isActive && <CheckCircle2 className="h-3 w-3 text-primary shrink-0" />}
                        </div>
                        <div className="text-[10px] text-muted-foreground mt-0.5 leading-tight">
                          {m.note}
                        </div>
                        <div className="text-[10px] font-mono text-muted-foreground/70 mt-0.5">
                          {m.id}
                        </div>
                      </button>
                    );
                  })}
                </div>

                {!AGNES_MODELS.find((m) => m.id === agnesModel)?.free && (
                  <div className="text-[11px] flex items-start gap-1.5 px-2.5 py-1.5 rounded-md border bg-amber-500/10 border-amber-500/20 text-amber-600 dark:text-amber-400">
                    <AlertCircle className="h-3.5 w-3.5 mt-0.5 shrink-0" />
                    <span className="leading-snug">{AGNES_PRO_BUDGET_WARNING}</span>
                  </div>
                )}

                <div className="text-[11px] text-muted-foreground flex items-start gap-1.5 px-2.5 py-1.5 rounded-md bg-muted/40 border border-border/60">
                  <Radio className="h-3.5 w-3.5 mt-0.5 shrink-0 text-muted-foreground" />
                  <span className="leading-snug">
                    限流已设为 {AGNES_FREE_TIER_RPM} RPM（免费档实测上限）。Agnes 不返回限流响应头，自动探测会给出偏高的 60，此处不采用。
                  </span>
                </div>
              </div>
            )}
          </div>

          {/* Manual Protocol Selector */}
          <div className="p-3 rounded-xl border bg-muted/20 space-y-2">
            <div className="flex items-center justify-between">
              <label className="text-xs font-medium text-foreground flex items-center gap-1.5">
                <Network className="h-3.5 w-3.5 text-primary" />
                服务协议类型 (可手动切换或由下方测试自动探测)
              </label>
              <Badge variant="outline" className="text-[10px] uppercase font-mono">
                当前协议: {currentProtocol}
              </Badge>
            </div>
            <div className="grid grid-cols-2 sm:grid-cols-5 gap-2">
              {PROTOCOLS.map((proto) => {
                const isActive = currentProtocol === proto.id;
                return (
                  <button
                    key={proto.id}
                    type="button"
                    onClick={() => handleSelectProtocol(proto.id)}
                    className={"p-2.5 rounded-lg border text-left transition-all flex flex-col justify-between gap-1 " + (isActive ? "border-primary bg-primary/10 text-foreground ring-1 ring-primary shadow-xs" : "border-border bg-card/60 text-muted-foreground hover:border-border/80 hover:bg-muted/30")}
                  >
                    <div className="flex items-center justify-between w-full">
                      <span className="text-xs font-semibold">{proto.name}</span>
                      {isActive && <CheckCircle2 className="h-3.5 w-3.5 text-primary shrink-0" />}
                    </div>
                    <span className="text-[10px] leading-tight text-muted-foreground line-clamp-2">
                      {proto.desc}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <label className="text-xs font-medium text-muted-foreground flex items-center gap-1 mb-1">
                <Globe className="h-3.5 w-3.5" /> API 基础地址 (Base URL)
              </label>
              <Input
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="https://api.openai.com/v1"
                className="font-mono text-xs"
              />
            </div>
            <div>
              <div className="flex items-center justify-between mb-1">
                <label className="text-xs font-medium text-muted-foreground flex items-center gap-1">
                  <Radio className="h-3.5 w-3.5" /> 默认模型标识 (Default Model)
                </label>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={handleFetchModels}
                  disabled={fetchingModels || !baseUrl.trim()}
                  className="h-6 px-2 text-[11px] font-medium border-primary/40 text-primary hover:bg-primary/10 hover:border-primary transition-all"
                >
                  {fetchingModels ? (
                    <>
                      <RotateCw className="h-3 w-3 mr-1 animate-spin" />
                      正在获取...
                    </>
                  ) : (
                    <>
                      <ListFilter className="h-3 w-3 mr-1" />
                      获取模型
                    </>
                  )}
                </Button>
              </div>
              <div className="space-y-2">
                <Input
                  value={model}
                  onChange={(e) => setModel(e.target.value)}
                  placeholder="gpt-4o / claude-3-7-sonnet"
                  className="font-mono text-xs"
                />

                {/* Dropdown if models are fetched */}
                {selectableModels.length > 0 && (
                  <div className="space-y-1">
                    <label className="text-[11px] text-muted-foreground flex items-center gap-1">
                      <Sparkles className="h-3 w-3 text-sky-400" />
                      从已获取的模型列表中快速选择：
                    </label>
                    <select
                      aria-label="从已获取的模型列表中选择"
                      value={selectableModels.some((m) => m.id === model) ? model : ""}
                      onChange={(e) => handleSelectModelDropdown(e.target.value)}
                      className="w-full text-xs font-mono bg-background border border-input rounded-md px-2.5 py-1.5 focus:outline-none focus:ring-1 focus:ring-ring text-foreground shadow-sm"
                    >
                      <option value="" disabled>
                        -- 共 {selectableModels.length} 个可用于对话的模型，点击选择 --
                      </option>
                      {selectableModels.map((m) => (
                        <option key={m.id} value={m.id}>
                          {m.id} {m.name && m.name !== m.id ? ("(" + m.name + ")") : ""}
                        </option>
                      ))}
                    </select>
                    {availableModels.length > selectableModels.length && (
                      <p className="text-[10px] text-muted-foreground leading-snug">
                        已隐藏 {availableModels.length - selectableModels.length} 个非对话模型(图像/视频),它们走独立端点,不能作为对话模型使用。
                      </p>
                    )}
                  </div>
                )}

                {/* Fetch status/fallback alert */}
                {fetchMessage && (
                  <div
                    className={"text-xs flex items-start gap-1.5 px-2.5 py-1.5 rounded-md border " + (fetchSuccess ? "bg-emerald-500/10 border-emerald-500/20 text-emerald-600 dark:text-emerald-400" : "bg-amber-500/10 border-amber-500/20 text-amber-600 dark:text-amber-400")}
                  >
                    {fetchSuccess ? (
                      <CheckCircle2 className="h-3.5 w-3.5 mt-0.5 shrink-0" />
                    ) : (
                      <AlertCircle className="h-3.5 w-3.5 mt-0.5 shrink-0" />
                    )}
                    <span className="leading-snug">{fetchMessage}</span>
                  </div>
                )}
              </div>
            </div>
          </div>

          <div>
            <label className="text-xs font-medium text-muted-foreground flex items-center gap-1 mb-1">
              <Key className="h-3.5 w-3.5" /> API Key / 访问凭据 (存储于操作系统加密安全区)
            </label>
            <div className="relative flex items-center">
              <Input
                type={showApiKey ? "text" : "password"}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-..."
                className="font-mono text-xs pr-8"
                data-testid="quicksetup-api-key-input"
              />
              <button
                type="button"
                onClick={() => setShowApiKey(!showApiKey)}
                className="absolute right-2 text-muted-foreground hover:text-foreground transition-colors p-0.5 rounded focus:outline-none"
                title={showApiKey ? "隐藏 API Key" : "显示 API Key"}
                data-testid="quicksetup-api-key-toggle"
              >
                {showApiKey ? (
                  <EyeOff className="h-3.5 w-3.5" />
                ) : (
                  <Eye className="h-3.5 w-3.5" />
                )}
              </button>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-3 pt-2">
            <Button
              variant="outline"
              size="sm"
              onClick={handleTestConnection}
              disabled={testing || !baseUrl.trim()}
              className="text-xs"
            >
              {testing ? (
                <>
                  <RotateCw className="h-3.5 w-3.5 mr-1 animate-spin" />
                  正在测试连通性...
                </>
              ) : (
                <>
                  <Radio className="h-3.5 w-3.5 mr-1" />
                  测试连接
                </>
              )}
            </Button>

            <Button
              variant="outline"
              size="sm"
              onClick={handleTestChat}
              disabled={testingChat || !baseUrl.trim()}
              className="text-xs border-primary/40 text-primary hover:bg-primary/10"
            >
              {testingChat ? (
                <>
                  <RotateCw className="h-3.5 w-3.5 mr-1 animate-spin" />
                  正在进行真实对话...
                </>
              ) : (
                <>
                  <MessageSquare className="h-3.5 w-3.5 mr-1" />
                  真实对话测试
                </>
              )}
            </Button>

            {testResult && (
              <div
                className={"text-xs flex items-center gap-1.5 px-3 py-1.5 rounded-md " + (testResult.success ? "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400" : "bg-destructive/10 text-destructive")}
              >
                {testResult.success ? (
                  <CheckCircle2 className="h-3.5 w-3.5 shrink-0" />
                ) : (
                  <AlertCircle className="h-3.5 w-3.5 shrink-0" />
                )}
                <span>{testResult.message}</span>
                {testResult.latency !== undefined && (
                  <span className="font-mono opacity-80">({testResult.latency}ms)</span>
                )}
              </div>
            )}
          </div>

          {/* Real Chat Test Result Feedback */}
          {chatResult && (
            <div
              className={"p-3.5 rounded-xl border text-xs space-y-2 animate-in fade-in duration-200 " + (chatResult.success ? "bg-emerald-500/5 border-emerald-500/30 text-foreground" : "bg-destructive/10 border-destructive/30 text-destructive")}
            >
              <div className="flex items-center justify-between font-medium">
                <div className="flex items-center gap-2">
                  {chatResult.success ? (
                    <CheckCircle2 className="h-4 w-4 text-emerald-500 shrink-0" />
                  ) : (
                    <AlertCircle className="h-4 w-4 text-destructive shrink-0" />
                  )}
                  <span className={chatResult.success ? "text-emerald-600 dark:text-emerald-400 font-semibold" : "font-semibold"}>
                    {chatResult.success
                      ? ("真实对话测试成功 (耗时: " + (("latencyMs" in chatResult) ? chatResult.latencyMs : 0) + "ms)")
                      : "真实对话测试失败"}
                  </span>
                </div>
                {"model" in chatResult && chatResult.model && (
                  <Badge variant="outline" className="text-[10px] font-mono">
                    模型: {chatResult.model}
                  </Badge>
                )}
              </div>

              {"reply" in chatResult && chatResult.reply && (
                <div className="p-3 rounded-lg bg-background/90 border border-border/80 text-foreground text-xs leading-relaxed font-mono whitespace-pre-wrap select-text shadow-inner">
                  <div className="text-[10px] text-muted-foreground mb-1 flex items-center gap-1 font-sans font-medium">
                    <MessageSquare className="h-3 w-3 text-sky-400" />
                    模型回复内容：
                  </div>
                  {chatResult.reply}
                </div>
              )}

              {"message" in chatResult && !chatResult.success && (
                <div className="text-xs leading-relaxed opacity-95">{chatResult.message}</div>
              )}
            </div>
          )}
        </CardContent>
      </Card>

﻿
      {/* Step 2: Smart Gateway & Clients Selection */}
      <Card className="border-border/60 shadow-sm">
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <div className="h-6 w-6 rounded-full bg-primary text-primary-foreground text-xs flex items-center justify-center font-bold">
                2
              </div>
              <CardTitle className="text-lg">智能关联客户端与本地网关设置</CardTitle>
            </div>
            <Badge variant={gatewayEnabled ? "success" : "secondary"} className="text-xs">
              <Cpu className="h-3 w-3 mr-1" />
              {gatewayEnabled ? "已启用本地网关" : "原生直连模式"}
            </Badge>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* Gateway toggle and explanation */}
          <div className="p-3.5 rounded-xl border bg-muted/20 space-y-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="enable-gateway"
                  checked={gatewayEnabled}
                  onChange={(e) => setGatewayEnabled(e.target.checked)}
                  className="rounded border-input text-primary focus:ring-primary h-4 w-4"
                />
                <label htmlFor="enable-gateway" className="text-xs font-semibold cursor-pointer">
                  启用本地代理网关 (127.0.0.1:18888)
                </label>
              </div>
              <Badge
                variant={codexNeedsGateway(codexCompat) ? "destructive" : "outline"}
                className="text-[10px]"
              >
                {codexNeedsGateway(codexCompat) ? "Codex 必须开启桥接" : "推荐开启"}
              </Badge>
            </div>
            <p className="text-[11px] text-muted-foreground leading-relaxed pl-6">
              {gatewayReason}
            </p>

            {/*
              Turning the gateway off is legitimate when Codex is not a target,
              so this warns rather than blocks — but it names the exact failure
              instead of letting the user discover it as a 400 mid-session.
            */}
            {!gatewayEnabled && codexNeedsGateway(codexCompat) && selectedClients.includes("codex-cli") && (
              <div
                className="text-[11px] flex items-start gap-1.5 px-2.5 py-2 rounded-md border bg-destructive/10 border-destructive/30 text-destructive ml-6"
                data-testid="codex-needs-gateway-warning"
              >
                <AlertCircle className="h-3.5 w-3.5 mt-0.5 shrink-0" />
                <span className="leading-snug">
                  网关已关闭,但 Codex CLI 在同步列表中。该上游会拒绝 Codex 的 <code className="font-mono">custom</code> 类型工具(如 <code className="font-mono">apply_patch</code>),Codex 第一轮就会返回{" "}
                  <code className="font-mono">400 unknown variant `custom`</code>。要么开启网关,要么在下方取消勾选 Codex CLI。
                </span>
              </div>
            )}
          </div>

          {/* Target clients checkboxes (Smart selected) */}
          <div className="space-y-2.5">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <label className="text-xs font-medium text-muted-foreground flex items-center gap-1">
                <Layers className="h-3.5 w-3.5" /> 自动同步配置的 AI 客户端：
              </label>
              <div className="flex items-center gap-1.5 text-xs">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={handleSelectAllCoreClients}
                  className="h-6 px-2 text-[11px] text-primary hover:bg-primary/10"
                >
                  <CheckSquare className="h-3 w-3 mr-1" />
                  全选主流客户端 (Codex/Claude/Hermes)
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={handleSelectAllClients}
                  className="h-6 px-2 text-[11px] text-muted-foreground hover:text-foreground"
                >
                  全选所有
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={handleSelectSmartClients}
                  className="h-6 px-2 text-[11px] text-muted-foreground hover:text-foreground"
                >
                  智能推荐
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={handleClearClients}
                  className="h-6 px-2 text-[11px] text-muted-foreground hover:text-foreground"
                >
                  <Square className="h-3 w-3 mr-1" />
                  清空
                </Button>
              </div>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
              {detectedClients.map((c) => {
                const isSelected = selectedClients.includes(c.id);
                const isCore = CORE_CLIENT_IDS.includes(c.id);
                return (
                  <label
                    key={c.id}
                    className={"flex items-center justify-between p-2.5 rounded-lg border text-xs cursor-pointer transition-all " + (isSelected ? "border-primary bg-primary/5 font-medium" : "border-border bg-card/40 text-muted-foreground hover:border-border/80")}
                  >
                    <div className="flex items-center gap-2 min-w-0">
                      <input
                        type="checkbox"
                        checked={isSelected}
                        onChange={() => handleToggleClient(c.id)}
                        className="rounded border-input text-primary focus:ring-primary h-3.5 w-3.5"
                      />
                      <span className="truncate">{c.name}</span>
                      {isCore && (
                        <Badge variant="outline" className="text-[9px] px-1 py-0 border-primary/40 text-primary shrink-0">
                          主流
                        </Badge>
                      )}
                    </div>
                    {c.installed ? (
                      <Badge variant="success" className="text-[9px] px-1 py-0 shrink-0">已检测到</Badge>
                    ) : (
                      <Badge variant="outline" className="text-[9px] px-1 py-0 text-muted-foreground shrink-0">未安装</Badge>
                    )}
                  </label>
                );
              })}
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Step 3: Save Profile & Gateway */}
      <Card className="border-border/60 shadow-sm">
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <div className="h-6 w-6 rounded-full bg-primary text-primary-foreground text-xs flex items-center justify-center font-bold">
              3
            </div>
            <CardTitle className="text-lg">保存配置方案与激活</CardTitle>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col sm:flex-row gap-3">
            <div className="flex-1">
              <label className="text-xs font-medium text-muted-foreground block mb-1">方案名称</label>
              <Input
                value={profileName}
                onChange={(e) => setProfileName(e.target.value)}
                placeholder="例如: 生产开发方案"
                className="text-xs"
              />
            </div>
            <div className="flex flex-wrap items-end gap-2">
              <Button
                variant="outline"
                onClick={() => handleSaveProfile(false)}
                disabled={saving || !profileName.trim() || !baseUrl.trim()}
                className="text-xs font-medium"
              >
                {saving ? (
                  <>
                    <RotateCw className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                    保存中...
                  </>
                ) : saveStatus === "saved" ? (
                  <>
                    <Check className="h-3.5 w-3.5 mr-1.5 text-emerald-500" />
                    方案已保存
                  </>
                ) : (
                  <>
                    <Save className="h-3.5 w-3.5 mr-1.5" />
                    仅保存方案
                  </>
                )}
              </Button>

              <Button
                onClick={() => handleSaveProfile(true)}
                disabled={saving || !profileName.trim() || !baseUrl.trim()}
                className="text-xs font-medium"
              >
                {saving ? (
                  <>
                    <RotateCw className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                    处理中...
                  </>
                ) : saveStatus === "activated" ? (
                  <>
                    <Check className="h-3.5 w-3.5 mr-1.5 text-emerald-400" />
                    方案已激活 & 配置已分发
                  </>
                ) : (
                  <>
                    <ArrowRight className="h-3.5 w-3.5 mr-1.5" />
                    保存并立即激活
                  </>
                )}
              </Button>
            </div>
          </div>

          {gatewayEnabled && (
            <div className="p-3 bg-muted/40 rounded-lg border flex items-center justify-between">
              <div className="space-y-0.5">
                <div className="text-xs font-semibold">本地网关代理接入地址</div>
                <div className="text-xs text-muted-foreground font-mono">http://127.0.0.1:18888/v1</div>
              </div>
              <Button variant="outline" size="sm" onClick={copyProxyUrl} className="h-7 text-xs">
                {copied ? <Check className="h-3 w-3 mr-1 text-emerald-500" /> : <Copy className="h-3 w-3 mr-1" />}
                {copied ? "已复制" : "复制地址"}
              </Button>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
