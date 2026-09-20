import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import "@testing-library/jest-dom";
import { BrowserRouter } from "react-router-dom";
import SmartRoutePage from "@/pages/SmartRoutePage";
import { setMockResponse } from "./setup";
import { invalidateBackendReadCache } from "@/services/backend";
import type { SmartRouteSettings } from "@/domain/smartRoute";

const enabledConfig: SmartRouteSettings = {
  enableRoute: true,
  forceModel: null,
  defaultModel: "m-default",
  fallbackEnable: true,
  modelAliasMap: { "claude-opus-5": "glm-5.3" },
  categoryRouteMap: {
    complexCore: "m-complex",
    regularDev: "m-regular",
    visualFrontend: "m-visual",
  },
  customRules: [
    { id: "r1", keyword: "排课", targetModel: "m-rule", isRegex: false, enable: true },
  ],
};

function renderPage() {
  return render(
    <BrowserRouter>
      <SmartRoutePage />
    </BrowserRouter>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  // The page reads through the shared read cache; a prior test's config
  // would otherwise leak into this file's `setMockResponse` overrides.
  invalidateBackendReadCache();
});

describe("SmartRoutePage", () => {
  it("renders the config panel from the backend config", async () => {
    setMockResponse("ad_get_route_config", enabledConfig);
    renderPage();
    await waitFor(() =>
      expect(screen.getByTestId("route-force-model-input")).toHaveValue(""),
    );
    expect(screen.getByTestId("route-default-model-input")).toHaveValue("m-default");
    expect(screen.getByTestId("route-enable-toggle")).toBeChecked();
    expect(screen.getByTestId("route-fallback-toggle")).toBeChecked();
    // The alias row and the custom rule row both render from the config.
    expect(screen.getByTestId("alias-key-input-0")).toHaveValue("claude-opus-5");
    expect(screen.getByTestId("alias-value-input-0")).toHaveValue("glm-5.3");
    expect(screen.getByTestId("rule-keyword-input-r1")).toHaveValue("排课");
  });

  it("saves the config through the update command and shows saved status", async () => {
    setMockResponse("ad_get_route_config", enabledConfig);
    const update = vi.fn().mockResolvedValue({ config: enabledConfig, warnings: [] });
    setMockResponse("ad_update_route_config", update);
    renderPage();
    await waitFor(() => expect(screen.getByTestId("route-save-button")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("route-save-button"));
    await waitFor(() =>
      expect(screen.getByTestId("route-save-status")).toHaveTextContent("已保存"),
    );
    expect(update).toHaveBeenCalledTimes(1);
    expect(update.mock.calls[0][0]).toMatchObject({ config: enabledConfig });
  });

  it("surfaces warnings returned by the update command", async () => {
    setMockResponse("ad_get_route_config", enabledConfig);
    setMockResponse("ad_update_route_config", {
      config: enabledConfig,
      warnings: ["规则 r1 的正则无法编译: (["],
    });
    renderPage();
    await waitFor(() => expect(screen.getByTestId("route-save-button")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("route-save-button"));
    await waitFor(() =>
      expect(screen.getByText(/正则无法编译/)).toBeInTheDocument(),
    );
  });

  it("shows a regex error inline when the pattern does not compile", async () => {
    setMockResponse("ad_get_route_config", {
      ...enabledConfig,
      customRules: [
        { id: "bad", keyword: "([unclosed", targetModel: "m", isRegex: true, enable: true },
      ],
    });
    renderPage();
    await waitFor(() =>
      expect(screen.getByTestId("rule-error-bad")).toHaveTextContent("正则无法编译"),
    );
  });

  it("adds an alias row on the add button", async () => {
    const restore = setMockResponse("ad_get_route_config", {
      ...enabledConfig,
      modelAliasMap: {},
    });
    invalidateBackendReadCache("route-config");
    try {
      renderPage();
      await waitFor(() => expect(screen.getByTestId("alias-add-button")).toBeInTheDocument());
      fireEvent.click(screen.getByTestId("alias-add-button"));
      expect(screen.getAllByTestId(/^alias-key-input-/).length).toBe(1);
      expect(screen.getAllByTestId(/^alias-value-input-/).length).toBe(1);
    } finally {
      restore();
    }
  });

  it("deletes a rule row on its delete button", async () => {
    const restore = setMockResponse("ad_get_route_config", enabledConfig);
    invalidateBackendReadCache("route-config");
    try {
      renderPage();
      await waitFor(() =>
        expect(screen.getByTestId("rule-row-r1")).toBeInTheDocument(),
      );
      fireEvent.click(screen.getByTestId("rule-delete-r1"));
      expect(screen.queryByTestId("rule-row-r1")).not.toBeInTheDocument();
    } finally {
      restore();
    }
  });

  it("renders the simulator result from ad_simulate_route", async () => {
    setMockResponse("ad_get_route_config", enabledConfig);
    setMockResponse("ad_simulate_route", {
      routedModel: "m-rule",
      reason: "custom_rule",
      matchRule: "r1",
      isTagHit: false,
      validationWarn: null,
    });
    renderPage();
    await waitFor(() => expect(screen.getByTestId("simulator-run-button")).toBeInTheDocument());
    fireEvent.change(screen.getByTestId("simulator-prompt-input"), {
      target: { value: "修复排课冲突" },
    });
    fireEvent.click(screen.getByTestId("simulator-run-button"));
    await waitFor(() => expect(screen.getByTestId("simulator-result")).toBeInTheDocument());
    expect(screen.getByTestId("simulator-result")).toHaveTextContent("m-rule");
    expect(screen.getByTestId("simulator-result")).toHaveTextContent("命中规则：r1");
  });

  it("renders audit rows after refresh", async () => {
    setMockResponse("ad_get_route_config", enabledConfig);
    renderPage();
    await waitFor(() => expect(screen.getByTestId("audit-refresh-button")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("audit-refresh-button"));
    await waitFor(() =>
      expect(screen.getByTestId("audit-row-req-0001")).toBeInTheDocument(),
    );
    expect(screen.getByTestId("audit-row-req-0001")).toHaveTextContent("claude-opus-5");
  });
});
