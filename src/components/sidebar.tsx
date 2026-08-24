import { useEffect, useState } from "react";
import { NavLink } from "react-router-dom";
import { cn } from "@/lib/utils";
import { Zap, UserCheck, Monitor, Puzzle, History, Settings, Cpu } from "lucide-react";
import { ThemeToggle } from "./ThemeToggle";
import { preloadPage } from "@/lib/page-preload";
import { backend } from "@/services/backend";

const navItems = [
  { to: "/quick-setup", preload: "quickSetup", label: "快速配置", icon: Zap },
  { to: "/profiles", preload: "profiles", label: "配置方案", icon: UserCheck },
  { to: "/clients", preload: "clients", label: "客户端", icon: Monitor },
  { to: "/extensions", preload: "extensions", label: "扩展管理", icon: Puzzle },
  { to: "/history", preload: "history", label: "会话历史", icon: History },
  { to: "/settings", preload: "settings", label: "系统设置", icon: Settings },
] as const;

export function Sidebar() {
  // Read from the binary rather than hardcoding: a literal here is a second copy
  // of the version that has to be remembered on every release, and it was
  // already a release behind.
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    backend.getVersion().then(setVersion).catch(() => {});
  }, []);

  return (
    <aside className="w-[var(--sidebar-width)] border-r bg-card/60 backdrop-blur flex flex-col shrink-0 select-none">
      <div className="p-4 border-b flex items-center justify-between">
        <div className="flex items-center gap-2">
          <div className="h-8 w-8 rounded-lg bg-primary text-primary-foreground flex items-center justify-center font-bold">
            <Cpu className="h-5 w-5" />
          </div>
          <div>
            <h1 className="text-base font-bold tracking-tight">PolyDeck</h1>
            <p className="text-[11px] text-muted-foreground">
              {version ? `v${version} · ` : ""}Polymorphic Gateway
            </p>
          </div>
        </div>
        <ThemeToggle />
      </div>

      <nav className="flex-1 p-2 space-y-1 overflow-y-auto">
        {navItems.map(({ to, preload, label, icon: Icon }) => (
          <NavLink
            key={to}
            to={to}
            onPointerDown={() => preloadPage(preload)}
            onMouseEnter={() => preloadPage(preload)}
            onFocus={() => preloadPage(preload)}
            className={({ isActive }) =>
              cn(
                "flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all duration-150",
                isActive
                  ? "bg-primary text-primary-foreground shadow-sm"
                  : "text-muted-foreground hover:bg-accent hover:text-accent-foreground"
              )
            }
          >
            <Icon className="h-4 w-4 shrink-0" />
            <span>{label}</span>
          </NavLink>
        ))}
      </nav>

      <div className="p-3 border-t bg-muted/20 text-xs text-muted-foreground">
        <div className="flex items-center justify-between">
          <span>Tauri 2 · Rust Engine</span>
          <span className="inline-block w-2 h-2 rounded-full bg-emerald-500 animate-pulse" />
        </div>
      </div>
    </aside>
  );
}

