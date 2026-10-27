import { describe, expect, it } from "vitest";
import { unpricedModels } from "./customPrices";
import type { TokeiUsage } from "./tokeiUsage";

const metrics = { inputTokens: 1, cachedInputTokens: 0, outputTokens: 1, reasoningTokens: 0, totalTokens: 2 };

describe("unpricedModels", () => {
  it("lists models with tokens but no cost, once", () => {
    const usage = { devices: [{ id: "pc", updatedAt: "", stale: false, ranges: {}, daily: {
      "2026-10-26": { ...metrics, estimatedCostUsd: null, models: [{ ...metrics, id: "openai/gpt-7-terra", name: "x", estimatedCostUsd: null }, { ...metrics, id: "gpt-6.1-sol", name: "y", estimatedCostUsd: 1 }] },
      "2026-10-27": { ...metrics, estimatedCostUsd: null, models: [{ ...metrics, id: "gpt-7-terra", name: "x", estimatedCostUsd: null }] },
    } }] } as unknown as TokeiUsage;
    expect(unpricedModels(usage)).toEqual(["gpt-7-terra"]);
    expect(unpricedModels(null)).toEqual([]);
  });
});
