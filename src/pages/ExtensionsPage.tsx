import { useEffect, useState } from "react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, type TabItem } from "@/components/ui/tabs";
import { KeyValue, KeyValueGrid } from "@/components/ui/key-value";
import { backend } from "@/services/backend";
import type { McpServer, ManagedSkill, PromptTemplate } from "@/domain/extensions";
import type { InjectStatus } from "@/domain/injection";
import {
  Puzzle,
  Server,
  Sparkles,
  FileText,
  Radio,
  RotateCw,
  Shield,
  Wrench,
} from "lucide-react";

type ExtensionTab = "mcp" | "skills" | "prompts" | "inject";

const TAB_ICONS = {
  mcp: Server,
  skills: Sparkles,
  prompts: FileText,
  inject: Radio,
} as const;

export default function ExtensionsPage() {
  const [activeTab, setActiveTab] = useState<ExtensionTab>("mcp");
  const [mcpServers, setMcpServers] = useState<McpServer[]>([]);
  const [skills, setSkills] = useState<ManagedSkill[]>([]);
  const [prompts, setPrompts] = useState<PromptTemplate[]>([]);
  const [injectStatus, setInjectStatus] = useState<InjectStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [injecting, setInjecting] = useState(false);

  const loadData = async () => {
    setLoading(true);
    try {
      const [mList, sList, pList, iStatus] = await Promise.all([
        backend.listMcpServers().catch(() => []),
        backend.listSkills().catch(() => []),
        backend.listPrompts().catch(() => []),
        backend.injectStatus().catch(() => null),
      ]);
      setMcpServers(mList);
      setSkills(sList);
      setPrompts(pList);
      setInjectStatus(iStatus);
    } catch (err) {
      console.error("Failed to load extensions:", err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadData();
  }, []);

  const handleInstallInject = async () => {
    setInjecting(true);
    try {
      const res = await backend.injectInstallNative();
      setInjectStatus(res);
    } catch (err) {
      alert(`安装注入失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setInjecting(false);
    }
  };

  const handleRepairInject = async () => {
    setInjecting(true);
    try {
      const res = await backend.injectRepair();
      setInjectStatus(res);
    } catch (err) {
      alert(`修复注入失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setInjecting(false);
    }
  };

  return (
    <div className="space-y-8 max-w-6xl mx-auto pb-12">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <Badge variant="info" className="px-3 py-1">
              <Puzzle className="h-3 w-3 mr-1" />
              生态扩展
            </Badge>
            <span className="text-xs text-muted-foreground">
              MCP 服务器 ({mcpServers.length}) · Skills ({skills.length}) · Prompts ({prompts.length})
            </span>
          </div>
          <h1 className="text-3xl font-extrabold tracking-tight mt-1">扩展生态与脚本注入</h1>
          <p className="text-muted-foreground text-sm">
            管理 Model Context Protocol (MCP) 上下文服务器、Agent 技能包、提示词模板及客户端无感注入能力。
          </p>
        </div>

        <Button variant="outline" size="sm" onClick={loadData} disabled={loading} className="text-xs">
          <RotateCw className={`h-3.5 w-3.5 mr-1 ${loading ? "animate-spin" : ""}`} />
          刷新数据
        </Button>
      </div>

      <Tabs
        value={activeTab}
        onChange={setActiveTab}
        items={
          [
            { id: "mcp", label: "MCP 服务器", icon: TAB_ICONS.mcp, count: mcpServers.length },
            { id: "skills", label: "Skills 技能", icon: TAB_ICONS.skills, count: skills.length },
            { id: "prompts", label: "提示词模板", icon: TAB_ICONS.prompts, count: prompts.length },
            { id: "inject", label: "Native 注入管理", icon: TAB_ICONS.inject },
          ] satisfies TabItem<ExtensionTab>[]
        }
      />

      {/* Tab 1: MCP Servers */}
      {activeTab === "mcp" && (
        <div className="space-y-4">
          <div className="text-xs text-muted-foreground">
            MCP 为大模型提供本地文件系统、数据库查询、GitHub API 等上下文增强工具调用标准。
          </div>
          {mcpServers.length === 0 ? (
            <Card>
              <CardContent className="p-8 text-center text-muted-foreground text-xs">
                暂未加载到 MCP 服务器配置
              </CardContent>
            </Card>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {mcpServers.map((s) => (
                <Card key={s.id} className="border-border/60 shadow-sm flex flex-col justify-between">
                  <CardHeader className="pb-2">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <div className="p-2 rounded-lg bg-primary/10 text-primary">
                          <Server className="h-4 w-4" />
                        </div>
                        <div>
                          <CardTitle className="text-sm">{s.name}</CardTitle>
                          <p className="text-2xs text-muted-foreground font-mono">ID: {s.id}</p>
                        </div>
                      </div>
                      {s.isBuiltin && <Badge variant="info" className="text-2xs">内置</Badge>}
                    </div>
                    <p className="text-xs text-muted-foreground mt-2">{s.description || "无描述信息"}</p>
                  </CardHeader>
                  <CardContent className="pt-0 space-y-2">
                    <div className="rounded-md border bg-muted/60 p-2 font-mono text-xs">
                      <span className="block text-2xs text-muted-foreground">执行命令与参数</span>
                      <code className="block truncate text-foreground">
                        {s.command} {s.args?.join(" ")}
                      </code>
                    </div>
                  </CardContent>
                </Card>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Tab 2: Skills */}
      {activeTab === "skills" && (
        <div className="space-y-4">
          <div className="text-xs text-muted-foreground">
            Skills 为 Agent 提供专业场景任务指令与执行流程封装。
          </div>
          {skills.length === 0 ? (
            <Card>
              <CardContent className="p-8 text-center text-muted-foreground text-xs space-y-2">
                <Sparkles className="h-8 w-8 mx-auto text-muted-foreground/50" />
                <p>当前暂无已安装的外部 Skills 技能包</p>
                <p className="text-2xs opacity-80">支持从 GitHub 仓库直接同步符合规范的技能定义</p>
              </CardContent>
            </Card>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {skills.map((skill) => (
                <Card key={skill.id} className="border-border/60">
                  <CardHeader className="pb-2">
                    <div className="flex items-center justify-between">
                      <CardTitle className="text-sm">{skill.name}</CardTitle>
                      <Badge variant={skill.enabled ? "success" : "secondary"} className="text-2xs">
                        {skill.enabled ? "启用中" : "已禁用"}
                      </Badge>
                    </div>
                    <p className="text-xs text-muted-foreground">{skill.description}</p>
                  </CardHeader>
                </Card>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Tab 3: Prompt Templates */}
      {activeTab === "prompts" && (
        <div className="space-y-4">
          <div className="text-xs text-muted-foreground">
            提示词模板用于在发起请求前进行系统角色注入与任务引导。
          </div>
          {prompts.length === 0 ? (
            <Card>
              <CardContent className="p-8 text-center text-muted-foreground text-xs space-y-2">
                <FileText className="h-8 w-8 mx-auto text-muted-foreground/50" />
                <p>暂无自定义提示词模板</p>
                <p className="text-2xs opacity-80">可在 Profile 配置中绑定针对性系统提示词与代码审查规范</p>
              </CardContent>
            </Card>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {prompts.map((p) => (
                <Card key={p.id} className="border-border/60">
                  <CardHeader className="pb-2">
                    <div className="flex items-center justify-between">
                      <CardTitle className="text-sm">{p.name}</CardTitle>
                      <Badge variant="outline" className="text-2xs">{p.scope}</Badge>
                    </div>
                  </CardHeader>
                  <CardContent className="space-y-2 text-xs">
                    <div className="max-h-24 overflow-y-auto rounded-md border bg-muted/60 p-2 font-mono text-xs">
                      {p.content}
                    </div>
                    {p.variables.length > 0 && (
                      <div className="flex flex-wrap items-center gap-1 text-2xs">
                        <span className="text-muted-foreground">变量</span>
                        {p.variables.map((v) => (
                          <Badge key={v} variant="secondary" className="px-1.5 py-0 text-2xs font-mono">{v}</Badge>
                        ))}
                      </div>
                    )}
                  </CardContent>
                </Card>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Tab 4: Native Inject */}
      {activeTab === "inject" && (
        <div className="space-y-6">
          <Card className="border-border/60">
            <CardHeader className="pb-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Shield className="h-5 w-5 text-primary" />
                  <CardTitle className="text-base font-semibold">Webview / CDP 无感注入状态</CardTitle>
                </div>
                {injectStatus?.native?.healthy ? (
                  <Badge variant="success">运行健康</Badge>
                ) : (
                  <Badge variant="secondary">未激活 / 独立代理模式</Badge>
                )}
              </div>
            </CardHeader>
            <CardContent className="space-y-4 text-xs">
              <p className="text-muted-foreground leading-relaxed">
                针对使用 Electron / Chromium Webview 的 GUI AI 客户端（如 Codex Desktop 等），PolyDeck 支持通过 Native 用户脚本与 CDP 调试通道实现无感抓包与 Stepwise 建议注入。
              </p>

              <KeyValueGrid className="rounded-lg border bg-muted/30 p-4 lg:grid-cols-4">
                <KeyValue label="当前阶段 Stage">{injectStatus?.stage ?? "Unavailable"}</KeyValue>
                <KeyValue label="注入通道 Channel">{injectStatus?.channel ?? "None"}</KeyValue>
                <KeyValue label="脚本状态">
                  {injectStatus?.native?.installed ? "已就绪" : "未安装"}
                </KeyValue>
                <KeyValue label="校验摘要" mono>
                  <span title={injectStatus?.native?.script_hash ?? "N/A"}>
                    {injectStatus?.native?.script_hash
                      ? injectStatus.native.script_hash.slice(0, 12) + "…"
                      : "无"}
                  </span>
                </KeyValue>
              </KeyValueGrid>

              <div className="flex gap-3 pt-2">
                <Button
                  size="sm"
                  onClick={handleInstallInject}
                  disabled={injecting}
                  className="text-xs"
                >
                  <Wrench className="h-3.5 w-3.5 mr-1" />
                  安装 / 更新 Native 注入
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleRepairInject}
                  disabled={injecting}
                  className="text-xs"
                >
                  <RotateCw className="h-3.5 w-3.5 mr-1" />
                  诊断与修复
                </Button>
              </div>
            </CardContent>
          </Card>
        </div>
      )}
    </div>
  );
}