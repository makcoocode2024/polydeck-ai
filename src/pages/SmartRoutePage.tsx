import { useEffect, useState } from "react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { backend } from "@/services/backend";
import {
  defaultSmartRouteSettings,
  type SmartRouteSettings,
} from "@/domain/smartRoute";
import {
  AliasEditor,
  CategoryEditor,
  RulesEditor,
  ImportExport,
  Simulator,
  AuditPanel,
} from "./smart-route/SmartRouteCards";
import { CheckCircle2, XCircle } from "lucide-react";

/** All models of all providers of all profiles, for the dropdowns. */
function useKnownModels(): string[] {
  const [models, setModels] = useState<string[]>([]);
  useEffect(() => {
    backend
      .listProfiles()
      .then((profiles) => {
        const all = new Set<string>();
        for (const p of profiles) {
          for (const prov of p.providers) {
            for (const m of prov.models || []) {
              if (m) all.add(m);
            }
          }
        }
        setModels([...all].sort());
      })
      .catch(() => setModels([]));
  }, []);
  return models;
}

export default function SmartRoutePage() {
  const [settings, setSettings] = useState<SmartRouteSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveStatus, setSaveStatus] = useState<"idle" | "saved">("idle");
  const [warnings, setWarnings] = useState<string[]>([]);
  const knownModels = useKnownModels();

  useEffect(() => {
    backend
      .getRouteConfig()
      .then((config) => {
        setSettings(config);
        setError(null);
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, []);

  if (error) {
    return (
      <div className="max-w-3xl mx-auto space-y-4" data-testid="smart-route-page">
        <h1 className="text-xl font-bold">智能路由</h1>
        <Card data-testid="route-error-card">
          <CardContent className="flex items-start gap-2 text-sm text-destructive">
            <XCircle className="h-4 w-4 mt-0.5 shrink-0" />
            <span>读取配置失败：{error}</span>
          </CardContent>
        </Card>
      </div>
    );
  }

  if (!settings) {
    return (
      <div className="max-w-3xl mx-auto space-y-4" data-testid="smart-route-page">
        <h1 className="text-xl font-bold">智能路由</h1>
        <p className="text-sm text-muted-foreground">读取配置中…</p>
      </div>
    );
  }

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    setSaveStatus("idle");
    try {
      const result = await backend.updateRouteConfig(settings);
      setSettings(result.config);
      setWarnings(result.warnings);
      setSaveStatus("saved");
      setTimeout(() => setSaveStatus("idle"), 2000);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="max-w-3xl mx-auto space-y-4" data-testid="smart-route-page">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">智能路由</h1>
        <Badge variant={settings.enableRoute ? "success" : "secondary"}>
          {settings.enableRoute ? "已启用" : "未启用"}
        </Badge>
      </div>

      <Card data-testid="route-global-card">
        <CardHeader className="pb-3">
          <CardTitle className="text-sm">全局配置</CardTitle>
          <p className="text-2xs text-muted-foreground">
            判定顺序：强制模型 → 别名替换 → 方案级重写 → #MODEL: 标签 → 自定义规则 → 分类 →
            默认模型 → 已接入模型校验。对 /v1/messages、/v1/responses、/v1/chat/completions
            生效；count_tokens 与 MCP 不做判定。保存后立即热更新，无需重启网关。
          </p>
        </CardHeader>
        <CardContent className="space-y-2.5">
          <label className="flex items-start gap-3 rounded-lg border bg-muted/30 p-3 cursor-pointer">
            <input
              type="checkbox"
              checked={settings.enableRoute}
              data-testid="route-enable-toggle"
              onChange={(e) => setSettings({ ...settings, enableRoute: e.target.checked })}
              className="mt-0.5 h-4 w-4 rounded border-input text-primary"
            />
            <div className="flex-1 space-y-0.5">
              <div className="text-xs font-medium">启用智能路由</div>
              <p className="text-2xs text-muted-foreground">
                关闭后完全走原有方案级重写路径，零行为变化。
              </p>
            </div>
          </label>
          <div className="grid grid-cols-2 gap-2">
            <div className="space-y-1">
              <label className="text-2xs font-medium text-muted-foreground">强制模型（force_model）</label>
              <input
                value={settings.forceModel ?? ""}
                list="model-options-global"
                placeholder="留空不启用"
                data-testid="route-force-model-input"
                onChange={(e) =>
                  setSettings({ ...settings, forceModel: e.target.value || null })
                }
                className="w-full h-8 text-xs font-mono rounded-md border border-input bg-background px-2 text-foreground"
              />
            </div>
            <div className="space-y-1">
              <label className="text-2xs font-medium text-muted-foreground">默认兜底模型（default_model）</label>
              <input
                value={settings.defaultModel ?? ""}
                list="model-options-global"
                placeholder="规则全部未命中时使用"
                data-testid="route-default-model-input"
                onChange={(e) =>
                  setSettings({ ...settings, defaultModel: e.target.value || null })
                }
                className="w-full h-8 text-xs font-mono rounded-md border border-input bg-background px-2 text-foreground"
              />
            </div>
          </div>
          <label className="flex items-start gap-3 rounded-lg border bg-muted/30 p-3 cursor-pointer">
            <input
              type="checkbox"
              checked={settings.fallbackEnable}
              data-testid="route-fallback-toggle"
              onChange={(e) => setSettings({ ...settings, fallbackEnable: e.target.checked })}
              className="mt-0.5 h-4 w-4 rounded border-input text-primary"
            />
            <div className="flex-1 space-y-0.5">
              <div className="text-xs font-medium">模型不可用自动回退</div>
              <p className="text-2xs text-muted-foreground">
                仅当上游返回「模型不存在」类 4xx 错误时，用默认模型重发一次；5xx、限流与网络错误不回退。
              </p>
            </div>
          </label>
          <datalist id="model-options-global">
            {knownModels.map((m) => (
              <option key={m} value={m} />
            ))}
          </datalist>
          <div className="flex items-center gap-3">
            <Button
              size="xs"
              disabled={saving}
              data-testid="route-save-button"
              onClick={handleSave}
            >
              {saving ? "保存中…" : "保存配置"}
            </Button>
            {saveStatus === "saved" && (
              <span
                className="flex items-center gap-1 text-2xs text-success"
                data-testid="route-save-status"
              >
                <CheckCircle2 className="h-3.5 w-3.5" /> 已保存并热更新
              </span>
            )}
            {error && (
              <span className="text-2xs text-destructive" data-testid="route-error">
                {error}
              </span>
            )}
          </div>
          {warnings.length > 0 && (
            <div className="rounded-lg border border-warning/30 bg-warning-surface p-2 space-y-0.5">
              {warnings.map((w) => (
                <p key={w} className="text-2xs text-warning">
                  {w}
                </p>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      <AliasEditor
        value={settings.modelAliasMap}
        knownModels={knownModels}
        onChange={(modelAliasMap) => setSettings({ ...settings, modelAliasMap })}
      />

      <CategoryEditor
        value={settings.categoryRouteMap}
        knownModels={knownModels}
        onChange={(categoryRouteMap) => setSettings({ ...settings, categoryRouteMap })}
      />

      <RulesEditor
        value={settings.customRules}
        knownModels={knownModels}
        onChange={(customRules) => setSettings({ ...settings, customRules })}
      />

      <Simulator
        knownModels={knownModels}
        simulate={(model, prompt, effort) => backend.simulateRoute(model, prompt, effort)}
      />

      <ImportExport
        settings={settings}
        onImport={(parsed) => setSettings({ ...defaultSmartRouteSettings(), ...parsed })}
      />

      <AuditPanel load={(requestId) => backend.getRouteAudit(requestId, 100)} />
    </div>
  );
}
