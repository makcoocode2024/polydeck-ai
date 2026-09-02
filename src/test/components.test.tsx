import { describe, it, expect } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { ThemeToggle } from "@/components/ThemeToggle";
import { BrowserRouter } from "react-router-dom";
import { Sidebar } from "@/components/sidebar";
import { StatusBar } from "@/components/status-bar";
import { invalidateBackendReadCache } from "@/services/backend";
import { setMockResponse } from "./setup";

describe("UI Components", () => {
  it("renders Button correctly with variant and size", () => {
    render(<Button variant="destructive">删除方案</Button>);
    const btn = screen.getByRole("button", { name: /删除方案/i });
    expect(btn).toBeInTheDocument();
    expect(btn.className).toContain("bg-destructive");
  });

  it("renders Card and subcomponents", () => {
    render(
      <Card>
        <CardHeader>
          <CardTitle>测试卡片标题</CardTitle>
        </CardHeader>
        <CardContent>卡片详细内容</CardContent>
      </Card>
    );
    expect(screen.getByText("测试卡片标题")).toBeInTheDocument();
    expect(screen.getByText("卡片详细内容")).toBeInTheDocument();
  });

  it("renders Badge variants", () => {
    render(<Badge variant="success">已安装</Badge>);
    const badge = screen.getByText("已安装");
    expect(badge).toBeInTheDocument();
    expect(badge.className).toContain("text-emerald");
  });

  it("renders ThemeToggle", () => {
    render(<ThemeToggle />);
    const toggleBtn = screen.getByRole("button", { name: /切换主题/i });
    expect(toggleBtn).toBeInTheDocument();
  });

  it("renders Sidebar navigation links", () => {
    render(
      <BrowserRouter>
        <Sidebar />
      </BrowserRouter>
    );
    expect(screen.getByText("PolyDeck")).toBeInTheDocument();
    expect(screen.getByText("快速配置")).toBeInTheDocument();
    expect(screen.getByText("配置方案")).toBeInTheDocument();
    expect(screen.getByText("客户端")).toBeInTheDocument();
    expect(screen.getByText("扩展管理")).toBeInTheDocument();
    expect(screen.getByText("会话历史")).toBeInTheDocument();
    expect(screen.getByText("系统设置")).toBeInTheDocument();
  });

  it("shows the version the backend reports rather than a hardcoded one", async () => {
    // The mock answers ad_get_version with "2.0.0". A literal in the component
    // would show something else here, which is how the sidebar fell a release
    // behind the workspace version.
    invalidateBackendReadCache("version");
    render(
      <BrowserRouter>
        <Sidebar />
      </BrowserRouter>
    );

    await waitFor(() => {
      expect(screen.getByText(/v2\.0\.0 · Polymorphic Gateway/)).toBeInTheDocument();
    });
  });

  it("renders StatusBar gateway state", async () => {
    render(<StatusBar />);
    await waitFor(() => {
      expect(screen.getByText(/网关/)).toBeInTheDocument();
    });
  });

  it("reports every bound profile, not one active one", async () => {
    // The bar used to read `getActiveProfile`, which now answers null whenever the
    // bound clients disagree — so with two profiles in use it would have said
    // "未指定" exactly when the most was configured.
    const restore = setMockResponse("ad_list_client_bindings", [
      {
        clientId: "codex-cli",
        profileId: "prof_a",
        profileName: "方案A",
        gatewayEnabled: true,
        boundAt: "2026-08-18T00:00:00Z",
      },
      {
        clientId: "claude-code",
        profileId: "prof_b",
        profileName: "方案B",
        gatewayEnabled: true,
        boundAt: "2026-08-18T00:00:00Z",
      },
    ]);
    try {
      render(<StatusBar />);
      const row = await screen.findByTestId("status-bindings");
      await waitFor(() => expect(row.textContent).toContain("2 个客户端"));
      expect(row.textContent).toContain("方案A、方案B");
      // Which client went where is in the tooltip; the bar has no room for it.
      expect(row.getAttribute("title")).toBe(
        "codex-cli → 方案A\nclaude-code → 方案B"
      );
    } finally {
      restore();
    }
  });

  it("says so when nothing is bound", async () => {
    const restore = setMockResponse("ad_list_client_bindings", []);
    try {
      render(<StatusBar />);
      const row = await screen.findByTestId("status-bindings");
      await waitFor(() => expect(row.textContent).toContain("未绑定"));
    } finally {
      restore();
    }
  });
});