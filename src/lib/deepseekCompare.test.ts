import { describe, expect, it } from "vitest";
import { deepseekFlashEquivalent } from "./deepseekCompare";

describe("deepseekFlashEquivalent", () => {
  it("prices uncached, cached and output tokens at Flash rates", () => {
    // Codex shape: input 1.2M including 1M cached, output 0.1M → total 1.3M.
    const codex = deepseekFlashEquivalent({ totalTokens: 1_300_000, cachedInputTokens: 1_000_000, outputTokens: 100_000 })!;
    expect(codex.offPeakUsd).toBeCloseTo(0.2 * 0.15 + 1 * 0.003 + 0.1 * 0.6, 10);
    expect(codex.peakUsd).toBeCloseTo(codex.offPeakUsd * 2, 10);
    // Claude shape: input 0.2M uncached + 1M cache read + 0.1M output → same total, same price.
    const claude = deepseekFlashEquivalent({ totalTokens: 1_300_000, cachedInputTokens: 1_000_000, outputTokens: 100_000 })!;
    expect(claude.offPeakUsd).toBeCloseTo(codex.offPeakUsd, 10);
  });
  it("returns null without usage", () => {
    expect(deepseekFlashEquivalent({ totalTokens: 0, cachedInputTokens: 0, outputTokens: 0 })).toBeNull();
    expect(deepseekFlashEquivalent({ totalTokens: Number.NaN, cachedInputTokens: 0, outputTokens: 0 })).toBeNull();
  });
});
