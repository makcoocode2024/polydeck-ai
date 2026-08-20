import { useEffect, useState } from "react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
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

      {/* Tabs */}
      <div className="flex border-b border-border/60 gap-2">
        <button
          onClick={() => setActiveTab("mcp")}
          className={`pb-2.5 px-4 text-xs font-semibold border-b-2 transition-all flex items-center gap-2 ${
            activeTab === "mcp"
              ? "border-primary text-primary"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          <Server className="h-4 w-4" />
          MCP 服务器 ({mcpServers.length})
        </button>

        <button
          onClick={() => setActiveTab("skills")}
          className={`pb-2.5 px-4 text-xs font-semibold border-b-2 transition-all flex items-center gap-2 ${
            activeTab === "skills"
              ? "border-primary text-primary"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          <Sparkles className="h-4 w-4" />
          Skills 技能 ({skills.length})
        </button>

        <button
          onClick={() => setActiveTab("prompts")}
          className={`pb-2.5 px-4 text-xs font-semibold border-b-2 transition-all flex items-center gap-2 ${
            activeTab === "prompts"
              ? "border-primary text-primary"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          <FileText className="h-4 w-4" />
          提示词模板 ({prompts.length})
        </button>

        <button
          onClick={() => setActiveTab("inject")}
          className={`pb-2.5 px-4 text-xs font-semibold border-b-2 transition-all flex items-center gap-2 ${
            activeTab === "inject"
              ? "border-primary text-primary"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          <Radio className="h-4 w-4" />
          Native 注入管理
        </button>
      </div>

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
                          <CardTitle className="text-sm font-semibold">{s.name}</CardTitle>
                          <p className="text-[10px] text-muted-foreground font-mono">ID: {s.id}</p>
                        </div>
                      </div>
                      {s.isBuiltin && <Badge variant="info" className="text-[10px]">内置</Badge>}
                    </div>
                    <p className="text-xs text-muted-foreground mt-2">{s.description || "无描述信息"}</p>
                  </CardHeader>
                  <CardContent className="pt-0 space-y-2">
                    <div className="p-2 bg-muted/40 rounded text-[11px] font-mono">
                      <span className="text-muted-foreground block text-[10px]">执行命令与参数:</span>
                      <code className="text-foreground truncate block">
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
                <p className="text-[11px] opacity-80">支持从 GitHub 仓库直接同步符合规范的技能定义</p>
              </CardContent>
            </Card>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {skills.map((skill) => (
                <Card key={skill.id} className="border-border/60">
                  <CardHeader className="pb-2">
                    <div className="flex items-center justify-between">
                      <CardTitle className="text-sm font-semibold">{skill.name}</CardTitle>
                      <Badge variant={skill.enabled ? "success" : "secondary"} className="text-[10px]">
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
                <p className="text-[11px] opacity-80">可在 Profile 配置中绑定针对性系统提示词与代码审查规范</p>
              </CardContent>
            </Card>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {prompts.map((p) => (
                <Card key={p.id} className="border-border/60">
                  <CardHeader className="pb-2">
                    <div className="flex items-center justify-between">
                      <CardTitle className="text-sm font-semibold">{p.name}</CardTitle>
                      <Badge variant="outline" className="text-[10px]">{p.scope}</Badge>
                    </div>
                  </CardHeader>
                  <CardContent className="space-y-2 text-xs">
                    <div className="p-2 bg-muted/40 rounded font-mono text-[11px] max-h-24 overflow-y-auto">
                      {p.content}
                    </div>
                    {p.variables.length > 0 && (
                      <div className="flex gap-1 flex-wrap text-[10px]">
                        <span className="text-muted-foreground">变量:</span>
                        {p.variables.map((v) => (
                          <Badge key={v} variant="secondary" className="text-[9px] px-1 py-0">{v}</Badge>
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

              <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-4 gap-3">
                <div className="p-3 bg-muted/30 rounded-lg border space-y-1">
                  <div className="text-muted-foreground text-[11px]">当前阶段 (Stage)</div>
                  <div className="font-semibold text-xs">{injectStatus?.stage ?? "Unavailable"}</div>
                </div>
                <div className="p-3 bg-muted/30 rounded-lg border space-y-1">
                  <div className="text-muted-foreground text-[11px]">注入通道 (Channel)</div>
                  <div className="font-semibold text-xs">{injectStatus?.channel ?? "None"}</div>
                </div>
                <div className="p-3 bg-muted/30 rounded-lg border space-y-1">
                  <div className="text-muted-foreground text-[11px]">脚本状态</div>
                  <div className="font-semibold text-xs">
                    {injectStatus?.native?.installed ? "已就绪" : "未安装"}
                  </div>
                </div>
                <div className="p-3 bg-muted/30 rounded-lg border space-y-1">
                  <div className="text-muted-foreground text-[11px]">校验摘要</div>
                  <div className="font-mono text-[10px] truncate" title={injectStatus?.native?.script_hash ?? "N/A"}>
                    {injectStatus?.native?.script_hash ? injectStatus.native.script_hash.slice(0, 12) + "..." : "无"}
                  </div>
                </div>
              </div>

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