import { describe, it, expect } from "vitest";
import { backend } from "@/services/backend";

describe("Backend API Service", () => {
  it("fetches app version and ping", async () => {
    const version = await backend.getVersion();
    expect(version).toBe("2.0.0");

    const ping = await backend.ping();
    expect(ping).toBe("pong");
  });

  it("detects clients", async () => {
    const clients = await backend.detectClients();
    expect(Array.isArray(clients)).toBe(true);
    expect(clients.length).toBeGreaterThan(0);
    expect(clients[0].id).toBe("codex-cli");
  });

  it("probes provider rate limits and recommendations", async () => {
    const rec = await backend.probeRateLimits("https://api.openai.com/v1", "sk-test", "gpt-4o");
    expect(rec.recommendedRpm).toBe(60);
    expect(rec.recommendedTpm).toBe(100000);
    expect(rec.detectedFromHeaders).toBe(true);
    expect(rec.message).toContain("RPM=60");
  });

  it("probes provider for models and capabilities", async () => {
    const probe = await backend.probeProvider("https://api.openai.com/v1", "sk-test");
    expect(probe.protocol).toBe("openai");
    expect(probe.models.length).toBe(3);
    expect(probe.models[0].id).toBe("gpt-4o");
  });

  it("manages profiles and templates", async () => {
    const profiles = await backend.listProfiles();
    expect(profiles.length).toBeGreaterThan(0);
    expect(profiles[0].name).toBe("Default Profile");

    const active = await backend.getActiveProfile();
    expect(active?.id).toBe("prof_default");

    const templates = await backend.getProfileTemplates();
    expect(templates.length).toBeGreaterThan(0);

    const created = await backend.createProfile("Test Profile");
    expect(created.name).toBe("New Profile");
  });

  it("controls gateway lifecycle and status", async () => {
    const status = await backend.gatewayStatus();
    expect(status.running).toBe(true);
    expect(status.port).toBe(18888);

    const startAddr = await backend.gatewayStart();
    expect(startAddr).toBe("127.0.0.1:18888");
  });

  it("manages extensions: MCP, skills, prompts, inject", async () => {
    const mcp = await backend.listMcpServers();
    expect(mcp.length).toBe(1);
    expect(mcp[0].id).toBe("filesystem");

    const skills = await backend.listSkills();
    expect(skills.length).toBe(1);

    const prompts = await backend.listPrompts();
    expect(prompts.length).toBe(1);

    const inject = await backend.injectStatus();
    expect(inject.stage).toBe("NativeReady");
  });

  it("queries history and diagnostics", async () => {
    const history = await backend.queryHistory();
    expect(history.length).toBe(1);
    expect(history[0].client).toBe("Codex CLI");

    const diag = await backend.runDiagnostics();
    expect(diag.okCount).toBe(1);
    expect(diag.errors).toBe(0);

    const proxy = await backend.detectProxy();
    expect(proxy.tools.length).toBe(1);
    expect(proxy.tools[0].name).toBe("Clash Verge");
  });
});
