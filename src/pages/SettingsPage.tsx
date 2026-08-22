import { useEffect, useState } from "react";
import { ThemeToggle } from "@/components/ThemeToggle";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { backend } from "@/services/backend";
import { useAtom } from "jotai";
import { themeAtom } from "@/state/theme";
import type { DiagnosticReport, UpdateInfo, AutoLaunchStatus } from "@/domain/ops";
import type { ProxyStatus } from "@/domain/proxy";
import {
  Settings,
  Sun,
  Moon,
  Laptop,
  Power,
  Activity,
  RotateCw,
  CheckCircle2,
  AlertTriangle,
  XCircle,
  FileCode,
  Globe,
  DownloadCloud,
  ArrowDownToLine,
  Check,
} from "lucide-react";

export default function SettingsPage() {
  const [theme, setTheme] = useAtom(themeAtom);
  const [version, setVersion] = useState("2.0.0");
  const [autolaunch, setAutolaunch] = useState<AutoLaunchStatus | null>(null);
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus | null>(null);
  const [diagnostics, setDiagnostics] = useState<DiagnosticReport | null>(null);
  const [runningDiag, setRunningDiag] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [showLogs, setShowLogs] = useState(false);
  const [loadingLogs, setLoadingLogs] = useState(false);
  const [importable, setImportable] = useState<string[]>([]);
  const [importing, setImporting] = useState(false);
  const [importSuccess, setImportSuccess] = useState(false);

  useEffect(() => {
    backend.getVersion().then(setVersion).catch(() => {});
    backend.autolaunchStatus().then(setAutolaunch).catch(() => {});
    backend.detectProxy().then(setProxyStatus).catch(() => {});
    backend.detectImportable().then(setImportable).catch(() => []);
  }, []);

  const handleToggleAutolaunch = async () => {
    if (!autolaunch) return;
    const nextState = !autolaunch.enabled;
    try {
      await backend.setAutolaunch(nextState);
      setAutolaunch({ ...autolaunch, enabled: nextState });
    } catch (err) {
      alert(`设置开机自启失败: ${err instanceof Error ? err.message : String(err)}`);
    }
  };

  const handleRunDiagnostics = async () => {
    setRunningDiag(true);
    try {
      const report = await backend.runDiagnostics();
      setDiagnostics(report);
    } catch (err) {
      alert(`诊断执行失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setRunningDiag(false);
    }
  };

  const handleCheckUpdate = async () => {
    setCheckingUpdate(true);
    try {
      const res = await backend.checkUpdate();
      setUpdateInfo(res);
    } catch (err) {
      alert(`检查更新失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setCheckingUpdate(false);
    }
  };

  const handleLoadLogs = async () => {
    setLoadingLogs(true);
    try {
      const data = await backend.getLogs(100);
      setLogs(data);
      setShowLogs(true);
    } catch (err) {
      console.error("Failed to get logs:", err);
    } finally {
      setLoadingLogs(false);
    }
  };

  const handleImport = async (path: string) => {
    setImporting(true);
    setImportSuccess(false);
    try {
      await backend.importFromProviderDeck(path);
      setImportSuccess(true);
    } catch (err) {
      alert(`导入失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setImporting(false);
    }
  };

  return (
    <div className="space-y-8 max-w-5xl mx-auto pb-12">
      {/* Header */}
      <div>
        <div className="flex items-center gap-2">
          <Badge variant="info" className="px-3 py-1">
            <Settings className="h-3 w-3 mr-1" />
            偏好与维护
          </Badge>
          <span className="text-xs text-muted-foreground">PolyDeck v{version}</span>
        </div>
        <h1 className="text-3xl font-extrabold tracking-tight mt-1">系统设置与诊断</h1>
        <p className="text-muted-foreground text-sm">
          管理系统外观主题、开机自启、本地代理联动感知、Provider Doctor 深度诊断与运行日志。
        </p>
      </div>

      {/* Grid of Settings Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Appearance Card */}
        <Card className="border-border/60 shadow-sm">
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <CardTitle className="text-base font-semibold">外观与显示主题</CardTitle>
              <ThemeToggle />
            </div>
          </CardHeader>
          <CardContent className="space-y-3 text-xs">
            <p className="text-muted-foreground">
              选择适合您的色彩模式，支持随系统深浅色自动切换。
            </p>
            <div className="grid grid-cols-3 gap-2">
              <Button
                variant={theme === "light" ? "default" : "outline"}
                size="sm"
                className="text-xs"
                onClick={() => setTheme("light")}
              >
                <Sun className="h-3.5 w-3.5 mr-1" /> 明亮模式
              </Button>
              <Button
                variant={theme === "dark" ? "default" : "outline"}
                size="sm"
                className="text-xs"
                onClick={() => setTheme("dark")}
              >
                <Moon className="h-3.5 w-3.5 mr-1" /> 暗黑模式
              </Button>
              <Button
                variant={theme === "system" ? "default" : "outline"}
                size="sm"
                className="text-xs"
                onClick={() => setTheme("system")}
              >
                <Laptop className="h-3.5 w-3.5 mr-1" /> 跟随系统
              </Button>
            </div>
          </CardContent>
        </Card>

        {/* Autolaunch Card */}
        <Card className="border-border/60 shadow-sm">
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <CardTitle className="text-base font-semibold">开机启动与后台托管</CardTitle>
              {autolaunch && (
                <Badge variant={autolaunch.enabled ? "success" : "secondary"}>
                  {autolaunch.enabled ? "已启用" : "已关闭"}
                </Badge>
              )}
            </div>
          </CardHeader>
          <CardContent className="space-y-3 text-xs">
            <p className="text-muted-foreground">
              系统登录时自动启动并在后台托盘静默运行网关服务。
            </p>
            <Button
              variant={autolaunch?.enabled ? "outline" : "default"}
              size="sm"
              onClick={handleToggleAutolaunch}
              className="text-xs w-full"
            >
              <Power className="h-3.5 w-3.5 mr-1.5" />
              {autolaunch?.enabled ? "禁用开机自启" : "启用开机自启"}
            </Button>
          </CardContent>
        </Card>

        {/* Proxy Manager Card */}
        <Card className="border-border/60 shadow-sm">
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Globe className="h-4 w-4 text-primary" />
                <CardTitle className="text-base font-semibold">本地网络代理感知</CardTitle>
              </div>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 text-xs"
                onClick={() => backend.detectProxy().then(setProxyStatus)}
              >
                <RotateCw className="h-3 w-3 mr-1" /> 刷新
              </Button>
            </div>
          </CardHeader>
          <CardContent className="space-y-3 text-xs">
            <p className="text-muted-foreground">
              感知 Clash / Mihomo / Sing-box / V2Ray 等代理客户端端口，保障上游大模型连接稳定性。
            </p>
            <div className="space-y-1.5">
              {(!proxyStatus || !proxyStatus.tools || proxyStatus.tools.length === 0) ? (
                <div className="p-2 bg-muted/30 rounded text-muted-foreground text-[11px]">
                  未检测到运行中的本地代理软件（直连模式）
                </div>
              ) : (
                proxyStatus.tools.map((t) => (
                  <div key={t.name} className="p-2 bg-muted/40 rounded flex items-center justify-between">
                    <span className="font-semibold text-xs">{t.name}</span>
                    <div className="flex items-center gap-2">
                      {t.port && <span className="font-mono text-[10px]">端口: {t.port}</span>}
                      <Badge variant={t.running ? "success" : "secondary"} className="text-[10px]">
                        {t.running ? "运行中" : "未运行"}
                      </Badge>
                    </div>
                  </div>
                ))
              )}
            </div>
          </CardContent>
        </Card>

        {/* Update & Logs Card */}
        <Card className="border-border/60 shadow-sm">
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <DownloadCloud className="h-4 w-4 text-primary" />
                <CardTitle className="text-base font-semibold">版本与维护</CardTitle>
              </div>
              <Badge variant="outline" className="font-mono text-[10px]">v{version}</Badge>
            </div>
          </CardHeader>
          <CardContent className="space-y-3 text-xs">
            <div className="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={handleCheckUpdate}
                disabled={checkingUpdate}
                className="text-xs flex-1"
              >
                <DownloadCloud className={`h-3.5 w-3.5 mr-1 ${checkingUpdate ? "animate-spin" : ""}`} />
                检查最新版本
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={handleLoadLogs}
                disabled={loadingLogs}
                className="text-xs flex-1"
              >
                <FileCode className="h-3.5 w-3.5 mr-1" />
                查看运行日志
              </Button>
            </div>

            {updateInfo && (
              <div className="p-2 bg-muted/40 rounded text-xs flex items-center gap-2">
                {updateInfo.available ? (
                  <>
                    <AlertTriangle className="h-3.5 w-3.5 text-amber-500" />
                    <span>发现新版本: <b>{updateInfo.version}</b></span>
                  </>
                ) : (
                  <>
                    <CheckCircle2 className="h-3.5 w-3.5 text-emerald-500" />
                    <span>已是最新稳定版本 (v{version})</span>
                  </>
                )}
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Provider Deck Migration Card */}
      {importable.length > 0 && (
        <Card className="border-border/60 shadow-sm">
          <CardHeader className="pb-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <ArrowDownToLine className="h-4 w-4 text-primary" />
                <CardTitle className="text-base font-semibold">旧版 Provider Deck 配置迁移</CardTitle>
              </div>
              <Badge variant="info">发现旧版配置</Badge>
            </div>
          </CardHeader>
          <CardContent className="space-y-3 text-xs">
            <p className="text-muted-foreground">
              检测到本地存在旧版配置，支持一键无缝导入至 PolyDeck v2.0.3。
            </p>
            <div className="flex items-center gap-3">
              <Button
                size="sm"
                onClick={() => handleImport(importable[0])}
                disabled={importing}
                className="text-xs"
              >
                {importing ? "正在迁移导入..." : "一键导入全部配置"}
              </Button>
              {importSuccess && (
                <span className="text-emerald-500 flex items-center gap-1">
                  <Check className="h-3.5 w-3.5" /> 导入成功！
                </span>
              )}
            </div>
          </CardContent>
        </Card>
      )}

      {/* Provider Doctor Diagnostics Section */}
      <Card className="border-border/60 shadow-sm">
        <CardHeader className="pb-3 border-b">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <Activity className="h-5 w-5 text-primary" />
              <div>
                <CardTitle className="text-base font-semibold">Provider Doctor 智能诊断体系</CardTitle>
                <p className="text-xs text-muted-foreground">
                  全自动检查 API 凭据、网络上行延迟、协议兼容性与网关端口状态。
                </p>
              </div>
            </div>
            <Button
              size="sm"
              onClick={handleRunDiagnostics}
              disabled={runningDiag}
              className="text-xs shrink-0"
            >
              <Activity className={`h-3.5 w-3.5 mr-1.5 ${runningDiag ? "animate-spin" : ""}`} />
              {runningDiag ? "正在执行诊断..." : "运行全套诊断"}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="p-6">
          {!diagnostics ? (
            <div className="text-center py-6 text-xs text-muted-foreground">
              点击右上角“运行全套诊断”排查大模型连接与网关状态。
            </div>
          ) : (
            <div className="space-y-4">
              <div className="flex items-center gap-4 text-xs">
                <div className="flex items-center gap-1 text-emerald-500 font-semibold">
                  <CheckCircle2 className="h-4 w-4" /> 正常: {diagnostics.okCount} 项
                </div>
                <div className="flex items-center gap-1 text-amber-500 font-semibold">
                  <AlertTriangle className="h-4 w-4" /> 警告: {diagnostics.warnings} 项
                </div>
                <div className="flex items-center gap-1 text-destructive font-semibold">
                  <XCircle className="h-4 w-4" /> 异常: {diagnostics.errors} 项
                </div>
              </div>

              <div className="space-y-2 pt-2">
                {diagnostics.items.map((item, idx) => (
                  <div
                    key={idx}
                    className={`p-3 rounded-lg border text-xs space-y-1 ${
                      item.level === "error"
                        ? "bg-destructive/5 border-destructive/30 text-destructive"
                        : item.level === "warning"
                        ? "bg-amber-500/5 border-amber-500/30 text-amber-600 dark:text-amber-400"
                        : "bg-emerald-500/5 border-emerald-500/30 text-foreground"
                    }`}
                  >
                    <div className="flex items-center justify-between font-semibold">
                      <span>[{item.category}] {item.message}</span>
                      <Badge
                        variant={item.level === "error" ? "destructive" : item.level === "warning" ? "warning" : "success"}
                        className="text-[10px]"
                      >
                        {item.level.toUpperCase()}
                      </Badge>
                    </div>
                    {item.impact && <p className="text-[11px] opacity-80">影响: {item.impact}</p>}
                    {item.suggestion && (
                      <p className="text-[11px] opacity-90 font-medium">建议: {item.suggestion}</p>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Logs Viewer Modal / Box */}
      {showLogs && (
        <Card className="border-border/60 shadow-sm">
          <CardHeader className="pb-2 flex flex-row items-center justify-between">
            <CardTitle className="text-sm font-mono">系统运行日志 (最新 {logs.length} 条)</CardTitle>
            <Button variant="ghost" size="sm" onClick={() => setShowLogs(false)} className="text-xs h-7">
              收起日志
            </Button>
          </CardHeader>
          <CardContent>
            <div className="bg-muted/80 p-3 rounded-lg font-mono text-[11px] max-h-60 overflow-y-auto space-y-1">
              {logs.length === 0 ? (
                <div className="text-muted-foreground text-center py-4">暂无日志记录</div>
              ) : (
                logs.map((line, idx) => (
                  <div key={idx} className="leading-tight text-foreground/90">{line}</div>
                ))
              )}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
