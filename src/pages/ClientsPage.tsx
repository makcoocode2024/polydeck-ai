import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { CodeSnippet, type SnippetLine } from "@/components/ui/code-snippet";
import { backend } from "@/services/backend";
import type { DetectedClient } from "@/domain/client";
import type { ClientConnectionInfo } from "@/domain/profile";
import {
  Monitor,
  CheckCircle2,
  XCircle,
  RotateCw,
  Terminal,
  Code2,
  Bot,
  Laptop,
  Link2Off,
  type LucideIcon,
} from "lucide-react";

/**
 * Explicit, because `client.id.includes(...)` silently gave every unrecognised
 * client the terminal icon — a new GUI client would look like a CLI until
 * somebody noticed.
 */
const CLIENT_ICONS: Record<string, LucideIcon> = {
  "claude-desktop": Laptop,
  "cherry-studio": Laptop,
  chatbox: Laptop,
  hermes: Bot,
  "codex-cli": Terminal,
  "claude-code": Terminal,
  cursor: Code2,
  windsurf: Code2,
  vscode: Code2,
  opencode: Code2,
};

function clientIcon(id: string): LucideIcon {
  return CLIENT_ICONS[id] ?? Monitor;
}

/**
 * Real endpoint and bearer for one client, or an explicit statement that there
 * is none yet.
 *
 * This card used to print `Bearer ai-deck-local` as static text. The gateway has
 * rejected that sentinel since 2.1.0 and routes on a per-client token instead,
 * so anyone who followed the instruction got `401 No profile is bound to this
 * token`. An unbound client therefore says so rather than falling back to a
 * value that cannot work.
 */
function ConnectionBlock({
  state,
  /** Rendered with the resolved values once they arrive. */
  render,
}: {
  state: ConnState | undefined;
  render: (info: ClientConnectionInfo) => { lines: SnippetLine[]; copyText?: string };
}) {
  if (!state || state.status === "loading") {
    return (
      <div className="rounded-md border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
        读取连接信息…
      </div>
    );
  }

  if (state.status === "unbound") {
    return (
      <div className="space-y-1.5 rounded-md border border-warning/30 bg-warning-surface px-3 py-2">
        <div className="flex items-center gap-1.5 text-xs font-medium text-warning-foreground">
          <Link2Off className="h-3.5 w-3.5 shrink-0" />
          还没有绑定方案，暂无可用凭证
        </div>
        <p className="text-2xs leading-relaxed text-warning-foreground/80">
          令牌由网关按客户端逐个签发，绑定之后才存在。到
          <Link to="/profiles" className="mx-1 underline underline-offset-2">
            配置方案
          </Link>
          页把这个客户端绑到一个方案上，这里就会显示它真实的地址与令牌。
        </p>
      </div>
    );
  }

  const { lines, copyText } = render(state.info);
  return <CodeSnippet lines={lines} copyText={copyText} />;
}

/** Which client each snippet card is really configuring. */
type SnippetClient = "claude-desktop" | "hermes" | "claude-code" | "codex-cli" | "cursor";

type ConnState =
  | { status: "loading" }
  | { status: "ready"; info: ClientConnectionInfo }
  | { status: "unbound"; message: string };

