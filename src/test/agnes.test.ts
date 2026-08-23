import { describe, it, expect } from "vitest";
import {
  AGNES_BASE_URL_CN,
  AGNES_BASE_URL_GLOBAL,
  AGNES_DEFAULT_MODEL,
  AGNES_FREE_TIER_RPM,
  AGNES_MODELS,
  AGNES_MODEL_IDS,
  AGNES_ROUTE_KEY_SCOPE_NOTE,
  AGNES_ROUTES,
} from "@/domain/agnes";

/**
 * These pin values measured against the live Agnes endpoint. They mirror
 * `crates/core/src/profile_templates.rs`; if one side changes, the other has to
 * move with it.
 */
describe("Agnes provider constants", () => {
  it("offers both routes, each ending at /v1", () => {
    expect(AGNES_ROUTES).toHaveLength(2);
    expect(AGNES_ROUTES.map((r) => r.id)).toEqual(["agnes-cn", "agnes-global"]);
    for (const route of AGNES_ROUTES) {
      // Agnes 404s on a doubled /v1, and the probe strips one /v1 suffix, so
      // exactly one is what both paths expect.
      expect(route.baseUrl.endsWith("/v1")).toBe(true);
      expect(route.baseUrl).not.toContain("/v1/v1");
      expect(route.baseUrl).not.toContain("/chat/completions");
      expect(route.baseUrl.startsWith("https://")).toBe(true);
    }
  });

  it("keeps the two hosts distinct", () => {
    expect(AGNES_BASE_URL_CN).not.toEqual(AGNES_BASE_URL_GLOBAL);
    expect(AGNES_ROUTES.map((r) => r.baseUrl)).toEqual([
      AGNES_BASE_URL_CN,
      AGNES_BASE_URL_GLOBAL,
    ]);
  });

  it("defaults to a free model that exists in the list", () => {
    expect(AGNES_MODEL_IDS).toContain(AGNES_DEFAULT_MODEL);
    const target = AGNES_MODELS.find((m) => m.id === AGNES_DEFAULT_MODEL);
    expect(target?.free).toBe(true);
  });

  it("offers only chat-capable models", () => {
    // /v1/models also returns image and video models, which answer on
    // /v1/images/generations and /v1/videos and would fail as a chat model.
    for (const id of AGNES_MODEL_IDS) {
      expect(id).not.toContain("image");
      expect(id).not.toContain("video");
    }
    expect(AGNES_MODEL_IDS.length).toBeGreaterThan(0);
  });

  it("pins the measured free-tier rate limit below the generic default", () => {
    // Measured: the 17th request in a minute returns 429. The provider probe
    // falls back to 60 because Agnes sends no RateLimit-* headers.
    expect(AGNES_FREE_TIER_RPM).toBe(20);
    expect(AGNES_FREE_TIER_RPM).toBeLessThan(60);
  });

  it("labels paid models as paid", () => {
    const paid = AGNES_MODELS.filter((m) => !m.free).map((m) => m.id);
    expect(paid).toContain("agnes-2.5-pro");
    expect(paid).toContain("agnes-2.5-pro-alpha");
    // Both flash models are free at current pricing.
    const free = AGNES_MODELS.filter((m) => m.free).map((m) => m.id);
    expect(free).toContain("agnes-2.5-flash");
    expect(free).toContain("agnes-2.0-flash");
  });

  it("gives every model a unique id", () => {
    expect(new Set(AGNES_MODEL_IDS).size).toBe(AGNES_MODEL_IDS.length);
  });

  it("warns that keys are scoped per site", () => {
    // Measured: a CN key gets 200 from the international /v1/models and then
    // 401 on chat. Listing models is not proof the route will serve inference,
    // so the picker has to say so rather than implying a network-only choice.
    expect(AGNES_ROUTE_KEY_SCOPE_NOTE).toMatch(/不通用|401/);
    for (const route of AGNES_ROUTES) {
      expect(route.hint).toMatch(/Key/);
    }
  });
});
