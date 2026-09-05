import { useEffect, useState, useMemo } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { backend } from "@/services/backend";
import type { ConsolidateReport, SessionSummary } from "@/domain/history";
import {
  History,
  Download,
  ShieldCheck,
  RotateCw,
  Search,
  MessageSquare,
  Zap,
  Clock,
  Layers,
  FolderOpen,
  Combine,
  Server,
  CheckCircle2,
  AlertCircle,
} from "lucide-react";

export default function HistoryPage() {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchTerm, setSearchTerm] = useState("");
  const [selectedClient, setSelectedClient] = useState<string>("all");
  const [consolidating, setConsolidating] = useState(false);
  const [consolidateResult, setConsolidateResult] = useState<ConsolidateReport | null>(null);
  const [historyError, setHistoryError] = useState<string | null>(null);

  // Backup modal state
  const [showBackupModal, setShowBackupModal] = useState(false);
  const [backupPassword, setBackupPassword] = useState("");
  const [backupResult, setBackupResult] = useState<string | null>(null);
  const [backingUp, setBackingUp] = useState(false);

  // Restore modal state
  const [showRestoreModal, setShowRestoreModal] = useState(false);
  const [restorePath, setRestorePath] = useState("");
  const [restorePassword, setRestorePassword] = useState("");
  const [restoring, setRestoring] = useState(false);
  const [restoreMessage, setRestoreMessage] = useState<{ success: boolean; text: string } | null>(null);

  const loadHistory = async () => {
    setLoading(true);
    try {
      setSessions(await backend.queryHistory());
      setHistoryError(null);
    } catch (err) {
      setHistoryError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadHistory();
  }, []);

  const handleExportJson = async () => {
    try {
      const jsonStr = await backend.exportHistory("json");
      const blob = new Blob([jsonStr || JSON.stringify(sessions, null, 2)], {
        type: "application/json",
      });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `ai-deck-history-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch {
      const blob = new Blob([JSON.stringify(sessions, null, 2)], {
        type: "application/json",
      });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `ai-deck-history-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
    }
  };

  const handleCreateBackup = async () => {
    setBackingUp(true);
    setBackupResult(null);
    try {
      const path = await backend.createEncryptedBackup(backupPassword);
      setBackupResult(path || "已成功生成加密备份文件");
    } catch (err) {
      alert(`创建备份失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setBackingUp(false);
    }
  };

  const handleRestoreBackup = async () => {
    if (!restorePath.trim()) return;
    setRestoring(true);
    setRestoreMessage(null);
    try {
      await backend.restoreEncryptedBackup(restorePath, restorePassword);
      setRestoreMessage({ success: true, text: "备份还原成功，历史会话已更新" });
      await loadHistory();
    } catch (err) {
      setRestoreMessage({
        success: false,
        text: err instanceof Error ? err.message : "还原失败，密码不匹配或备份文件已损坏",
      });
    } finally {
      setRestoring(false);
    }
  };

  const handleConsolidate = async () => {
    setConsolidating(true);
    setConsolidateResult(null);
    try {
      // Re-index first: a conversation whose file exists but was never indexed is
      // not a duplicate to merge, it is simply missing, and merging alone would not
      // bring it back.
      await backend.syncHistory();
      setConsolidateResult(await backend.consolidateHistory());
      await loadHistory();
      setHistoryError(null);
    } catch (err) {
      setHistoryError(err instanceof Error ? err.message : String(err));
    } finally {
      setConsolidating(false);
    }
  };

  const filteredSessions = useMemo(() => {
    return sessions.filter((s) => {
      const matchSearch =
        s.title.toLowerCase().includes(searchTerm.toLowerCase()) ||
        s.client.toLowerCase().includes(searchTerm.toLowerCase());
      const matchClient = selectedClient === "all" || s.client === selectedClient;
      return matchSearch && matchClient;
    });
  }, [sessions, searchTerm, selectedClient]);

  const uniqueClients = useMemo(() => {
    return Array.from(new Set(sessions.map((s) => s.client)));
  }, [sessions]);

  const totalMessages = sessions.reduce((acc, s) => acc + (s.messageCount || 0), 0);
  const totalTokens = sessions.reduce((acc, s) => acc + (s.totalTokens || 0), 0);

  return (
    <div className="space-y-8 max-w-6xl mx-auto pb-12">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <Badge variant="info" className="px-3 py-1">
              <History className="h-3 w-3 mr-1" />
              历史归档
            </Badge>
            <span className="text-xs text-muted-foreground">共记录 {sessions.length} 个本地开发会话</span>
          </div>
          <h1 className="text-3xl font-extrabold tracking-tight mt-1">会话历史与安全备份</h1>
          <p className="text-muted-foreground text-sm">
            集中归档各 AI 客户端请求记录，支持 XChaCha20-Poly1305 端到端硬件级加密备份与还原。
          </p>
        </div>

        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" onClick={handleExportJson} className="text-xs">
            <Download className="h-3.5 w-3.5 mr-1" />
            导出 JSON
          </Button>
          <Button variant="outline" size="sm" onClick={() => setShowBackupModal(true)} className="text-xs">
            <ShieldCheck className="h-3.5 w-3.5 mr-1 text-emerald-500" />
            加密备份
          </Button>
          <Button variant="outline" size="sm" onClick={() => setShowRestoreModal(true)} className="text-xs">
            <FolderOpen className="h-3.5 w-3.5 mr-1" />
            还原备份
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={handleConsolidate}
            disabled={consolidating}
            className="text-xs"
            title="重新索引会话文件，并把同一会话在不同 id 方案下的重复记录合并为一条"
          >
            <Combine className={`h-3.5 w-3.5 mr-1 ${consolidating ? "animate-spin" : ""}`} />
            {consolidating ? "整合中…" : "整合会话"}
          </Button>
          <Button variant="ghost" size="sm" onClick={loadHistory} disabled={loading} className="text-xs">
            <RotateCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          </Button>
        </div>
      </div>

      {/* Stats Cards */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <Card className="border-border/60 shadow-sm">
          <CardContent className="p-4 flex items-center gap-4">
            <div className="p-3 rounded-xl bg-primary/10 text-primary">
              <MessageSquare className="h-5 w-5" />
            </div>
            <div>
              <div className="text-xs text-muted-foreground">会话总数</div>
              <div className="text-2xl font-bold">{sessions.length}</div>
            </div>
          </CardContent>
        </Card>

        <Card className="border-border/60 shadow-sm">
          <CardContent className="p-4 flex items-center gap-4">
            <div className="p-3 rounded-xl bg-sky-500/10 text-sky-500">
              <Layers className="h-5 w-5" />
            </div>
            <div>
              <div className="text-xs text-muted-foreground">累计交互轮次</div>
              <div className="text-2xl font-bold">{totalMessages.toLocaleString()}</div>
            </div>
          </CardContent>
        </Card>

        <Card className="border-border/60 shadow-sm">
          <CardContent className="p-4 flex items-center gap-4">
            <div className="p-3 rounded-xl bg-amber-500/10 text-amber-500">
              <Zap className="h-5 w-5" />
            </div>
            <div>
              <div className="text-xs text-muted-foreground">消耗 Tokens 估算</div>
              <div className="text-2xl font-bold">{totalTokens.toLocaleString()}</div>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Filter and Search */}
      <div className="flex flex-col sm:flex-row gap-3 items-center justify-between">
        <div className="relative w-full sm:w-80">
          <Search className="h-4 w-4 absolute left-3 top-2.5 text-muted-foreground" />
          <Input
            value={searchTerm}
            onChange={(e) => setSearchTerm(e.target.value)}
            placeholder="搜索会话标题或内容..."
            className="text-xs pl-9"
          />
        </div>

        {uniqueClients.length > 0 && (
          <div className="flex items-center gap-2 overflow-x-auto w-full sm:w-auto">
            <span className="text-xs text-muted-foreground shrink-0">客户端筛选:</span>
            <Button
              variant={selectedClient === "all" ? "default" : "outline"}
              size="sm"
              onClick={() => setSelectedClient("all")}
              className="text-xs h-7"
            >
              全部
            </Button>
            {uniqueClients.map((c) => (
              <Button
                key={c}
                variant={selectedClient === c ? "default" : "outline"}
                size="sm"
                onClick={() => setSelectedClient(c)}
                className="text-xs h-7"
              >
                {c}
              </Button>
            ))}
          </div>
        )}
      </div>

      {historyError && (
        <div className="p-3 rounded-lg border border-destructive/40 bg-destructive/5 text-xs text-destructive" role="alert">
          读取或整合历史失败：{historyError}
        </div>
      )}

      {consolidateResult && (
        <div className="p-3 rounded-lg border border-emerald-500/40 bg-emerald-500/5 text-xs space-y-1">
          <div className="flex items-center gap-2 font-semibold text-emerald-600 dark:text-emerald-400">
            <CheckCircle2 className="h-3.5 w-3.5" />
            整合完成，当前共 {consolidateResult.sessionsAfter} 个会话
          </div>
          <div className="text-muted-foreground">
            {consolidateResult.duplicatesMerged > 0
              ? `合并重复记录 ${consolidateResult.duplicatesMerged} 条`
              : "没有发现重复记录"}
            {consolidateResult.clientsNormalized > 0 &&
              `，统一客户端名称 ${consolidateResult.clientsNormalized} 条`}
            {consolidateResult.timestampsNormalized > 0 &&
              `，修正时间格式 ${consolidateResult.timestampsNormalized} 条`}
          </div>
        </div>
      )}

      {/* Session List */}
      <Card className="border-border/60 shadow-sm">
        <CardHeader className="pb-3">
          <CardTitle className="text-base font-semibold">历史记录列表</CardTitle>
        </CardHeader>
        <CardContent>
          {filteredSessions.length === 0 ? (
            <div className="py-12 text-center text-muted-foreground text-xs space-y-2">
              <History className="h-8 w-8 mx-auto text-muted-foreground/40" />
              <p>暂无符合条件的会话记录</p>
              <p className="text-[11px] opacity-70">当您通过本地网关向大模型发起请求或使用各客户端时，会话将自动同步在此处。</p>
            </div>
          ) : (
            <div className="divide-y divide-border/40">
              {filteredSessions.map((s) => (
                <div key={s.id} className="py-3 flex items-center justify-between gap-4 hover:bg-muted/20 px-2 rounded-lg transition-colors">
                  <div className="space-y-1 min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <Badge variant="outline" className="text-[10px] shrink-0 font-mono">{s.client}</Badge>
                      <span className="text-xs font-semibold truncate text-foreground">{s.title || "未命名会话"}</span>
                    </div>
                    <div className="flex items-center gap-3 text-[11px] text-muted-foreground">
                      <span className="flex items-center gap-1">
                        <Clock className="h-3 w-3" />
                        {new Date(s.updatedAt || s.createdAt).toLocaleString()}
                      </span>
                      <span>·</span>
                      <span>{s.messageCount} 消息</span>
                      <span>·</span>
                      <span>{s.totalTokens.toLocaleString()} tokens</span>
                      {s.providerId && (
                        <>
                          <span>·</span>
                          <span className="flex items-center gap-1" title={`Provider: ${s.providerId}`}>
                            <Server className="h-3 w-3" />
                            {s.providerId}
                          </span>
                        </>
                      )}
                      {s.mergedFrom > 1 && (
                        <>
                          <span>·</span>
                          <span
                            className="flex items-center gap-1 text-sky-500"
                            title="该会话由多份不同 id 方案的记录合并而来"
                          >
                            <Combine className="h-3 w-3" />
                            合并 {s.mergedFrom} 份
                          </span>
                        </>
                      )}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Backup Modal Dialog */}
      {showBackupModal && (
        <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-4">
          <Card className="w-full max-w-md border-border bg-card shadow-2xl">
            <CardHeader className="pb-3">
              <div className="flex items-center gap-2">
                <ShieldCheck className="h-5 w-5 text-emerald-500" />
                <CardTitle className="text-base font-bold">创建 XChaCha20-Poly1305 加密备份</CardTitle>
              </div>
            </CardHeader>
            <CardContent className="space-y-4 text-xs">
              <p className="text-muted-foreground leading-relaxed">
                备份文件使用高强度对称加密，即使云同步存储亦能确保隐私安全。
              </p>
              <div>
                <label className="text-xs font-medium block mb-1">备份独立密码 (可选，默认使用系统安全区密钥)</label>
                <Input
                  type="password"
                  value={backupPassword}
                  onChange={(e) => setBackupPassword(e.target.value)}
                  placeholder="输入保护密码..."
                  className="text-xs"
                />
              </div>

              {backupResult && (
                <div className="p-3 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 rounded text-xs flex items-start gap-2">
                  <CheckCircle2 className="h-4 w-4 shrink-0 mt-0.5" />
                  <div className="font-mono break-all">{backupResult}</div>
                </div>
              )}

              <div className="flex justify-end gap-2 pt-2">
                <Button variant="ghost" size="sm" onClick={() => setShowBackupModal(false)}>
                  关闭
                </Button>
                <Button size="sm" onClick={handleCreateBackup} disabled={backingUp}>
                  {backingUp ? "正在加密打包..." : "立即创建备份"}
                </Button>
              </div>
            </CardContent>
          </Card>
        </div>
      )}

      {/* Restore Modal Dialog */}
      {showRestoreModal && (
        <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-4">
          <Card className="w-full max-w-md border-border bg-card shadow-2xl">
            <CardHeader className="pb-3">
              <div className="flex items-center gap-2">
                <FolderOpen className="h-5 w-5 text-primary" />
                <CardTitle className="text-base font-bold">还原历史会话备份</CardTitle>
              </div>
            </CardHeader>
            <CardContent className="space-y-4 text-xs">
              <div>
                <label className="text-xs font-medium block mb-1">备份文件路径 (.history-backup / .json)</label>
                <Input
                  value={restorePath}
                  onChange={(e) => setRestorePath(e.target.value)}
                  placeholder="C:\Users\...\ai-deck.history-backup"
                  className="text-xs font-mono"
                />
              </div>
              <div>
                <label className="text-xs font-medium block mb-1">解密密码 (如创建时设置)</label>
                <Input
                  type="password"
                  value={restorePassword}
                  onChange={(e) => setRestorePassword(e.target.value)}
                  placeholder="输入解密密码..."
                  className="text-xs"
                />
              </div>

              {restoreMessage && (
                <div
                  className={`p-3 rounded text-xs flex items-start gap-2 ${
                    restoreMessage.success
                      ? "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                      : "bg-destructive/10 text-destructive"
                  }`}
                >
                  {restoreMessage.success ? (
                    <CheckCircle2 className="h-4 w-4 shrink-0 mt-0.5" />
                  ) : (
                    <AlertCircle className="h-4 w-4 shrink-0 mt-0.5" />
                  )}
                  <div>{restoreMessage.text}</div>
                </div>
              )}

              <div className="flex justify-end gap-2 pt-2">
                <Button variant="ghost" size="sm" onClick={() => setShowRestoreModal(false)}>
                  关闭
                </Button>
                <Button size="sm" onClick={handleRestoreBackup} disabled={restoring || !restorePath.trim()}>
                  {restoring ? "正在解密恢复..." : "开始还原"}
                </Button>
              </div>
            </CardContent>
          </Card>
        </div>
      )}
    </div>
  );
}
