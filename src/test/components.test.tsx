import { describe, it, expect } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { ThemeToggle } from "@/components/ThemeToggle";
import { BrowserRouter } from "react-router-dom";
import { Sidebar } from "@/components/sidebar";
import { StatusBar } from "@/components/status-bar";

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

  it("renders StatusBar gateway state", async () => {
    render(<StatusBar />);
    await waitFor(() => {
      expect(screen.getByText(/网关/)).toBeInTheDocument();
    });
  });
});