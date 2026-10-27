import type { UsageMetrics } from "./tokeiUsage";

/**
 * DeepSeek V4.1 Flash (`deepseek-flash`) list prices, USD per million tokens, as a
 * what-if comparison only. Off-peak rates; peak (weekdays 01:00–04:00 and
 * 06:00–10:00 UTC) is exactly double. Checked 2026-10-27 against two independent
 * write-ups of DeepSeek's pricing page (effective 2026-09-10).
 */
export const DEEPSEEK_FLASH = { cacheHit: 0.003, cacheMiss: 0.15, output: 0.6, peakMultiplier: 2, checkedAt: "2026-10-27" } as const;

export interface DeepSeekEstimate { offPeakUsd: number; peakUsd: number }

/**
 * The same tokens priced at DeepSeek V4.1 Flash. Uncached input is derived as
 * total − cached − output, which holds for both sources: Codex's input count
 * includes its cached part, Claude's excludes cache reads (cache writes count as
 * uncached input, since DeepSeek has no write surcharge).
 */
export function deepseekFlashEquivalent(metrics: Pick<UsageMetrics, "totalTokens" | "cachedInputTokens" | "outputTokens">): DeepSeekEstimate | null {
  const { totalTokens, cachedInputTokens, outputTokens } = metrics;
  if (![totalTokens, cachedInputTokens, outputTokens].every(value => Number.isFinite(value) && value >= 0) || totalTokens <= 0) return null;
  const uncached = Math.max(0, totalTokens - cachedInputTokens - outputTokens);
  const offPeakUsd = (uncached * DEEPSEEK_FLASH.cacheMiss + cachedInputTokens * DEEPSEEK_FLASH.cacheHit + outputTokens * DEEPSEEK_FLASH.output) / 1e6;
  return { offPeakUsd, peakUsd: offPeakUsd * DEEPSEEK_FLASH.peakMultiplier };
}
