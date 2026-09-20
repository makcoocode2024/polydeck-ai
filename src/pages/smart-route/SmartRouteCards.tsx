import { useState } from "react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Plus, Trash2, Copy, ClipboardPaste, Check } from "lucide-react";
import type { SmartRouteSettings, CustomRouteRule } from "@/domain/smartRoute";

interface AliasEditorProps {
  value: Record<string, string>;
  onChange: (next: Record<string, string>) => void;
  knownModels: string[];
}

/** Key-value editor for the global alias map, following the extra-headers
 * editor pattern on ProfilesPage: entries as rows, add via a placeholder
 * key, delete per row. */
export function AliasEditor({ value, onChange, knownModels }: AliasEditorProps) {
  const entries = Object.entries(value);
  return (
    <Card data-testid="alias-card">
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm">模型别名映射</CardTitle>
          <Button
            type="button"
            variant="outline"
            size="xs"
            data-testid="alias-add-button"
            onClick={() => onChange({ ...value, "": "" })}
          >
            <Plus className="h-3 w-3" /> 添加
          </Button>
        </div>
        <p className="text-2xs text-muted-foreground">
          键 = 客户端入站模型名（如 claude-opus-5），值 = 后端实际模型名（如 glm-5.3）。别名在方案级重写规则之前执行。
        </p>
      </CardHeader>
      <CardContent className="space-y-1.5">
        {entries.length === 0 && (
          <p className="text-2xs text-muted-foreground">暂无别名。请求模型名原样进入方案级重写。</p>
        )}
        <datalist id="alias-model-options">
          {knownModels.map((m) => (
            <option key={m} value={m} />
          ))}
        </datalist>
        {entries.map(([key, val], i) => (
          <div key={i} className="flex items-center gap-1.5">
            <input
              value={key}
              placeholder="入站模型名"
              data-testid={`alias-key-input-${i}`}
              onChange={(e) => {
                const next = { ...value };
                delete next[key];
                next[e.target.value] = val;
                onChange(next);
              }}
              className="flex-1 h-8 text-xs font-mono rounded-md border border-input bg-background px-2 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
            />
            <span className="text-muted-foreground text-xs">→</span>
            <input
              value={val}
              list="alias-model-options"
              placeholder="实际模型名"
              data-testid={`alias-value-input-${i}`}
              onChange={(e) => onChange({ ...value, [key]: e.target.value })}
              className="flex-1 h-8 text-xs font-mono rounded-md border border-input bg-background px-2 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
            />
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              className="text-destructive"
              data-testid={`alias-delete-${i}`}
              onClick={() => {
                const next = { ...value };
                delete next[key];
                onChange(next);
              }}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

interface CategoryEditorProps {
  value: SmartRouteSettings["categoryRouteMap"];
  onChange: (next: SmartRouteSettings["categoryRouteMap"]) => void;
  knownModels: string[];
}

const CATEGORY_ORDER: Array<keyof SmartRouteSettings["categoryRouteMap"]> = [
  "complexCore",
  "regularDev",
  "visualFrontend",
];

/** The three built-in categories; only the target model is configurable. */
export function CategoryEditor({ value, onChange, knownModels }: CategoryEditorProps) {
  const labels: Record<keyof SmartRouteSettings["categoryRouteMap"], string> = {
    complexCore: "复杂核心任务",
    regularDev: "常规开发任务",
    visualFrontend: "视觉前端任务",
  };
  return (
    <Card data-testid="category-card">
      <CardHeader className="pb-3">
        <CardTitle className="text-sm">分类路由</CardTitle>
        <p className="text-2xs text-muted-foreground">
          分类特征内置不可改（关键词 / effort=max / 多模态 / 超长上下文），只选择每类的目标模型。分类按 复杂核心 → 视觉前端 → 常规兜底 判定。
        </p>
      </CardHeader>
      <CardContent className="space-y-2.5">
        {CATEGORY_ORDER.map((key) => (
          <div key={key} className="flex items-center gap-3">
            <span className="w-28 shrink-0 text-xs font-medium">{labels[key]}</span>
            <select
              value={value[key] ?? ""}
              data-testid={`category-select-${key.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase())}`}
              onChange={(e) => onChange({ ...value, [key]: e.target.value || null })}
              className="flex-1 h-8 text-xs rounded-md border border-input bg-background px-2 text-foreground"
            >
              <option value="">未配置（该分类不参与路由）</option>
              {knownModels.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

interface RulesEditorProps {
  value: CustomRouteRule[];
  onChange: (next: CustomRouteRule[]) => void;
  knownModels: string[];
}

/** Custom keyword/regex rules, in order: first match wins. */
export function RulesEditor({ value, onChange, knownModels }: RulesEditorProps) {
  const nextId = () => `rule-${Date.now()}-${Math.floor(Math.random() * 1000)}`;
  const update = (id: string, patch: Partial<CustomRouteRule>) =>
    onChange(value.map((r) => (r.id === id ? { ...r, ...patch } : r)));

  return (
    <Card data-testid="rules-card">
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm">自定义路由规则</CardTitle>
          <Button
            type="button"
            variant="outline"
            size="xs"
            data-testid="rules-add-button"
            onClick={() =>
              onChange([
                ...value,
                { id: nextId(), keyword: "", targetModel: "", isRegex: false, enable: true },
              ])
            }
          >
            <Plus className="h-3 w-3" /> 添加规则
          </Button>
        </div>
        <p className="text-2xs text-muted-foreground">
          命中关键词的请求直接转发到目标模型，优先级高于分类路由（低于 #MODEL: 标签）。支持正则（灾难性回溯风险由扫描上限兜底）。
        </p>
      </CardHeader>
      <CardContent className="space-y-2">
        <datalist id="rule-model-options">
          {knownModels.map((m) => (
            <option key={m} value={m} />
          ))}
        </datalist>
        {value.length === 0 && (
          <p className="text-2xs text-muted-foreground">暂无自定义规则。</p>
        )}
        {value.map((rule) => {
          const regexError =
            rule.isRegex && rule.enable && rule.keyword ? tryRegex(rule.keyword) : null;
          return (
            <div
              key={rule.id}
              data-testid={`rule-row-${rule.id}`}
              className="space-y-1 rounded-lg border bg-muted/20 p-2"
            >
              <div className="flex items-center gap-1.5">
                <input
                  type="checkbox"
                  checked={rule.enable}
                  data-testid={`rule-enable-toggle-${rule.id}`}
                  onChange={(e) => update(rule.id, { enable: e.target.checked })}
                  className="h-4 w-4 rounded border-input text-primary"
                  title="启用/禁用该规则"
                />
                <input
                  value={rule.keyword}
                  placeholder={rule.isRegex ? "正则表达式，如 issue-\\d+" : "关键词，如 排课"}
                  data-testid={`rule-keyword-input-${rule.id}`}
                  onChange={(e) => update(rule.id, { keyword: e.target.value })}
                  className="flex-1 h-8 text-xs font-mono rounded-md border border-input bg-background px-2 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                />
                <label className="flex items-center gap-1 text-2xs text-muted-foreground">
                  <input
                    type="checkbox"
                    checked={rule.isRegex}
                    data-testid={`rule-regex-toggle-${rule.id}`}
                    onChange={(e) => update(rule.id, { isRegex: e.target.checked })}
                    className="h-3.5 w-3.5 rounded border-input text-primary"
                  />
                  正则
                </label>
                <span className="text-muted-foreground text-xs">→</span>
                <input
                  value={rule.targetModel}
                  list="rule-model-options"
                  placeholder="目标模型"
                  data-testid={`rule-target-input-${rule.id}`}
                  onChange={(e) => update(rule.id, { targetModel: e.target.value })}
                  className="flex-1 h-8 text-xs font-mono rounded-md border border-input bg-background px-2 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  className="text-destructive"
                  data-testid={`rule-delete-${rule.id}`}
                  onClick={() => onChange(value.filter((r) => r.id !== rule.id))}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
              {regexError && (
                <p className="text-2xs text-warning" data-testid={`rule-error-${rule.id}`}>
                  正则无法编译，保存后该规则会被禁用：{regexError}
                </p>
              )}
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}

function tryRegex(pattern: string): string | null {
  try {
    new RegExp(pattern);
    return null;
  } catch (e) {
    return e instanceof Error ? e.message : String(e);
  }
}

interface ImportExportProps {
  settings: SmartRouteSettings;
  onImport: (parsed: SmartRouteSettings) => void;
}

/** Paste-based import/export — no file dialogs in this app's UI vocabulary. */
export function ImportExport({ settings, onImport }: ImportExportProps) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const exportText = JSON.stringify(settings, null, 2);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(exportText);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      setStatus("复制失败，请手动选中复制");
    }
  };

  const handleImport = () => {
    try {
      const parsed = JSON.parse(text) as SmartRouteSettings;
      onImport(parsed);
      setStatus("导入成功，记得保存");
    } catch (e) {
      setStatus(`导入失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  return (
    <Card data-testid="import-export-card">
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm">导入 / 导出</CardTitle>
          <Button type="button" variant="outline" size="xs" onClick={() => setOpen(!open)}>
            {open ? "收起" : "展开"}
          </Button>
        </div>
      </CardHeader>
      {open && (
        <CardContent className="space-y-2">
          <div className="space-y-1">
            <div className="flex items-center gap-2">
              <span className="text-2xs font-medium text-muted-foreground">当前配置</span>
              <Button
                type="button"
                variant="ghost"
                size="xs"
                data-testid="export-copy-button"
                onClick={handleCopy}
              >
                {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
                {copied ? "已复制" : "复制"}
              </Button>
            </div>
            <textarea
              readOnly
              value={exportText}
              data-testid="export-textarea"
              className="w-full h-32 text-2xs font-mono rounded-md border border-input bg-muted/30 p-2 text-foreground"
            />
          </div>
          <div className="space-y-1">
            <div className="flex items-center gap-2">
              <span className="text-2xs font-medium text-muted-foreground">粘贴配置以导入</span>
              <Button
                type="button"
                variant="ghost"
                size="xs"
                data-testid="import-button"
                onClick={handleImport}
              >
                <ClipboardPaste className="h-3 w-3" /> 导入
              </Button>
            </div>
            <textarea
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder="粘贴完整的 JSON 配置"
              data-testid="import-textarea"
              className="w-full h-32 text-2xs font-mono rounded-md border border-input bg-background p-2 text-foreground"
            />
          </div>
          {status && (
            <p className="text-2xs text-primary" data-testid="import-export-status">
              {status}
            </p>
          )}
        </CardContent>
      )}
    </Card>
  );
}

interface SimulatorProps {
  knownModels: string[];
  simulate: (model: string, prompt: string, effort?: string) => Promise<import("@/domain/smartRoute").RouteDecision | null>;
}

/** Route simulator: dry-run the engine on a prompt, no request is sent. */
export function Simulator({ knownModels, simulate }: SimulatorProps) {
  const [model, setModel] = useState("");
  const [prompt, setPrompt] = useState("");
  const [effort, setEffort] = useState("");
  const [result, setResult] = useState<import("@/domain/smartRoute").RouteDecision | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setRunning(true);
    setError(null);
    try {
      const decision = await simulate(model, prompt, effort || undefined);
      setResult(decision);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  };

  return (
    <Card data-testid="simulator-card">
      <CardHeader className="pb-3">
        <CardTitle className="text-sm">路由模拟调试器</CardTitle>
        <p className="text-2xs text-muted-foreground">
          输入模型名与一段 prompt，按已保存的配置模拟判定（不发送任何请求）。修改配置后需先保存再模拟。
        </p>
      </CardHeader>
      <CardContent className="space-y-2">
        <datalist id="simulator-model-options">
          {knownModels.map((m) => (
            <option key={m} value={m} />
          ))}
        </datalist>
        <div className="flex items-center gap-1.5">
          <input
            value={model}
            list="simulator-model-options"
            placeholder="入站模型名（可留空）"
            data-testid="simulator-model-input"
            onChange={(e) => setModel(e.target.value)}
            className="flex-1 h-8 text-xs font-mono rounded-md border border-input bg-background px-2 text-foreground"
          />
          <select
            value={effort}
            data-testid="simulator-effort-select"
            onChange={(e) => setEffort(e.target.value)}
            className="w-28 h-8 text-xs rounded-md border border-input bg-background px-2 text-foreground"
          >
            <option value="">无 effort</option>
            <option value="low">low</option>
            <option value="medium">medium</option>
            <option value="high">high</option>
            <option value="max">max</option>
          </select>
        </div>
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          placeholder="粘贴一段 prompt，比如「设计多租户行级权限」或含 #MODEL:glm-5.3 的文本"
          data-testid="simulator-prompt-input"
          className="w-full h-24 text-xs rounded-md border border-input bg-background p-2 text-foreground"
        />
        <Button
          type="button"
          size="xs"
          disabled={running}
          data-testid="simulator-run-button"
          onClick={run}
        >
          {running ? "模拟中…" : "运行模拟"}
        </Button>
        {error && (
          <p className="text-2xs text-destructive" data-testid="simulator-error">
            {error}
          </p>
        )}
        {result && (
          <div
            className="space-y-1 rounded-lg border bg-muted/30 p-3 text-xs"
            data-testid="simulator-result"
          >
            <div className="flex items-center gap-2">
              <Badge variant="info">{reasonText(result.reason)}</Badge>
              <span className="font-mono font-semibold">{result.routedModel}</span>
            </div>
            {result.matchRule && (
              <p className="text-2xs text-muted-foreground">命中规则：{result.matchRule}</p>
            )}
            {result.isTagHit && (
              <p className="text-2xs text-muted-foreground">#MODEL: 标签命中</p>
            )}
            {result.validationWarn && (
              <p className="text-2xs text-warning">{result.validationWarn}</p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function reasonText(reason: import("@/domain/smartRoute").RouteDecision["reason"]): string {
  if (typeof reason === "object") return `分类：${reason.category}`;
  const labels: Record<string, string> = {
    force_model: "强制模型",
    model_tag: "#MODEL: 标签",
    custom_rule: "自定义规则",
    default_model: "默认模型",
    passthrough: "透传（无规则命中）",
    validation_fallback: "模型校验回退",
    fallback_retry: "失败回退",
  };
  return labels[reason] ?? String(reason);
}

interface AuditPanelProps {
  load: (requestId?: string) => Promise<import("@/domain/smartRoute").RouteAuditRecord[]>;
}

/** The decision trail, newest first, filterable by request id. */
export function AuditPanel({ load }: AuditPanelProps) {
  const [records, setRecords] = useState<import("@/domain/smartRoute").RouteAuditRecord[]>([]);
  const [filter, setFilter] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try {
      setRecords(await load(filter.trim() || undefined));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <Card data-testid="audit-card">
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm">路由审计</CardTitle>
          <Button
            type="button"
            variant="outline"
            size="xs"
            data-testid="audit-refresh-button"
            onClick={refresh}
            disabled={loading}
          >
            刷新
          </Button>
        </div>
        <p className="text-2xs text-muted-foreground">
          每条请求的判定记录（内存中最近 1000 条，脱敏：只记元数据不记内容）。
        </p>
      </CardHeader>
      <CardContent className="space-y-2">
        <input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="按 request_id 过滤（回车后点刷新）"
          data-testid="audit-filter-input"
          className="w-full h-8 text-xs font-mono rounded-md border border-input bg-background px-2 text-foreground"
        />
        {error && (
          <p className="text-2xs text-destructive" data-testid="audit-error">
            {error}
          </p>
        )}
        {records.length === 0 && !loading && (
          <p className="text-2xs text-muted-foreground">暂无记录。点击刷新拉取。</p>
        )}
        <div className="max-h-72 overflow-y-auto space-y-1.5">
          {records.map((r) => (
            <div
              key={`${r.requestId}-${r.isFallback}`}
              data-testid={`audit-row-${r.requestId}`}
              className="rounded-lg border bg-muted/20 p-2 text-2xs space-y-0.5"
            >
              <div className="flex items-center gap-2">
                <span className="font-mono text-muted-foreground">{r.timestamp}</span>
                <Badge variant={r.isFallback ? "warning" : "secondary"}>{r.routeReason}</Badge>
                {r.isFallback && <Badge variant="warning">回退</Badge>}
              </div>
              <div className="font-mono">
                {r.originalModel} → {r.aliasedModel} → <span className="font-semibold">{r.routedModel}</span>
              </div>
              <div className="flex items-center gap-2 text-muted-foreground">
                <span>{r.endpoint}</span>
                {r.matchRule && <span>规则 {r.matchRule}</span>}
                {r.thinkingEffort && <span>effort={r.thinkingEffort}</span>}
                <span>~{r.inputTokens} tok</span>
              </div>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
}
