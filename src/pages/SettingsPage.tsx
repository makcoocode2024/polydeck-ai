import { useEffect, useState } from "react";
import { ThemeToggle } from "@/components/ThemeToggle";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { backend } from "@/services/backend";
import { useAtom } from "jotai";
import { themeAtom } from "@/state/theme";
import type { DiagnosticReport, UpdateInfo, AutoLaunchStatus, ClientRuleStatus, LogEntry } from "@/domain/ops";
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
  Languages,
  ShieldCheck,
} from "lucide-react";

export default function SettingsPage() {
  const [theme, setTheme] = useAtom(themeAtom);
  // Filled by ad_get_version; no baked-in number to drift from Cargo.toml.
  const [version, setVersion] = useState("");
  const [autolaunch, setAutolaunch] = useState<AutoLaunchStatus | null>(null);
  const [autolaunchError, setAutolaunchError] = useState<string | null>(null);
  const [savingAutolaunch, setSavingAutolaunch] = useState(false);
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus | null>(null);
  const [diagnostics, setDiagnostics] = useState<DiagnosticReport | null>(null);
  const [runningDiag, setRunningDiag] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [logsError, setLogsError] = useState<string | null>(null);
  const [showLogs, setShowLogs] = useState(false);
  const [loadingLogs, setLoadingLogs] = useState(false);
  const [importable, setImportable] = useState<string[]>([]);
  const [importing, setImporting] = useState(false);
  const [importSuccess, setImportSuccess] = useState(false);
  const [forceChinese, setForceChinese] = useState<ClientRuleStatus | null>(null);
  const [forceChineseError, setForceChineseError] = useState<string | null>(null);
  const [savingForceChinese, setSavingForceChinese] = useState(false);
  const [toolTruth, setToolTruth] = useState<ClientRuleStatus | null>(null);
  const [toolTruthError, setToolTruthError] = useState<string | null>(null);
  const [savingToolTruth, setSavingToolTruth] = useState(false);

  useEffect(() => {
    backend.getVersion().then(setVersion).catch(() => {});
    backend
      .autolaunchStatus()
      .then((status) => {
        setAutolaunch(status);
        setAutolaunchError(null);
      })
      .catch((err) =>
        setAutolaunchError(err instanceof Error ? err.message : String(err)),
      );
    backend.detectProxy().then(setProxyStatus).catch(() => {});
    backend.detectImportable().then(setImportable).catch(() => []);
    // The toggle stays disabled until this resolves, so a swallowed failure
    // would leave it dead with nothing on screen to explain why.
    backend
      .forceChineseStatus()
      .then((status) => {
        setForceChinese(status);
        setForceChineseError(null);
      })
      .catch((err) =>
        setForceChineseError(err instanceof Error ? err.message : String(err)),
      );
    backend
      .toolTruthfulnessStatus()
      .then((status) => {
        setToolTruth(status);
        setToolTruthError(null);
      })
      .catch((err) =>
        setToolTruthError(err instanceof Error ? err.message : String(err)),
      );
  }, []);

  const handleToggleForceChinese = async (next: boolean) => {
    setSavingForceChinese(true);
    try {
      setForceChinese(await backend.setForceChinese(next));
      setForceChineseError(null);
    } catch (err) {
      setForceChineseError(err instanceof Error ? err.message : String(err));
    } finally {
      setSavingForceChinese(false);
    }
  };

  const handleToggleToolTruth = async (next: boolean) => {
    setSavingToolTruth(true);
    try {
      setToolTruth(await backend.setToolTruthfulness(next));
      setToolTruthError(null);
    } catch (err) {
      setToolTruthError(err instanceof Error ? err.message : String(err));
    } finally {
      setSavingToolTruth(false);
    }
  };

  const handleToggleAutolaunch = async () => {
    if (!autolaunch || !autolaunch.supported) return;
    setSavingAutolaunch(true);
    try {
      await backend.setAutolaunch(!autolaunch.enabled);
      // Re-read rather than assuming the write took: the previous version set the
      // toggle optimistically over a backend that only logged, so it showed
      // "enabled" until the next restart proved otherwise.
      setAutolaunch(await backend.autolaunchStatus());
      setAutolaunchError(null);
    } catch (err) {
      setAutolaunchError(err instanceof Error ? err.message : String(err));
    } finally {
      setSavingAutolaunch(false);
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
      setLogs(await backend.getLogs(100));
      setLogsError(null);
      setShowLogs(true);
    } catch (err) {
      // The log view is the thing users open when something else already broke,
      // so a silent console.error was the wrong place for this.
      setLogsError(err instanceof Error ? err.message : String(err));
      setShowLogs(true);
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
                <Badge
                  variant={
                    !autolaunch.supported
                      ? "secondary"
                      : autolaunch.enabled
                        ? "success"
                        : "secondary"
                  }
                >
                  {!autolaunch.supported
                    ? "当前平台不支持"
                    : autolaunch.enabled
                      ? "已启用"
                      : "已关闭"}
                </Badge>
              )}
            </div>
          </CardHeader>
          <CardContent className="space-y-3 text-xs">
            <p className="text-muted-foreground">
              系统登录时自动启动并在后台托盘静默运行网关服务。
            </p>
            {autolaunch?.command && (
              <p className="text-muted-foreground font-mono text-[11px] break-all">
                登录时执行：{autolaunch.command}
              </p>
            )}
            {autolaunchError && (
              <p className="text-destructive" role="alert">
                {autolaunchError}
              </p>
            )}
            <Button
              variant={autolaunch?.enabled ? "outline" : "default"}
              size="sm"
              onClick={handleToggleAutolaunch}
              disabled={!autolaunch?.supported || savingAutolaunch}
              className="text-xs w-full"
            >
              <Power className="h-3.5 w-3.5 mr-1.5" />
              {!autolaunch?.supported
                ? "当前平台暂不支持"
                : savingAutolaunch
                  ? "正在写入…"
                  : autolaunch?.enabled
                    ? "禁用开机自启"
                    : "启用开机自启"}
            </Button>
          </CardContent>
        </Card>

        {/* Forced Chinese Output Card */}
        <Card className="border-border/60 shadow-sm" data-testid="force-chinese-card">
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Languages className="h-4 w-4 text-primary" />
                <CardTitle className="text-base font-semibold">强制中文输出</CardTitle>
              </div>
              {forceChineseError ? (
                <Badge variant="destructive">不可用</Badge>
              ) : forceChinese ? (
                <Badge variant={forceChinese.enabled ? "success" : "secondary"}>
                  {forceChinese.enabled ? "已启用" : "已关闭"}
                </Badge>
              ) : (
                <Badge variant="secondary">读取中</Badge>
              )}
            </div>
          </CardHeader>
          <CardContent className="space-y-3 text-xs">
            <label className="flex items-start gap-3 p-3.5 rounded-lg border bg-muted/10 hover:bg-muted/20 cursor-pointer transition-all">
              <input
                type="checkbox"
                checked={forceChinese?.enabled ?? false}
                disabled={!forceChinese || savingForceChinese}
                onChange={(e) => handleToggleForceChinese(e.target.checked)}
                className="mt-0.5 rounded border-input text-primary focus:ring-primary h-4 w-4"
                data-testid="force-chinese-toggle"
              />
              <div className="space-y-0.5 flex-1">
                <div className="text-xs font-medium text-foreground">
                  向客户端全局指令文件写入中文输出约束
                </div>
                <p className="text-[11px] text-muted-foreground">
                  规则写在标记块内，块外的内容不会被改动；关闭时只移除该块。代码、报错和标识符仍保留英文。
                </p>
              </div>
            </label>

            {forceChineseError && (
              <div
                className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 space-y-1"
                data-testid="force-chinese-error"
              >
                <p className="text-[11px] text-destructive flex items-start gap-1">
                  <XCircle className="h-3 w-3 mt-0.5 shrink-0" />
                  <span>读取失败：{forceChineseError}</span>
                </p>
                <p className="text-[11px] text-muted-foreground">
                  若提示命令不存在，说明当前运行的是旧构建。执行 build_release.bat
                  重新生成生产构建后重启即可。
                </p>
              </div>
            )}

            {forceChinese?.targets.map((t) => (
              <div key={t.path} className="space-y-1">
                <div className="flex items-center justify-between gap-2">
                  <span className="font-medium text-foreground">{t.target}</span>
                  {t.error ? (
                    <Badge variant="destructive">写入失败</Badge>
                  ) : (
                    <Badge variant={t.rulePresent ? "success" : "secondary"}>
                      {t.rulePresent ? "规则已写入" : "未写入"}
                    </Badge>
                  )}
                </div>
                <p className="text-[11px] text-muted-foreground font-mono break-all">{t.path}</p>
                {t.error && (
                  <p className="text-[11px] text-destructive flex items-start gap-1">
                    <XCircle className="h-3 w-3 mt-0.5 shrink-0" />
                    {t.error}
                  </p>
                )}
                {t.shadowedBy && (
                  <p className="text-[11px] text-amber-600 dark:text-amber-500 flex items-start gap-1">
                    <AlertTriangle className="h-3 w-3 mt-0.5 shrink-0" />
                    <span>
                      该文件被 <span className="font-mono break-all">{t.shadowedBy}</span>{" "}
                      抢先读取，规则不会生效。删除或清空该文件后才会读到这里。
                    </span>
                  </p>
                )}
              </div>
            ))}

            <p className="text-[11px] text-muted-foreground">
              客户端每次新开会话才读取指令文件，已经打开的会话需要重启才会生效。
            </p>
          </CardContent>
        </Card>

        {/* Tool Execution Truthfulness Card */}
        <Card className="border-border/60 shadow-sm" data-testid="tool-truthfulness-card">
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <ShieldCheck className="h-4 w-4 text-primary" />
                <CardTitle className="text-base font-semibold">工具执行真实性检查</CardTitle>
              </div>
              {toolTruthError ? (
                <Badge variant="destructive">不可用</Badge>
              ) : toolTruth ? (
                <Badge variant={toolTruth.enabled ? "success" : "secondary"}>
                  {toolTruth.enabled ? "已启用" : "已关闭"}
                </Badge>
              ) : (
                <Badge variant="secondary">读取中</Badge>
              )}
            </div>
          </CardHeader>
          <CardContent className="space-y-3 text-xs">
            <label className="flex items-start gap-3 p-3.5 rounded-lg border bg-muted/10 hover:bg-muted/20 cursor-pointer transition-all">
              <input
                type="checkbox"
                checked={toolTruth?.enabled ?? false}
                disabled={!toolTruth || savingToolTruth}
                onChange={(e) => handleToggleToolTruth(e.target.checked)}
                className="mt-0.5 rounded border-input text-primary focus:ring-primary h-4 w-4"
                data-testid="tool-truthfulness-toggle"
              />
              <div className="space-y-0.5 flex-1">
                <div className="text-xs font-medium text-foreground">
                  禁止客户端虚构工具执行结果
                </div>
                <p className="text-[11px] text-muted-foreground">
                  要求只报告工具实际返回的内容：未执行的操作标记【未执行】，无法从工具结果确认的标记【无法确认】，
                  输出被截断时必须重新调用工具。与中文输出规则各自独立，写在不同的标记块里。
                </p>
              </div>
            </label>

            {toolTruthError && (
              <div
                className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 space-y-1"
                data-testid="tool-truthfulness-error"
              >
                <p className="text-[11px] text-destructive flex items-start gap-1">
                  <XCircle className="h-3 w-3 mt-0.5 shrink-0" />
                  <span>读取失败：{toolTruthError}</span>
                </p>
                <p className="text-[11px] text-muted-foreground">
                  若提示命令不存在，说明当前运行的是旧构建。执行 build_release.bat
                  重新生成生产构建后重启即可。
                </p>
              </div>
            )}

            {toolTruth?.targets.map((t) => (
              <div key={t.path} className="space-y-1">
                <div className="flex items-center justify-between gap-2">
                  <span className="font-medium text-foreground">{t.target}</span>
                  {t.error ? (
                    <Badge variant="destructive">写入失败</Badge>
                  ) : (
                    <Badge variant={t.rulePresent ? "success" : "secondary"}>
                      {t.rulePresent ? "规则已写入" : "未写入"}
                    </Badge>
                  )}
                </div>
                <p className="text-[11px] text-muted-foreground font-mono break-all">{t.path}</p>
                {t.error && (
                  <p className="text-[11px] text-destructive flex items-start gap-1">
                    <XCircle className="h-3 w-3 mt-0.5 shrink-0" />
                    {t.error}
                  </p>
                )}
                {t.shadowedBy && (
                  <p className="text-[11px] text-amber-600 dark:text-amber-500 flex items-start gap-1">
                    <AlertTriangle className="h-3 w-3 mt-0.5 shrink-0" />
                    <span>
                      该文件被 <span className="font-mono break-all">{t.shadowedBy}</span>{" "}
                      抢先读取，规则不会生效。删除或清空该文件后才会读到这里。
                    </span>
                  </p>
                )}
              </div>
            ))}

            <p className="text-[11px] text-muted-foreground">
              这是写给客户端的约束，不是运行时校验：它降低虚构结果的概率，但不能从机制上阻止。
              客户端每次新开会话才读取指令文件，已经打开的会话需要重启才会生效。
            </p>
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
              检测到本地存在旧版配置，支持一键无缝导入至 PolyDeck。
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
              {logsError ? (
                <div className="text-destructive text-center py-4" role="alert">
                  读取日志失败：{logsError}
                </div>
              ) : logs.length === 0 ? (
                <div className="text-muted-foreground text-center py-4">暂无日志记录</div>
              ) : (
                logs.map((entry, idx) => (
                  <div key={idx} className="leading-tight text-foreground/90">
                    <span className="text-muted-foreground">{entry.timestamp}</span>{" "}
                    <span
                      className={
                        entry.level === "ERROR"
                          ? "text-destructive"
                          : entry.level === "WARN"
                            ? "text-amber-500"
                            : "text-muted-foreground"
                      }
                    >
                      [{entry.level}]
                    </span>{" "}
                    {entry.message}
                  </div>
                ))
              )}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
