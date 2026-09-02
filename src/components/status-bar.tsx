import { useEffect, useState, useCallback } from "react";
import { useAtom } from "jotai";
import { gatewayStatusAtom } from "@/state/gateway";
import { backend } from "@/services/backend";
import { Button } from "@/components/ui/button";
import { Play, Square, RefreshCw, Layers, RotateCw } from "lucide-react";
import type { ClientBindingView } from "@/domain/profile";

export function StatusBar() {
  const [gateway, setGateway] = useAtom(gatewayStatusAtom);
  // Bindings rather than one active profile: several can be in use at once, and
  // `getActiveProfile` answers null whenever the bound clients disagree — which
  // would read as "nothing configured" exactly when the most is.
  const [bindings, setBindings] = useState<ClientBindingView[]>([]);
  const [loading, setLoading] = useState(false);

  const refreshStatus = useCallback(async () => {
    try {
      const gw = await backend.gatewayStatus();
      setGateway(gw);
    } catch {
      // Keep previous state if offline or mocking
    }
    try {
      setBindings((await backend.listClientBindings()) ?? []);
    } catch {
      // Ignored
    }
  }, [setGateway]);

  useEffect(() => {
    refreshStatus();
    const interval = setInterval(refreshStatus, 3000);
    return () => clearInterval(interval);
  }, [refreshStatus]);

  const toggleGateway = async () => {
    setLoading(true);
    try {
      if (gateway.running) {
        await backend.gatewayStop();
      } else {
        await backend.gatewayStart();
      }
      await refreshStatus();
    } catch (err) {
      console.error("Gateway toggle error:", err); alert("网关操作失败: " + (err instanceof Error ? err.message : String(err)));
    } finally {
      setLoading(false);
    }
  };

  return (
    <footer className="h-9 border-t bg-card/80 backdrop-blur flex items-center justify-between px-4 text-xs select-none">
      <div className="flex items-center gap-4">
        <div className="flex items-center gap-2">
          <span
            className={`h-2 w-2 rounded-full ${
              gateway.running ? "bg-emerald-500 shadow-sm shadow-emerald-500/50" : "bg-zinc-400"
            }`}
          />
          <span className="font-medium text-foreground">
            {gateway.running ? `网关运行中 (端口: ${gateway.port ?? 18888})` : "网关已停止"}
          </span>
          <Button
            variant="ghost"
            size="sm"
            className="h-6 px-2 text-[11px] hover:bg-muted"
            onClick={toggleGateway}
            disabled={loading}
          >
            {loading ? (<><RotateCw className="h-3 w-3 mr-1 animate-spin text-primary" />处理中...</>) : gateway.running ? (<><Square className="h-3 w-3 mr-1 text-destructive" />停止</>) : (<><Play className="h-3 w-3 mr-1 text-emerald-500" />启动</>)}
          </Button>
        </div>

        <div className="h-3 w-px bg-border" />

        {(() => {
          const names = Array.from(
            new Set(bindings.map((b) => b.profileName).filter((n): n is string => !!n))
          );
          return (
            <div
              className="flex items-center gap-1.5 text-muted-foreground"
              title={
                bindings.length > 0
                  ? bindings.map((b) => `${b.clientId} → ${b.profileName ?? "?"}`).join("\n")
                  : undefined
              }
              data-testid="status-bindings"
            >
              <Layers className="h-3.5 w-3.5" />
              {bindings.length === 0 ? (
                <>
                  <span>客户端绑定:</span>
                  <span className="font-medium text-foreground">未绑定</span>
                </>
              ) : (
                <>
                  <span>{bindings.length} 个客户端:</span>
                  <span className="font-medium text-foreground">{names.join("、")}</span>
                </>
              )}
            </div>
          );
        })()}
      </div>

      <div className="flex items-center gap-2 text-muted-foreground">
        <Button
          variant="ghost"
          size="sm"
          className="h-6 px-1.5 text-muted-foreground hover:text-foreground"
          onClick={refreshStatus}
          title="刷新状态"
        >
          <RefreshCw className="h-3 w-3" />
        </Button>
        <span>PolyDeck Core</span>
      </div>
    </footer>
  );
}