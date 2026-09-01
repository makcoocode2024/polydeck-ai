import { useEffect, useState } from "react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { backend } from "@/services/backend";
import type { DetectedClient } from "@/domain/client";
import {
  Monitor,
  CheckCircle2,
  XCircle,
  RotateCw,
  Terminal,
  Copy,
  Check,
  Code2,
  Bot,
  Laptop,
} from "lucide-react";

export default function ClientsPage() {
  const [clients, setClients] = useState<DetectedClient[]>([]);
  const [loading, setLoading] = useState(true);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  const fetchClients = async () => {
    setLoading(true);
    try {
      const data = await backend.detectClients();
      setClients(data);
    } catch (err) {
      console.error("Failed to detect clients:", err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchClients();
  }, []);

  const copySnippet = (key: string, text: string) => {
    navigator.clipboard.writeText(text);
    setCopiedKey(key);
    setTimeout(() => setCopiedKey(null), 2000);
  };

  const installedCount = clients.filter((c) => c.installed).length;

  return (
    <div className="space-y-8 max-w-6xl mx-auto pb-12">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <Badge variant="info" className="px-3 py-1">
              <Monitor className="h-3 w-3 mr-1" />
              客户端支持
            </Badge>
            <span className="text-xs text-muted-foreground">已检测到 {installedCount} / {clients.length} 个客户端</span>
          </div>
          <h1 className="text-3xl font-extrabold tracking-tight mt-1">AI 开发客户端与接入</h1>
          <p className="text-muted-foreground text-sm">
            PolyDeck 自动扫描系统中的 Claude Desktop、Hermes、Codex、Cursor 等主流客户端，提供一键接入、配置同步与代理分发。
          </p>
        </div>

        <Button variant="outline" size="sm" onClick={fetchClients} disabled={loading} className="text-xs">
          <RotateCw className={`h-3.5 w-3.5 mr-1 ${loading ? "animate-spin" : ""}`} />
          重新扫描
        </Button>
      </div>

      {/* Clients Cards Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {clients.map((client) => (
          <Card
            key={client.id}
            className={`border transition-all ${
              client.installed
                ? "border-border/80 bg-card/80 shadow-sm"
                : "border-border/40 bg-muted/20 opacity-70"
            }`}
          >
            <CardHeader className="pb-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <div className={`p-2 rounded-lg ${client.installed ? "bg-primary/10 text-primary" : "bg-muted text-muted-foreground"}`}>
                    {client.id.includes("desktop") || client.id.includes("studio") || client.id.includes("chatbox") ? (
                      <Laptop className="h-4 w-4" />
                    ) : client.id.includes("hermes") ? (
                      <Bot className="h-4 w-4" />
                    ) : (
                      <Terminal className="h-4 w-4" />
                    )}
                  </div>
                  <CardTitle className="text-base font-semibold">{client.name}</CardTitle>
                </div>
                {client.installed ? (
                  <Badge variant="success" className="text-[10px]">
                    <CheckCircle2 className="h-2.5 w-2.5 mr-1" />
                    已安装
                  </Badge>
                ) : (
                  <Badge variant="secondary" className="text-[10px]">
                    <XCircle className="h-2.5 w-2.5 mr-1" />
                    未发现
                  </Badge>
                )}
              </div>
            </CardHeader>
            <CardContent className="space-y-3 pt-1">
              <div className="text-xs space-y-1">
                {client.version && (
                  <div className="text-muted-foreground">
                    版本号: <span className="font-mono text-foreground">{client.version}</span>
                  </div>
                )}
                {client.configPath && (
                  <div className="text-muted-foreground">
                    <span className="block text-[11px] mb-0.5">配置文件路径:</span>
                    <span className="font-mono text-[10px] bg-muted/60 px-1.5 py-0.5 rounded block truncate" title={client.configPath}>
                      {client.configPath}
                    </span>
                  </div>
                )}
              </div>

              <div className="pt-2 border-t flex items-center justify-between text-[11px]">
                <span className="text-muted-foreground">
                  {client.supportsAutoConfig ? "支持一键配置同步写入" : "支持本地网关代理接入"}
                </span>
                {client.installed && (
                  <Badge variant="outline" className="text-[10px]">就绪</Badge>
                )}
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {/* Client Configuration Snippets */}
      <div className="space-y-4 pt-4 border-t">
        <div className="flex items-center gap-2">
          <Code2 className="h-4 w-4 text-primary" />
          <h2 className="text-lg font-bold">主流客户端接入配置指南</h2>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {/* Claude Desktop */}
          <Card className="border-border/60">
            <CardHeader className="pb-2">
              <div className="flex items-center justify-between">
                <CardTitle className="text-sm font-semibold">Claude Desktop 官方桌面端</CardTitle>
                <Badge variant="outline" className="text-[10px]">MCP 自动同步 / 端点手填</Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-3 text-xs">
              <p className="text-muted-foreground text-[11px]">
                PolyDeck 只能同步 <code>claude_desktop_config.json</code> 里的 MCP 服务。网关地址与令牌存在 Desktop 的账号设置里，需在其中手填（不要带 <code>/v1</code>，Anthropic SDK 会自行拼接，否则请求 <code>/v1/v1/messages</code> 并 404）：
              </p>
              <div className="p-2.5 bg-muted/50 rounded font-mono text-[11px] space-y-1 relative group">
                <div>Gateway URL: <b>http://127.0.0.1:18888</b></div>
                <div>Auth Token: <b>Bearer ai-deck-local</b></div>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 w-6 p-0 absolute top-2 right-2"
                  onClick={() => copySnippet("claude-desktop", "http://127.0.0.1:18888")}
                >
                  {copiedKey === "claude-desktop" ? <Check className="h-3 w-3 text-emerald-500" /> : <Copy className="h-3 w-3" />}
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Hermes Agent */}
          <Card className="border-border/60">
            <CardHeader className="pb-2">
              <div className="flex items-center justify-between">
                <CardTitle className="text-sm font-semibold">Hermes Agent / CLI</CardTitle>
                <Badge variant="outline" className="text-[10px]">Agent 终端与配置</Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-3 text-xs">
              <p className="text-muted-foreground text-[11px]">
                配置文件位于 <code>~/.hermes/config.yaml</code>，切换方案时 PolyDeck 会自动写入模型与 Base URL。
              </p>
              <div className="p-2.5 bg-muted/50 rounded font-mono text-[11px] space-y-1 relative group">
                <div>api_base: <b>http://127.0.0.1:18888/v1</b></div>
                <div>api_key: <b>sk-ai-deck-local</b></div>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 w-6 p-0 absolute top-2 right-2"
                  onClick={() => copySnippet("hermes", "http://127.0.0.1:18888/v1")}
                >
                  {copiedKey === "hermes" ? <Check className="h-3 w-3 text-emerald-500" /> : <Copy className="h-3 w-3" />}
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Cursor / VS Code (Cline / Continue) */}
          <Card className="border-border/60">
            <CardHeader className="pb-2">
              <div className="flex items-center justify-between">
                <CardTitle className="text-sm font-semibold">Cursor / Windsurf / VS Code</CardTitle>
                <Badge variant="outline" className="text-[10px]">OpenAI 兼容协议</Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-3 text-xs">
              <p className="text-muted-foreground text-[11px]">
                在客户端模型设置中将 Base URL 指向 PolyDeck 本地网关，API Key 可填任意字符串。
              </p>
              <div className="p-2.5 bg-muted/50 rounded font-mono text-[11px] space-y-1 relative group">
                <div>Base URL: <b>http://127.0.0.1:18888/v1</b></div>
                <div>API Key: <b>sk-ai-deck-local</b></div>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 w-6 p-0 absolute top-2 right-2"
                  onClick={() => copySnippet("cursor", "http://127.0.0.1:18888/v1")}
                >
                  {copiedKey === "cursor" ? <Check className="h-3 w-3 text-emerald-500" /> : <Copy className="h-3 w-3" />}
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Claude Code CLI */}
          <Card className="border-border/60">
            <CardHeader className="pb-2">
              <div className="flex items-center justify-between">
                <CardTitle className="text-sm font-semibold">Claude Code CLI</CardTitle>
                <Badge variant="outline" className="text-[10px]">Anthropic 环境变量</Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-3 text-xs">
              <p className="text-muted-foreground text-[11px]">
                在终端启动 Claude Code 前，设置 <code>ANTHROPIC_BASE_URL</code> 环境变量指向 PolyDeck（不要带 <code>/v1</code>，Anthropic SDK 会自行拼接）：
              </p>
              <div className="p-2.5 bg-muted/50 rounded font-mono text-[11px] relative group">
                <code>$env:ANTHROPIC_BASE_URL="http://127.0.0.1:18888"</code>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 w-6 p-0 absolute top-2 right-2"
                  onClick={() => copySnippet("claude", '$env:ANTHROPIC_BASE_URL="http://127.0.0.1:18888"')}
                >
                  {copiedKey === "claude" ? <Check className="h-3 w-3 text-emerald-500" /> : <Copy className="h-3 w-3" />}
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Codex CLI */}
          <Card className="border-border/60">
            <CardHeader className="pb-2">
              <div className="flex items-center justify-between">
                <CardTitle className="text-sm font-semibold">Codex CLI / Agent 终端</CardTitle>
                <Badge variant="outline" className="text-[10px]">Responses 协议</Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-3 text-xs">
              <p className="text-muted-foreground text-[11px]">
                Codex CLI 原生支持 OpenAI Responses 协议，PolyDeck 会自动完成双向格式转译与流式对齐。
              </p>
              <div className="p-2.5 bg-muted/50 rounded font-mono text-[11px] relative group">
                <code>codex --api-base http://127.0.0.1:18888/v1</code>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 w-6 p-0 absolute top-2 right-2"
                  onClick={() => copySnippet("codex", "codex --api-base http://127.0.0.1:18888/v1")}
                >
                  {copiedKey === "codex" ? <Check className="h-3 w-3 text-emerald-500" /> : <Copy className="h-3 w-3" />}
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Web UI / Chatbox / Cherry Studio */}
          <Card className="border-border/60">
            <CardHeader className="pb-2">
              <div className="flex items-center justify-between">
                <CardTitle className="text-sm font-semibold">Cherry Studio / Chatbox / NextChat</CardTitle>
                <Badge variant="outline" className="text-[10px]">GUI 客户端</Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-3 text-xs">
              <p className="text-muted-foreground text-[11px]">
                在客户端提供商中选择 OpenAI API，并将 API 域名改为本地端口即可享受低延迟与智能故障切换。
              </p>
              <div className="p-2.5 bg-muted/50 rounded font-mono text-[11px]">
                <span>API Host: </span><b className="text-foreground">http://127.0.0.1:18888</b>
              </div>
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}
