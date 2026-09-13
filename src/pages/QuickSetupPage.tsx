import { useState, useEffect, useMemo } from "react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { backend } from "@/services/backend";
import type { DetectedClient } from "@/domain/client";
import type {
  ModelInfo,
  ProviderConfig,
  ProtocolKind,
  CodexToolCompat,
  RelayChatCompat,
  ChatTestResult,
} from "@/domain/profile";
import {
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

import {
  type PresetProvider,
  CORE_CLIENT_IDS,
  codexNeedsGateway,
  getSmartClients,
  PRESETS,
  PROTOCOLS,
} from "./quick-setup/presets";

/**
 * One numbered stage of the wizard.
 *
 * The three stages had their header markup written out three times, which is
 * why the step badge and title drifted apart in size between them.
 */
function StepCard({
  step,
  title,
  hint,
  aside,
  children,
}: {
  step: number;
  title: string;
  hint?: string;
  aside?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <Card>
      <CardHeader className="gap-3 p-5 pb-0 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex min-w-0 items-start gap-3">
          <div className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-primary text-xs font-bold text-primary-foreground">
            {step}
          </div>
          <div className="min-w-0 space-y-1">
            <CardTitle className="text-base">{title}</CardTitle>
            {hint && <p className="text-xs leading-relaxed text-muted-foreground">{hint}</p>}
          </div>
        </div>
        {aside && <div className="shrink-0">{aside}</div>}
      </CardHeader>
      <CardContent className="space-y-5 p-5">{children}</CardContent>
    </Card>
  );
}

/**
 * A labelled band inside a step.
 *
 * Deliberately not another bordered box: step 1 nested five `rounded-xl border
 * bg-muted/20` panels inside a bordered card, so every group carried the same
 * visual weight as the card containing it and nothing read as subordinate. A
 * label plus a rule gives the grouping without a fourth level of frame.
 */
function Field({
  label,
  hint,
  action,
  children,
  className,
}: {
  label: string;
  hint?: string;
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={"space-y-2 border-t pt-4 first:border-t-0 first:pt-0 " + (className ?? "")}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            {label}
          </h3>
          {hint && <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{hint}</p>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

/** An inline status line. Tone maps to a token trio, never a raw palette entry. */
function Notice({
  tone,
  icon: Icon,
  children,
  className,
}: {
  tone: "success" | "warning" | "info" | "danger" | "neutral";
  icon: typeof AlertCircle;
  children: React.ReactNode;
  className?: string;
}) {
  const tones = {
    success: "border-success/25 bg-success-surface text-success-foreground",
    warning: "border-warning/25 bg-warning-surface text-warning-foreground",
    info: "border-info/25 bg-info-surface text-info-foreground",
    danger: "border-destructive/30 bg-destructive-surface text-destructive",
    neutral: "border-border bg-muted/50 text-muted-foreground",
  } as const;
  return (
    <div
      className={
        "flex items-start gap-2 rounded-md border px-2.5 py-2 text-xs " +
        tones[tone] +
        (className ? " " + className : "")
      }
    >
      <Icon className="mt-0.5 h-3.5 w-3.5 shrink-0" />
      <span className="leading-snug">{children}</span>
    </div>
  );
}

/** Shared frame for the selectable tiles: presets, routes, models, protocols. */
function tileClass(active: boolean) {
  return (
    "rounded-lg border p-2.5 text-left transition-colors " +
    (active
      ? "border-primary bg-accent ring-1 ring-primary"
      : "border-border bg-card hover:border-input hover:bg-accent/50")
  );
}

export default function QuickSetupPage() {
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [baseUrl, setBaseUrl] = useState("https://api.example.com/v1");
  const [model, setModel] = useState("gpt-4o");
  const [profileName, setProfileName] = useState("自定义方案");
  const [detectedClients, setDetectedClients] = useState<DetectedClient[]>([]);
  // Filled by ad_get_version; no baked-in number to drift from Cargo.toml.
  const [version, setVersion] = useState("");
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
  // Probed, not chosen: the relay's non-streaming Chat Completions verdict.
  const [relayChatCompat, setRelayChatCompat] = useState<RelayChatCompat>("auto");
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
        if (probeRes.relayChatCompat && probeRes.relayChatCompat !== "auto") {
          setRelayChatCompat(probeRes.relayChatCompat);
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
      if (probeRes.relayChatCompat && probeRes.relayChatCompat !== "auto") {
        setRelayChatCompat(probeRes.relayChatCompat);
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
          relayChatCompat,
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
          // No client list: bind the ones this wizard just selected, which is what
          // the profile's own `clients` now holds.
          await backend.activateProfile(created.id);
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


  return (
    <div className="mx-auto max-w-5xl space-y-5 pb-12">
      {/* Hero Title */}
      <div className="space-y-2">
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant="info">
            <Zap className="mr-1 h-3 w-3" />
            快速向导 v{version}
          </Badge>
          <span className="text-xs text-muted-foreground">三步完成 AI 开发客户端统一代理接入</span>
        </div>
        <h1 className="text-2xl font-bold tracking-tight">快速配置 PolyDeck</h1>
        <p className="max-w-3xl text-sm leading-relaxed text-muted-foreground">
          PolyDeck 通过智能协议感知与本地网关代理 OpenAI、Anthropic 及各类推理模型，提供智能协议转换、多客户端配置分发与凭据安全托管。
        </p>
      </div>

      {/* Step 1: Provider Setup */}
      <StepCard
        step={1}
        title="配置大模型服务商与 API Key"
        hint="先挑模板或 Agnes 线路，再核对协议与端点，最后验证连通性。"
        aside={
          detectedProviderType && (
            <Badge variant="info">
              <Sparkles className="mr-1 h-3 w-3" />
              识别到: {detectedProviderType}
            </Badge>
          )
        }
      >
        {/* Quick presets */}
        <Field label="常用服务商模板">
          <div className="flex flex-wrap gap-2">
            {PRESETS.map((p) => (
              <Button
                key={p.name}
                type="button"
                variant="outline"
                size="xs"
                onClick={() => handleSelectPreset(p)}
                className="hover:border-primary"
              >
                <Server className="h-3 w-3 text-muted-foreground" />
                {p.name}
              </Button>
            ))}
          </div>
        </Field>

        {/* Agnes dedicated panel */}
        <Field label="Agnes AI 专区" className="border-t pt-4">
          <div
            className={
              "space-y-3 rounded-lg border p-3 transition-colors " +
              (agnesRouteId ? "border-primary/50 bg-accent/60" : "border-border bg-muted/30")
            }
            data-testid="agnes-panel"
          >
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0 space-y-1">
                <div className="flex flex-wrap items-center gap-2">
                  <Boxes className="h-4 w-4 shrink-0 text-primary" />
                  <span className="text-sm font-semibold">一键接入 Claude Code 与 Codex</span>
                  <Badge variant="success">Flash 现价免费</Badge>
                  <Badge variant="outline">三协议齐备</Badge>
                </div>
                <p className="text-xs leading-relaxed text-muted-foreground">
                  无需 CC-Switch 或 Codex++。选择线路后网关自动启用并映射 Claude 三档模型。
                </p>
              </div>
              <a
                href={AGNES_CONSOLE_URL}
                target="_blank"
                rel="noreferrer noopener"
                className="shrink-0 whitespace-nowrap rounded text-xs text-primary hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                获取 API Key ↗
              </a>
            </div>

            {/* Route choice */}
            <div className="space-y-2">
              <label className="block text-xs text-muted-foreground">
                接入线路（按你的网络环境选择）
              </label>
              <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                {AGNES_ROUTES.map((route) => {
                  const isActive = agnesRouteId === route.id;
                  return (
                    <button
                      key={route.id}
                      type="button"
                      onClick={() => handleSelectAgnesRoute(route)}
                      aria-pressed={isActive}
                      data-testid={"agnes-route-" + route.id}
                      className={tileClass(isActive)}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="text-sm font-semibold">{route.label}</span>
                        {isActive && <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-primary" />}
                      </div>
                      <div className="mt-0.5 text-xs text-muted-foreground">{route.hint}</div>
                      <div className="mt-0.5 truncate font-mono text-2xs text-muted-foreground">
                        {route.baseUrl}
                      </div>
                    </button>
                  );
                })}
              </div>
              <Notice tone="warning" icon={AlertCircle}>
                {AGNES_ROUTE_KEY_SCOPE_NOTE}
              </Notice>
            </div>

            {/* Model choice, only once a route is armed */}
            {agnesRouteId && (
              <div className="animate-fade-in space-y-2">
                <label className="block text-xs text-muted-foreground">模型</label>
                <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                  {AGNES_MODELS.map((m) => {
                    const isActive = agnesModel === m.id;
                    return (
                      <button
                        key={m.id}
                        type="button"
                        onClick={() => handleSelectAgnesModel(m.id)}
                        aria-pressed={isActive}
                        data-testid={"agnes-model-" + m.id}
                        className={tileClass(isActive)}
                      >
                        <div className="flex flex-wrap items-center gap-1.5">
                          <span className="text-sm font-medium">{m.label}</span>
                          <Badge variant={m.free ? "success" : "outline"}>
                            {m.free ? "免费" : "计费"}
                          </Badge>
                          {isActive && <CheckCircle2 className="h-3 w-3 shrink-0 text-primary" />}
                        </div>
                        <div className="mt-0.5 text-xs leading-snug text-muted-foreground">
                          {m.note}
                        </div>
                        <div className="mt-0.5 font-mono text-2xs text-muted-foreground">
                          {m.id}
                        </div>
                      </button>
                    );
                  })}
                </div>

                {!AGNES_MODELS.find((m) => m.id === agnesModel)?.free && (
                  <Notice tone="warning" icon={AlertCircle}>
                    {AGNES_PRO_BUDGET_WARNING}
                  </Notice>
                )}

                <Notice tone="neutral" icon={Radio}>
                  限流已设为 {AGNES_FREE_TIER_RPM} RPM（免费档实测上限）。Agnes 不返回限流响应头，自动探测会给出偏高的 60，此处不采用。
                </Notice>
              </div>
            )}
          </div>
        </Field>

        {/* Manual Protocol Selector */}
        <Field
          label="服务协议"
          hint="可手动切换，或由下方连通性测试自动探测。"
          action={
            <Badge variant="outline" className="font-mono uppercase">
              <Network className="mr-1 h-3 w-3" />
              当前协议: {currentProtocol}
            </Badge>
          }
        >
          {/* Five columns left ~195px per cell at this page width, too narrow for
              a protocol name plus two lines of description. */}
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
            {PROTOCOLS.map((proto) => {
              const isActive = currentProtocol === proto.id;
              return (
                <button
                  key={proto.id}
                  type="button"
                  onClick={() => handleSelectProtocol(proto.id)}
                  className={tileClass(isActive) + " flex flex-col justify-between gap-1"}
                >
                  <div className="flex w-full items-center justify-between gap-2">
                    <span className="text-sm font-semibold">{proto.name}</span>
                    {isActive && <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-primary" />}
                  </div>
                  <span className="text-xs leading-snug text-muted-foreground">{proto.desc}</span>
                </button>
              );
            })}
          </div>
        </Field>

        <Field label="端点参数">
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <div>
              <label className="mb-1 flex items-center gap-1 text-xs font-medium text-muted-foreground">
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
              <div className="mb-1 flex items-center justify-between gap-2">
                <label className="flex items-center gap-1 text-xs font-medium text-muted-foreground">
                  <Radio className="h-3.5 w-3.5" /> 默认模型标识 (Default Model)
                </label>
                <Button
                  type="button"
                  variant="outline"
                  size="xs"
                  onClick={handleFetchModels}
                  disabled={fetchingModels || !baseUrl.trim()}
                  className="border-primary/40 text-primary hover:border-primary hover:bg-accent"
                >
                  {fetchingModels ? (
                    <>
                      <RotateCw className="h-3 w-3 animate-spin" />
                      正在获取...
                    </>
                  ) : (
                    <>
                      <ListFilter className="h-3 w-3" />
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
                    <label className="flex items-center gap-1 text-xs text-muted-foreground">
                      <Sparkles className="h-3 w-3 text-info" />
                      从已获取的模型列表中快速选择：
                    </label>
                    <select
                      aria-label="从已获取的模型列表中选择"
                      value={selectableModels.some((m) => m.id === model) ? model : ""}
                      onChange={(e) => handleSelectModelDropdown(e.target.value)}
                      className="w-full rounded-md border border-input bg-card px-2.5 py-1.5 font-mono text-xs text-foreground shadow-sm focus:outline-none focus:ring-1 focus:ring-ring"
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
                      <p className="text-xs leading-snug text-muted-foreground">
                        已隐藏 {availableModels.length - selectableModels.length} 个非对话模型(图像/视频),它们走独立端点,不能作为对话模型使用。
                      </p>
                    )}
                  </div>
                )}

                {/* Fetch status/fallback alert */}
                {fetchMessage && (
                  <Notice
                    tone={fetchSuccess ? "success" : "warning"}
                    icon={fetchSuccess ? CheckCircle2 : AlertCircle}
                  >
                    {fetchMessage}
                  </Notice>
                )}
              </div>
            </div>
          </div>

          <div>
            <label className="mb-1 flex items-center gap-1 text-xs font-medium text-muted-foreground">
              <Key className="h-3.5 w-3.5" /> API Key / 访问凭据 (存储于操作系统加密安全区)
            </label>
            <div className="relative flex items-center">
              <Input
                type={showApiKey ? "text" : "password"}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-..."
                className="pr-8 font-mono text-xs"
                data-testid="quicksetup-api-key-input"
              />
              <button
                type="button"
                onClick={() => setShowApiKey(!showApiKey)}
                className="absolute right-2 rounded p-0.5 text-muted-foreground transition-colors hover:text-foreground focus:outline-none"
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
        </Field>

        <Field label="连通性验证">
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={handleTestConnection}
              disabled={testing || !baseUrl.trim()}
              className="text-xs"
            >
              {testing ? (
                <>
                  <RotateCw className="mr-1 h-3.5 w-3.5 animate-spin" />
                  正在测试连通性...
                </>
              ) : (
                <>
                  <Radio className="mr-1 h-3.5 w-3.5" />
                  测试连接
                </>
              )}
            </Button>

            <Button
              variant="outline"
              size="sm"
              onClick={handleTestChat}
              disabled={testingChat || !baseUrl.trim()}
              className="border-primary/40 text-xs text-primary hover:bg-accent"
            >
              {testingChat ? (
                <>
                  <RotateCw className="mr-1 h-3.5 w-3.5 animate-spin" />
                  正在进行真实对话...
                </>
              ) : (
                <>
                  <MessageSquare className="mr-1 h-3.5 w-3.5" />
                  真实对话测试
                </>
              )}
            </Button>
          </div>

          {testResult && (
            <Notice
              tone={testResult.success ? "success" : "danger"}
              icon={testResult.success ? CheckCircle2 : AlertCircle}
            >
              {testResult.message}
              {testResult.latency !== undefined && (
                <span className="ml-1 font-mono opacity-80">({testResult.latency}ms)</span>
              )}
            </Notice>
          )}

          {/* Real Chat Test Result Feedback */}
          {chatResult && (
            <div
              className={
                "animate-fade-in space-y-2 rounded-lg border p-3 text-xs " +
                (chatResult.success
                  ? "border-success/25 bg-success-surface"
                  : "border-destructive/30 bg-destructive-surface text-destructive")
              }
            >
              <div className="flex items-center justify-between gap-2 font-medium">
                <div className="flex items-center gap-2">
                  {chatResult.success ? (
                    <CheckCircle2 className="h-4 w-4 shrink-0 text-success" />
                  ) : (
                    <AlertCircle className="h-4 w-4 shrink-0 text-destructive" />
                  )}
                  <span className={chatResult.success ? "font-semibold text-success-foreground" : "font-semibold"}>
                    {chatResult.success
                      ? ("真实对话测试成功 (耗时: " + (("latencyMs" in chatResult) ? chatResult.latencyMs : 0) + "ms)")
                      : "真实对话测试失败"}
                  </span>
                </div>
                {"model" in chatResult && chatResult.model && (
                  <Badge variant="outline" className="font-mono">
                    模型: {chatResult.model}
                  </Badge>
                )}
              </div>

              {"reply" in chatResult && chatResult.reply && (
                <div className="select-text whitespace-pre-wrap rounded-md border bg-card p-3 font-mono text-xs leading-relaxed text-foreground">
                  <div className="mb-1 flex items-center gap-1 font-sans text-xs font-medium text-muted-foreground">
                    <MessageSquare className="h-3 w-3 text-info" />
                    模型回复内容：
                  </div>
                  {chatResult.reply}
                </div>
              )}

              {"message" in chatResult && !chatResult.success && (
                <div className="text-xs leading-relaxed">{chatResult.message}</div>
              )}
            </div>
          )}
        </Field>
      </StepCard>


      {/* Step 2: Smart Gateway & Clients Selection */}
      <StepCard
        step={2}
        title="选网关模式与要同步的客户端"
        aside={
          <Badge variant={gatewayEnabled ? "success" : "secondary"}>
            <Cpu className="mr-1 h-3 w-3" />
            {gatewayEnabled ? "已启用本地网关" : "原生直连模式"}
          </Badge>
        }
      >
        {/* Gateway toggle and explanation */}
        <Field label="本地网关">
          <div className="space-y-2 rounded-lg border bg-muted/30 p-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="enable-gateway"
                  checked={gatewayEnabled}
                  onChange={(e) => setGatewayEnabled(e.target.checked)}
                  className="h-4 w-4 rounded border-input text-primary focus:ring-primary"
                />
                <label htmlFor="enable-gateway" className="cursor-pointer text-sm font-semibold">
                  启用本地代理网关 (127.0.0.1:18888)
                </label>
              </div>
              <Badge variant={codexNeedsGateway(codexCompat) ? "destructive" : "outline"}>
                {codexNeedsGateway(codexCompat) ? "Codex 必须开启桥接" : "推荐开启"}
              </Badge>
            </div>
            <p className="pl-6 text-xs leading-relaxed text-muted-foreground">
              {gatewayReason}
            </p>

            {/*
              Turning the gateway off is legitimate when Codex is not a target,
              so this warns rather than blocks — but it names the exact failure
              instead of letting the user discover it as a 400 mid-session.
            */}
            {!gatewayEnabled && codexNeedsGateway(codexCompat) && selectedClients.includes("codex-cli") && (
              <div
                className="ml-6 flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive-surface px-2.5 py-2 text-xs text-destructive"
                data-testid="codex-needs-gateway-warning"
              >
                <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                <span className="leading-snug">
                  网关已关闭,但 Codex CLI 在同步列表中。该上游会拒绝 Codex 的 <code className="font-mono">custom</code> 类型工具(如 <code className="font-mono">apply_patch</code>),Codex 第一轮就会返回{" "}
                  <code className="font-mono">400 unknown variant `custom`</code>。要么开启网关,要么在下方取消勾选 Codex CLI。
                </span>
              </div>
            )}
          </div>
        </Field>

        {/* Target clients checkboxes (Smart selected) */}
        <Field
          label="自动同步配置的客户端"
          action={
            <div className="flex flex-wrap items-center gap-1">
              <Button
                type="button"
                variant="ghost"
                size="xs"
                onClick={handleSelectAllCoreClients}
                className="text-primary hover:bg-accent"
              >
                <CheckSquare className="h-3 w-3" />
                全选主流 (Codex/Claude/Hermes)
              </Button>
              <Button type="button" variant="ghost" size="xs" onClick={handleSelectAllClients}>
                全选所有
              </Button>
              <Button type="button" variant="ghost" size="xs" onClick={handleSelectSmartClients}>
                智能推荐
              </Button>
              <Button type="button" variant="ghost" size="xs" onClick={handleClearClients}>
                <Square className="h-3 w-3" />
                清空
              </Button>
            </div>
          }
        >
          <div className="flex items-center gap-1 text-xs text-muted-foreground">
            <Layers className="h-3.5 w-3.5" />
            激活方案时，PolyDeck 会为勾选的客户端写入端点与令牌。
          </div>

          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
            {detectedClients.map((c) => {
              const isSelected = selectedClients.includes(c.id);
              const isCore = CORE_CLIENT_IDS.includes(c.id);
              return (
                <label
                  key={c.id}
                  className={
                    "flex cursor-pointer items-center justify-between gap-2 rounded-lg border p-2.5 text-sm transition-colors " +
                    (isSelected
                      ? "border-primary bg-accent font-medium"
                      : "border-border bg-card text-muted-foreground hover:border-input hover:bg-accent/50")
                  }
                >
                  <div className="flex min-w-0 items-center gap-2">
                    <input
                      type="checkbox"
                      checked={isSelected}
                      onChange={() => handleToggleClient(c.id)}
                      className="h-3.5 w-3.5 rounded border-input text-primary focus:ring-primary"
                    />
                    <span className="truncate">{c.name}</span>
                    {isCore && (
                      <Badge variant="outline" className="shrink-0 border-primary/40 text-primary">
                        主流
                      </Badge>
                    )}
                  </div>
                  <Badge variant={c.installed ? "success" : "outline"} className="shrink-0">
                    {c.installed ? "已检测到" : "未安装"}
                  </Badge>
                </label>
              );
            })}
          </div>
        </Field>
      </StepCard>

      {/* Step 3: Save Profile & Gateway */}
      <StepCard
        step={3}
        title="保存方案并激活"
        hint="仅保存会写入方案但不改动客户端；激活才会分发配置。"
      >
        <Field label="方案">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-end">
            <div className="flex-1">
              <label className="mb-1 block text-xs font-medium text-muted-foreground">方案名称</label>
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
                    <Check className="mr-1.5 h-3.5 w-3.5 text-success" />
                    方案已保存
                  </>
                ) : (
                  <>
                    <Save className="mr-1.5 h-3.5 w-3.5" />
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
                    <Check className="mr-1.5 h-3.5 w-3.5 text-success" />
                    方案已激活 & 配置已分发
                  </>
                ) : (
                  <>
                    <ArrowRight className="mr-1.5 h-3.5 w-3.5" />
                    保存并立即激活
                  </>
                )}
              </Button>
            </div>
          </div>
        </Field>

        {gatewayEnabled && (
          <Field label="接入地址">
            <div className="flex flex-wrap items-center justify-between gap-2 rounded-lg border bg-muted/30 p-3">
              <div className="min-w-0 space-y-0.5">
                <div className="text-sm font-semibold">本地网关代理接入地址</div>
                <div className="truncate font-mono text-xs text-muted-foreground">
                  http://127.0.0.1:18888/v1
                </div>
              </div>
              <Button variant="outline" size="xs" onClick={copyProxyUrl}>
                {copied ? <Check className="h-3 w-3 text-success" /> : <Copy className="h-3 w-3" />}
                {copied ? "已复制" : "复制地址"}
              </Button>
            </div>
          </Field>
        )}
      </StepCard>
    </div>
  );
}
