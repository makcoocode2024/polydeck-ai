import { describe, it, expect, vi } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import QuickSetupPage from "@/pages/QuickSetupPage";
import ProfilesPage from "@/pages/ProfilesPage";
import ClientsPage from "@/pages/ClientsPage";
import ExtensionsPage from "@/pages/ExtensionsPage";
import HistoryPage from "@/pages/HistoryPage";
import SettingsPage from "@/pages/SettingsPage";
import { backend } from "@/services/backend";

describe("Frontend Pages", () => {
  it("arms an Agnes route from the dedicated panel and drives the form", async () => {
    render(<QuickSetupPage />);

    // Panel is present but no route armed, so no model choices yet.
    expect(screen.getByTestId("agnes-panel")).toBeInTheDocument();
    expect(screen.queryByTestId("agnes-model-agnes-2.5-flash")).not.toBeInTheDocument();

    const baseUrlInput = screen.getByPlaceholderText("https://api.openai.com/v1") as HTMLInputElement;
    const modelInput = screen.getByPlaceholderText("gpt-4o / claude-3-7-sonnet") as HTMLInputElement;

    // Arming the CN route fills base URL and model, and opens the model picker.
    fireEvent.click(screen.getByTestId("agnes-route-agnes-cn"));
    await waitFor(() => {
      expect(baseUrlInput.value).toBe("https://api.agnes-ai.cn/v1");
    });
    expect(modelInput.value).toBe("agnes-2.5-flash");
    expect(screen.getByTestId("agnes-model-agnes-2.5-flash")).toBeInTheDocument();

    // Switching to the international route only changes the host.
    fireEvent.click(screen.getByTestId("agnes-route-agnes-global"));
    await waitFor(() => {
      expect(baseUrlInput.value).toBe("https://apihub.agnes-ai.com/v1");
    });
    expect(modelInput.value).toBe("agnes-2.5-flash");

    // A paid model raises the output-budget warning; the free default does not.
    expect(screen.queryByText(/先消耗输出预算做推理/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agnes-model-agnes-2.5-pro"));
    await waitFor(() => {
      expect(modelInput.value).toBe("agnes-2.5-pro");
    });
    expect(screen.getByText(/先消耗输出预算做推理/)).toBeInTheDocument();

    // Back to a free model clears it again.
    fireEvent.click(screen.getByTestId("agnes-model-agnes-2.0-flash"));
    await waitFor(() => {
      expect(modelInput.value).toBe("agnes-2.0-flash");
    });
    expect(screen.queryByText(/先消耗输出预算做推理/)).not.toBeInTheDocument();
  });

  it("keeps Claude Code selected after a probe changes the protocol", async () => {
    // Regression: the client-detection effect depended on `currentProtocol` and
    // re-derived the selection whenever a probe adjusted it, silently dropping
    // claude-code and leaving an Agnes profile's tier slots unreachable.
    render(<QuickSetupPage />);

    fireEvent.click(screen.getByTestId("agnes-route-agnes-cn"));
    await waitFor(() => {
      expect(screen.getByTestId("agnes-model-agnes-2.5-flash")).toBeInTheDocument();
    });

    const claudeCode = await screen.findByRole("checkbox", { name: /Claude Code/i });
    expect((claudeCode as HTMLInputElement).checked).toBe(true);

    // Probing reports `responses` for Agnes; the selection must survive it.
    fireEvent.click(screen.getByRole("button", { name: /测试连接/i }));
    await waitFor(() => {
      expect(screen.getByText(/当前协议/i)).toBeInTheDocument();
    });
    expect((claudeCode as HTMLInputElement).checked).toBe(true);
  });

  it("hides non-chat Agnes models from the picker", async () => {
    // Regression: /v1/models also lists image and video models, which answer on
    // other endpoints. Saving them verbatim put agnes-video-2.5 into Codex's
    // model catalogue as a selectable chat model.
    vi.spyOn(backend, "probeProvider").mockResolvedValueOnce({
      protocol: "responses",
      confidence: "high",
      evidence: [],
      codexCompat: "responses_function",
      baseUrl: "https://api.agnes-ai.cn",
      supportsStreaming: true,
      models: [
        { id: "agnes-2.5-flash", name: "agnes-2.5-flash" },
        { id: "agnes-2.0-flash", name: "agnes-2.0-flash" },
        { id: "agnes-image-2.1-flash", name: "agnes-image-2.1-flash" },
        { id: "agnes-video-2.5", name: "agnes-video-2.5" },
        { id: "agnes-video-v2.0", name: "agnes-video-v2.0" },
      ],
    } as never);

    render(<QuickSetupPage />);
    fireEvent.click(screen.getByTestId("agnes-route-agnes-cn"));
    await waitFor(() => {
      expect(screen.getByTestId("agnes-model-agnes-2.5-flash")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: /获取模型/i }));
    const select = (await screen.findByRole("combobox", {
      name: /从已获取的模型列表中选择/i,
    })) as HTMLSelectElement;

    const offered = Array.from(select.options)
      .map((o) => o.value)
      .filter(Boolean);
    expect(offered).toContain("agnes-2.5-flash");
    expect(offered).toContain("agnes-2.0-flash");
    // The three non-chat models must not be selectable.
    expect(offered).not.toContain("agnes-image-2.1-flash");
    expect(offered).not.toContain("agnes-video-2.5");
    expect(offered).not.toContain("agnes-video-v2.0");
    expect(screen.getByText(/已隐藏 3 个非对话模型/)).toBeInTheDocument();
  });

  it("warns when the gateway is off but Codex needs it", async () => {
    render(<QuickSetupPage />);

    fireEvent.click(screen.getByTestId("agnes-route-agnes-cn"));
    await waitFor(() => {
      expect(screen.getByTestId("agnes-model-agnes-2.5-flash")).toBeInTheDocument();
    });

    // Agnes arms as chat_function, which Codex cannot reach directly.
    expect(screen.getByText(/Codex 必须开启桥接/)).toBeInTheDocument();
    expect(screen.queryByTestId("codex-needs-gateway-warning")).not.toBeInTheDocument();

    // Switching the gateway off must name the failure rather than stay silent.
    const gatewayToggle = screen.getByRole("checkbox", { name: /启用本地代理网关/i });
    fireEvent.click(gatewayToggle);
    await waitFor(() => {
      expect(screen.getByTestId("codex-needs-gateway-warning")).toBeInTheDocument();
    });
    expect(screen.getByText(/unknown variant/)).toBeInTheDocument();

    // Back on, warning clears.
    fireEvent.click(gatewayToggle);
    await waitFor(() => {
      expect(screen.queryByTestId("codex-needs-gateway-warning")).not.toBeInTheDocument();
    });
  });

  it("renders QuickSetupPage successfully and handles model fetch and dropdown selection", async () => {
    render(<QuickSetupPage />);
    expect(screen.getByText("快速配置 PolyDeck")).toBeInTheDocument();
    expect(screen.getByText("配置大模型服务商与 API Key")).toBeInTheDocument();
    
    // Check fetch model button exists
    const fetchBtn = screen.getByRole("button", { name: /获取模型/i });
    expect(fetchBtn).toBeInTheDocument();

    // Click fetch model button
    fireEvent.click(fetchBtn);

    // Wait for models to be loaded and dropdown to appear
    await waitFor(() => {
      expect(screen.getByText(/从已获取的模型列表中快速选择/i)).toBeInTheDocument();
    });

    // Verify model options
    const select = screen.getByRole("combobox", { name: /从已获取的模型列表中选择/i }) as HTMLSelectElement;
    expect(select).toBeInTheDocument();
    expect(screen.getByText(/gpt-4o-mini/i)).toBeInTheDocument();

    // Select gpt-4o-mini from dropdown
    fireEvent.change(select, { target: { value: "gpt-4o-mini" } });

    // Verify the input box updated to gpt-4o-mini
    const modelInput = screen.getByPlaceholderText("gpt-4o / claude-3-7-sonnet") as HTMLInputElement;
    expect(modelInput.value).toBe("gpt-4o-mini");

    await waitFor(() => {
      expect(screen.getByText("Codex CLI")).toBeInTheDocument();
    });
  });

  it("handles model fetch failure with user-friendly fallback hint", async () => {
    // Temporarily mock probeProvider to reject
    const spy = vi.spyOn(backend, "probeProvider").mockRejectedValueOnce(new Error("Network Timeout"));

    render(<QuickSetupPage />);
    const fetchBtn = screen.getByRole("button", { name: /获取模型/i });
    fireEvent.click(fetchBtn);

    await waitFor(() => {
      expect(screen.getByText(/未能自动获取到模型列表|获取模型失败/i)).toBeInTheDocument();
    });

    // Input box is still accessible and editable
    const modelInput = screen.getByPlaceholderText("gpt-4o / claude-3-7-sonnet") as HTMLInputElement;
    fireEvent.change(modelInput, { target: { value: "custom-deepseek-v3" } });
    expect(modelInput.value).toBe("custom-deepseek-v3");

    spy.mockRestore();
  });

  it("renders ProfilesPage and handles primary provider connectivity test and edit modal probing", async () => {
    const probeSpy = vi.spyOn(backend, "probeProvider");

    render(<ProfilesPage />);
    await waitFor(() => {
      expect(screen.getAllByText("Default Profile").length).toBeGreaterThan(0);
      expect(screen.getByText("OpenAI 官方")).toBeInTheDocument();
    });

    // Click "连通探测" button in profile details
    const probePrimaryBtn = screen.getByRole("button", { name: /连通探测/i });
    expect(probePrimaryBtn).toBeInTheDocument();
    fireEvent.click(probePrimaryBtn);

    await waitFor(() => {
      expect(screen.getByText(/主节点连通与鉴权正常/i)).toBeInTheDocument();
    });

    // Open Edit Modal
    const editBtns = screen.getAllByRole("button", { name: /编辑/i });
    fireEvent.click(editBtns[0]);

    await waitFor(() => {
      expect(screen.getByText("编辑配置方案")).toBeInTheDocument();
    });

    // Switch to Provider tab
    const providerTabBtn = screen.getByRole("button", { name: /Provider 节点/i });
    fireEvent.click(providerTabBtn);

    // Test probe inside modal
    const nodeProbeBtn = screen.getByRole("button", { name: /探测连通与模型/i });
    expect(nodeProbeBtn).toBeInTheDocument();
    fireEvent.click(nodeProbeBtn);

    await waitFor(() => {
      expect(screen.getByText(/探测与鉴权成功/i)).toBeInTheDocument();
      expect(screen.getByText(/从探测到的模型列表中选择/i)).toBeInTheDocument();
    });

    probeSpy.mockRestore();
  });

  it("opens edit modal when clicking edit button on a profile and saves modifications", async () => {
    const updateSpy = vi.spyOn(backend, "updateProfile");

    render(<ProfilesPage />);
    await waitFor(() => {
      expect(screen.getAllByText("Default Profile").length).toBeGreaterThan(0);
    });

    // Click the Edit button for the profile
    const editBtns = screen.getAllByRole("button", { name: /编辑/i });
    expect(editBtns.length).toBeGreaterThan(0);
    fireEvent.click(editBtns[0]);

    // Modal should be opened
    await waitFor(() => {
      expect(screen.getByText("编辑配置方案")).toBeInTheDocument();
    });

    // Change profile name in basic settings
    const nameInput = screen.getByPlaceholderText("输入方案名称") as HTMLInputElement;
    expect(nameInput.value).toBe("Default Profile");
    fireEvent.change(nameInput, { target: { value: "Modified Profile" } });

    // Switch to Provider tab
    const providerTabBtn = screen.getByRole("button", { name: /Provider 节点/i });
    fireEvent.click(providerTabBtn);

    // Verify existing provider is shown
    expect(screen.getByDisplayValue("OpenAI Primary")).toBeInTheDocument();

    // Click Add Provider
    const addProvBtn = screen.getByRole("button", { name: /添加 Provider 节点/i });
    fireEvent.click(addProvBtn);
    expect(screen.getByDisplayValue("Provider 2")).toBeInTheDocument();

    // Switch to Clients tab
    const clientsTabBtn = screen.getByRole("button", { name: /客户端绑定/i });
    fireEvent.click(clientsTabBtn);
    expect(screen.getAllByText(/Codex CLI/i).length).toBeGreaterThan(0);

    // Click Save button
    const saveBtn = screen.getByRole("button", { name: /仅保存方案/i });
    fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(updateSpy).toHaveBeenCalledWith("prof_default", expect.objectContaining({
        name: "Modified Profile",
      }));
    });

    updateSpy.mockRestore();
  });


  it("handles real chat test in QuickSetupPage", async () => {
    const chatSpy = vi.spyOn(backend, "testProviderChat");

    render(<QuickSetupPage />);
    const chatBtn = screen.getByRole("button", { name: /真实对话测试/i });
    expect(chatBtn).toBeInTheDocument();

    fireEvent.click(chatBtn);

    await waitFor(() => {
      expect(screen.getByText(/真实对话测试成功/i)).toBeInTheDocument();
      expect(screen.getByText(/这是一条来自 AI 模型的实时对话测试回复/i)).toBeInTheDocument();
    });

    expect(chatSpy).toHaveBeenCalled();
    chatSpy.mockRestore();
  });

  it("handles real chat test on ProfilesPage inspector and inside edit modal", async () => {
    const chatSpy = vi.spyOn(backend, "testProviderChat");

    render(<ProfilesPage />);
    await waitFor(() => {
      expect(screen.getAllByText("Default Profile").length).toBeGreaterThan(0);
    });

    // Click "真实对话测试" in Profile details inspector
    const chatInspectorBtn = screen.getByRole("button", { name: /真实对话测试/i });
    expect(chatInspectorBtn).toBeInTheDocument();
    fireEvent.click(chatInspectorBtn);

    await waitFor(() => {
      expect(screen.getByText(/主节点模型回复/i)).toBeInTheDocument();
      expect(screen.getByText(/这是一条来自 AI 模型的实时对话测试回复/i)).toBeInTheDocument();
    });

    // Open Edit Modal
    const editBtn = screen.getByRole("button", { name: /编辑方案/i });
    fireEvent.click(editBtn);

    await waitFor(() => {
      expect(screen.getByText("编辑配置方案")).toBeInTheDocument();
    });

    // Switch to Provider tab
    const providerTabBtn = screen.getByRole("button", { name: /Provider 节点/i });
    fireEvent.click(providerTabBtn);

    // Click node chat test inside modal (last button with text 真实对话测试)
    const nodeChatBtns = screen.getAllByRole("button", { name: /真实对话测试/i });
    expect(nodeChatBtns.length).toBeGreaterThan(0);
    fireEvent.click(nodeChatBtns[nodeChatBtns.length - 1]);

    await waitFor(() => {
      expect(screen.getByText(/对话成功/i)).toBeInTheDocument();
    });

    expect(chatSpy).toHaveBeenCalled();
    chatSpy.mockRestore();
  });

  it("handles duplicating a profile from the profile list", async () => {
    const dupSpy = vi.spyOn(backend, "duplicateProfile");

    render(<ProfilesPage />);
    await waitFor(() => {
      expect(screen.getAllByText("Default Profile").length).toBeGreaterThan(0);
    });

    // Find and click the Copy button for the profile
    const copyBtns = screen.getAllByRole("button", { name: /复制/i });
    expect(copyBtns.length).toBeGreaterThan(0);
    fireEvent.click(copyBtns[0]);

    await waitFor(() => {
      expect(dupSpy).toHaveBeenCalledWith("prof_default");
      expect(screen.getByText("Default Profile (副本)")).toBeInTheDocument();
    });

    dupSpy.mockRestore();
  });

  it("handles provider rate limiting configuration, auto-probing, and saving", async () => {
    const probeRateSpy = vi.spyOn(backend, "probeRateLimits");
    const updateSpy = vi.spyOn(backend, "updateProfile");

    render(<ProfilesPage />);
    await waitFor(() => {
      expect(screen.getAllByText("Default Profile").length).toBeGreaterThan(0);
    });

    // Open Edit Modal
    const editBtn = screen.getByRole("button", { name: /编辑方案/i });
    fireEvent.click(editBtn);

    await waitFor(() => {
      expect(screen.getByText("编辑配置方案")).toBeInTheDocument();
    });

    // Switch to Provider tab
    const providerTabBtn = screen.getByRole("button", { name: /Provider 节点/i });
    fireEvent.click(providerTabBtn);

    // Verify Rate Limiting section exists
    const rateLimitSection = screen.getByTestId("provider-ratelimit-section-0");
    expect(rateLimitSection).toBeInTheDocument();
    expect(screen.getByText(/请求速率与 Token 限流/i)).toBeInTheDocument();

    // Toggle rate limit on
    const rateLimitToggle = screen.getByTestId("provider-ratelimit-toggle-0") as HTMLInputElement;
    expect(rateLimitToggle.checked).toBe(false);
    fireEvent.click(rateLimitToggle);
    expect(rateLimitToggle.checked).toBe(true);

    // Test Auto-Probe button
    const autoProbeBtn = screen.getByTestId("provider-auto-probe-ratelimit-btn-0");
    expect(autoProbeBtn).toBeInTheDocument();
    fireEvent.click(autoProbeBtn);

    await waitFor(() => {
      expect(screen.getByTestId("provider-ratelimit-probe-msg-0")).toBeInTheDocument();
      expect(screen.getByText(/从上游响应头获取到限制/i)).toBeInTheDocument();
    });
    expect(probeRateSpy).toHaveBeenCalled();

    // Modify RPM and TPM manually
    const rpmInput = screen.getByTestId("provider-rpm-input-0") as HTMLInputElement;
    fireEvent.change(rpmInput, { target: { value: "45" } });
    expect(rpmInput.value).toBe("45");

    const tpmInput = screen.getByTestId("provider-tpm-input-0") as HTMLInputElement;
    fireEvent.change(tpmInput, { target: { value: "80000" } });
    expect(tpmInput.value).toBe("80000");

    // Click Save button
    const saveBtn = screen.getByRole("button", { name: /仅保存方案/i });
    fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(updateSpy).toHaveBeenCalledWith(
        "prof_default",
        expect.objectContaining({
          providers: expect.arrayContaining([
            expect.objectContaining({
              rateLimit: expect.objectContaining({
                enabled: true,
                rpm: 45,
                tpm: 80000,
                adaptive: true,
              }),
            }),
          ]),
        })
      );
    });

    probeRateSpy.mockRestore();
    updateSpy.mockRestore();
  });

  it("displays rate limit badge in profile details inspector", async () => {
    render(<ProfilesPage />);
    await waitFor(() => {
      expect(screen.getAllByText("Default Profile").length).toBeGreaterThan(0);
    });

    // Verify rate limit badge in inspector
    const badge = screen.getByTestId("inspector-ratelimit-badge-prov_1");
    expect(badge).toBeInTheDocument();
    expect(badge.textContent).toMatch(/限流|未设速率限制/i);
  });

  it("renders ClientsPage and lists detected clients", async () => {
    render(<ClientsPage />);
    expect(screen.getByText("AI 开发客户端与接入")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText("Codex CLI")).toBeInTheDocument();
      expect(screen.getByText("Cursor IDE")).toBeInTheDocument();
    });
  });

  it("renders ExtensionsPage with MCP, skills, and inject tabs", async () => {
    render(<ExtensionsPage />);
    expect(screen.getByText("扩展生态与脚本注入")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText("本地文件系统")).toBeInTheDocument();
    });
  });

  it("renders HistoryPage and displays stats and sessions", async () => {
    render(<HistoryPage />);
    expect(screen.getByText("会话历史与安全备份")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText("优化 Rust Gateway 路由")).toBeInTheDocument();
    });
  });

  it("renders SettingsPage and displays settings modules", async () => {
    render(<SettingsPage />);
    expect(screen.getByText("系统设置与诊断")).toBeInTheDocument();
    expect(screen.getByText("外观与显示主题")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText("Provider Doctor 智能诊断体系")).toBeInTheDocument();
      expect(screen.getByText("Clash Verge")).toBeInTheDocument();
    });
  });

      it("pre-fills saved API key in edit profile modal with masked password and allows reveal toggle", async () => {
    render(<ProfilesPage />);
    await waitFor(() => {
      expect(screen.getAllByText("Default Profile").length).toBeGreaterThan(0);
    });

    // Open Edit Modal
    const editBtns = screen.getAllByRole("button", { name: /编辑/i });
    expect(editBtns.length).toBeGreaterThan(0);
    fireEvent.click(editBtns[0]);

    await waitFor(() => {
      expect(screen.getByText("编辑配置方案")).toBeInTheDocument();
    });

    // Switch to Provider tab
    const providerTabBtn = screen.getByRole("button", { name: /Provider 节点/i });
    fireEvent.click(providerTabBtn);

    // Verify API key input is pre-filled with saved key and is type="password" by default
    const keyInput = screen.getByTestId("provider-key-input-0") as HTMLInputElement;
    await waitFor(() => {
      expect(keyInput.value).toBe("sk-mock-key-123456");
    });
    expect(keyInput.type).toBe("password");

    // Click toggle button to reveal API key
    const toggleBtn = screen.getByTestId("provider-key-toggle-0");
    fireEvent.click(toggleBtn);
    expect(keyInput.type).toBe("text");

    // Click toggle button again to hide API key
    fireEvent.click(toggleBtn);
    expect(keyInput.type).toBe("password");
  });

  it("supports API key hide/reveal toggle in QuickSetupPage", async () => {
    render(<QuickSetupPage />);
    const keyInput = screen.getByTestId("quicksetup-api-key-input") as HTMLInputElement;
    expect(keyInput).toBeInTheDocument();
    expect(keyInput.type).toBe("password");

    // Type a key
    fireEvent.change(keyInput, { target: { value: "sk-secret-key-999" } });
    expect(keyInput.value).toBe("sk-secret-key-999");
    expect(keyInput.type).toBe("password");

    // Click toggle button to show key
    const toggleBtn = screen.getByTestId("quicksetup-api-key-toggle");
    fireEvent.click(toggleBtn);
    expect(keyInput.type).toBe("text");

    // Click toggle button again to hide key
    fireEvent.click(toggleBtn);
    expect(keyInput.type).toBe("password");
  });
});