export default function ClientsPage() {
  const [clients, setClients] = useState<DetectedClient[]>([]);
  const [loading, setLoading] = useState(true);
  const [conns, setConns] = useState<Record<string, ConnState>>({});

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

  // The per-client bearer is the only value the gateway will route on, and it
  // lives in the keyring — there is nothing to show until the backend hands it
  // over, and no placeholder that would work in its place.
  const loadConnections = useCallback(async () => {
    const ids: SnippetClient[] = [
      "claude-desktop",
      "hermes",
      "claude-code",
      "codex-cli",
      "cursor",
    ];
    setConns(Object.fromEntries(ids.map((id) => [id, { status: "loading" } as ConnState])));
    await Promise.all(
      ids.map(async (id) => {
        try {
          const info = await backend.clientConnectionInfo(id);
          setConns((prev) => ({ ...prev, [id]: { status: "ready", info } }));
        } catch (err) {
          setConns((prev) => ({
            ...prev,
            [id]: {
              status: "unbound",
              message: err instanceof Error ? err.message : String(err),
            },
          }));
        }
      }),
    );
  }, []);

  useEffect(() => {
    fetchClients();
    loadConnections();
  }, [loadConnections]);

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

        <Button
          variant="outline"
          size="xs"
          onClick={() => {
            fetchClients();
            loadConnections();
          }}
          disabled={loading}
        >
          <RotateCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          重新扫描
        </Button>
      </div>

      {/* Clients Cards Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {clients.map((client) => {
          const Icon = clientIcon(client.id);
          return (
            <Card
              key={client.id}
              className={client.installed ? undefined : "bg-muted/30 opacity-70 shadow-none"}
            >
              <CardHeader className="p-4 pb-2.5">
                <div className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 items-center gap-2.5">
                    <div
                      className={`shrink-0 rounded-lg p-2 ${
                        client.installed
                          ? "bg-primary/10 text-primary"
                          : "bg-muted text-muted-foreground"
                      }`}
                    >
                      <Icon className="h-4 w-4" />
                    </div>
                    <CardTitle className="truncate">{client.name}</CardTitle>
                  </div>
                  {client.installed ? (
                    <Badge variant="success" className="shrink-0 text-2xs">
                      <CheckCircle2 className="mr-1 h-2.5 w-2.5" />
                      已安装
                    </Badge>
                  ) : (
                    <Badge variant="secondary" className="shrink-0 text-2xs">
                      <XCircle className="mr-1 h-2.5 w-2.5" />
                      未发现
                    </Badge>
                  )}
                </div>
              </CardHeader>
              <CardContent className="space-y-3 p-4 pt-0">
                <div className="space-y-1.5 text-xs">
                  {client.version && (
                    <div className="text-muted-foreground">
                      版本号 <span className="font-mono text-foreground">{client.version}</span>
                    </div>
                  )}
                  {client.configPath && (
                    <div>
                      <div className="text-2xs uppercase tracking-wide text-muted-foreground">
                        配置文件路径
                      </div>
                      <span
                        className="mt-0.5 block truncate rounded bg-muted px-1.5 py-0.5 font-mono text-2xs"
                        title={client.configPath}
                      >
                        {client.configPath}
                      </span>
                    </div>
                  )}
                </div>

                <div className="flex items-center justify-between gap-2 border-t pt-2.5 text-xs">
                  <span className="text-muted-foreground">
                    {client.supportsAutoConfig ? "支持一键配置同步写入" : "支持本地网关代理接入"}
                  </span>
                  {client.installed && (
                    <Badge variant="outline" className="shrink-0 text-2xs">
                      就绪
                    </Badge>
                  )}
                </div>
              </CardContent>
            </Card>
          );
        })}
      </div>

      {/* Client Configuration Snippets */}
      <div className="space-y-4 pt-4 border-t">
        <div className="flex items-center gap-2">
          <Code2 className="h-4 w-4 text-primary" />
          <h2 className="text-lg font-bold">主流客户端接入配置指南</h2>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {/* Claude Desktop */}
          <Card>
            <CardHeader className="p-4 pb-2.5">
              <div className="flex items-center justify-between gap-2">
                <CardTitle className="text-sm">Claude Desktop 官方桌面端</CardTitle>
                <Badge variant="outline" className="shrink-0 text-2xs">
                  MCP 自动同步 / 端点手填
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-2.5 p-4 pt-0">
              <p className="text-xs leading-relaxed text-muted-foreground">
                PolyDeck 只能同步 <code>claude_desktop_config.json</code> 里的 MCP 服务。网关地址与令牌要在 Desktop 的账号设置里手填（不要带 <code>/v1</code>，Anthropic SDK 会自行拼接，否则请求 <code>/v1/v1/messages</code> 并 404）。
              </p>
              <ConnectionBlock
                state={conns["claude-desktop"]}
                render={(info) => ({
                  lines: [
                    { label: "Gateway URL", value: info.baseUrl },
                    { label: "Auth Token", value: info.token, secret: true },
                  ],
                  copyText: `Gateway URL: ${info.baseUrl}\nAuth Token: Bearer ${info.token}`,
                })}
              />
            </CardContent>
          </Card>

          {/* Hermes Agent */}
          <Card>
            <CardHeader className="p-4 pb-2.5">
              <div className="flex items-center justify-between gap-2">
                <CardTitle className="text-sm">Hermes Agent / CLI</CardTitle>
                <Badge variant="outline" className="shrink-0 text-2xs">
                  Agent 终端与配置
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-2.5 p-4 pt-0">
              <p className="text-xs leading-relaxed text-muted-foreground">
                配置文件位于 <code>~/.hermes/config.yaml</code>，绑定方案时 PolyDeck 会自动写入模型与 Base URL，下面的值只是给你核对用。
              </p>
              <ConnectionBlock
                state={conns.hermes}
                render={(info) => ({
                  lines: [
                    { label: "api_base", value: `${info.baseUrl}/v1` },
                    { label: "api_key", value: info.token, secret: true },
                  ],
                  copyText: `api_base: ${info.baseUrl}/v1\napi_key: ${info.token}`,
                })}
              />
            </CardContent>
          </Card>

          {/* Cursor / VS Code (Cline / Continue) */}
          <Card>
            <CardHeader className="p-4 pb-2.5">
              <div className="flex items-center justify-between gap-2">
                <CardTitle className="text-sm">Cursor / Windsurf / VS Code</CardTitle>
                <Badge variant="outline" className="shrink-0 text-2xs">
                  OpenAI 兼容协议
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-2.5 p-4 pt-0">
              <p className="text-xs leading-relaxed text-muted-foreground">
                PolyDeck 写不了这几个客户端的配置文件，要手填。API Key 必须填下面这个令牌——网关正是靠它认出该把请求发到哪个方案，填别的会被拒。
              </p>
              <ConnectionBlock
                state={conns.cursor}
                render={(info) => ({
                  lines: [
                    { label: "Base URL", value: `${info.baseUrl}/v1` },
                    { label: "API Key", value: info.token, secret: true },
                  ],
                  copyText: `Base URL: ${info.baseUrl}/v1\nAPI Key: ${info.token}`,
                })}
              />
            </CardContent>
          </Card>

          {/* Claude Code CLI */}
          <Card>
            <CardHeader className="p-4 pb-2.5">
              <div className="flex items-center justify-between gap-2">
                <CardTitle className="text-sm">Claude Code CLI</CardTitle>
                <Badge variant="outline" className="shrink-0 text-2xs">
                  Anthropic 环境变量
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-2.5 p-4 pt-0">
              <p className="text-xs leading-relaxed text-muted-foreground">
                绑定方案时 PolyDeck 会直接写 <code>~/.claude/settings.json</code>，通常不用手动设环境变量。要临时指向别处再用这两条（不要带 <code>/v1</code>，Anthropic SDK 会自行拼接）。
              </p>
              <ConnectionBlock
                state={conns["claude-code"]}
                render={(info) => ({
                  lines: [
                    { label: "ANTHROPIC_BASE_URL", value: info.baseUrl },
                    { label: "ANTHROPIC_AUTH_TOKEN", value: info.token, secret: true },
                  ],
                  copyText: `$env:ANTHROPIC_BASE_URL="${info.baseUrl}"\n$env:ANTHROPIC_AUTH_TOKEN="${info.token}"`,
                })}
              />
            </CardContent>
          </Card>

          {/* Codex CLI */}
          <Card>
            <CardHeader className="p-4 pb-2.5">
              <div className="flex items-center justify-between gap-2">
                <CardTitle className="text-sm">Codex CLI / Agent 终端</CardTitle>
                <Badge variant="outline" className="shrink-0 text-2xs">
                  Responses 协议
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-2.5 p-4 pt-0">
              <p className="text-xs leading-relaxed text-muted-foreground">
                绑定方案时 PolyDeck 会写 <code>~/.codex/config.toml</code>，Codex CLI 与桌面端共用这一份，因此它们始终跟随同一个方案。
              </p>
              <ConnectionBlock
                state={conns["codex-cli"]}
                render={(info) => ({
                  lines: [
                    { label: "base_url", value: `${info.baseUrl}/v1` },
                    { label: "OPENAI_API_KEY", value: info.token, secret: true },
                  ],
                  copyText: `base_url = "${info.baseUrl}/v1"\nOPENAI_API_KEY=${info.token}`,
                })}
              />
            </CardContent>
          </Card>

          {/* Web UI / Chatbox / Cherry Studio */}
          <Card>
            <CardHeader className="p-4 pb-2.5">
              <div className="flex items-center justify-between gap-2">
                <CardTitle className="text-sm">Cherry Studio / Chatbox / NextChat</CardTitle>
                <Badge variant="outline" className="shrink-0 text-2xs">
                  GUI 客户端
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-2.5 p-4 pt-0">
              <p className="text-xs leading-relaxed text-muted-foreground">
                在客户端里选 OpenAI 协议，Base URL 指向本地网关。这类客户端没有独立的绑定条目，令牌沿用上面 Cursor 那一栏的值。
              </p>
              <ConnectionBlock
                state={conns.cursor}
                render={(info) => ({
                  lines: [{ label: "API Host", value: `${info.baseUrl}/v1` }],
                  copyText: `${info.baseUrl}/v1`,
                })}
              />
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}
