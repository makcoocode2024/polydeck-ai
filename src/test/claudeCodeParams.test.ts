import { describe, it, expect } from "vitest";
import {
  resolveClaudeCodeParams,
  OUTPUT_TOKENS_WITH_THINKING,
  OUTPUT_TOKENS_WITHOUT_THINKING,
  THINKING_TOKENS_DEFAULT,
} from "@/domain/profile";

describe("resolveClaudeCodeParams", () => {
  it("treats only signed thinking as supported", () => {
    for (const s of ["unprobed", "unsigned", "absent"] as const) {
      const r = resolveClaudeCodeParams({ thinkingSupport: s }, undefined);
      expect(r.thinkingSupported, s).toBe(false);
      expect(r.maxThinkingTokens, s).toBeNull();
      expect(r.maxOutputTokens, s).toBe(OUTPUT_TOKENS_WITHOUT_THINKING);
    }
    const signed = resolveClaudeCodeParams({ thinkingSupport: "signed" }, undefined);
    expect(signed.thinkingSupported).toBe(true);
    expect(signed.maxThinkingTokens).toBe(THINKING_TOKENS_DEFAULT);
    expect(signed.maxOutputTokens).toBe(OUTPUT_TOKENS_WITH_THINKING);
  });

  it("prefers a probed ceiling over the built-in table", () => {
    const r = resolveClaudeCodeParams(
      { thinkingSupport: "absent", probedMaxOutputTokens: 16384 },
      undefined,
    );
    expect(r.maxOutputTokens).toBe(16384);
  });

  it("lets a manual value win over the probe", () => {
    const r = resolveClaudeCodeParams(
      { thinkingSupport: "signed", probedMaxOutputTokens: 16384 },
      { maxOutputTokens: 65536, disableAutoupdater: false },
    );
    expect(r.maxOutputTokens).toBe(65536);
  });

  it("never reports a thinking ceiling for an unsupported model, even if one is stored", () => {
    const r = resolveClaudeCodeParams(
      { thinkingSupport: "unsigned" },
      { maxThinkingTokens: 12288, disableAutoupdater: false },
    );
    expect(r.maxThinkingTokens).toBeNull();
  });

  it("falls back to the no-thinking default when the provider is unknown", () => {
    const r = resolveClaudeCodeParams(undefined, undefined);
    expect(r.thinkingSupported).toBe(false);
    expect(r.maxOutputTokens).toBe(OUTPUT_TOKENS_WITHOUT_THINKING);
  });
});
