import { describe, expect, it } from "vitest";
import { dailySeries, periodStart, summarize, type ClaudeDayUsage } from "./claudeUsage";

const row = (id: string, tokens: number, cost: number, incomplete = false) => ({ id, name: id, inputTokens: tokens, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 0, totalTokens: tokens, estimatedCostUsd: cost, costIncomplete: incomplete });
const day = (date: string, models: ReturnType<typeof row>[]): ClaudeDayUsage => ({ date, requests: models.length, models, projects: [row("proj", models.reduce((s, m) => s + m.totalTokens, 0), models.reduce((s, m) => s + m.estimatedCostUsd, 0))] });

describe("claude usage", () => {
  const now = new Date(2026, 9, 18, 12);
  const days = [day("2026-09-01", [row("old", 999, 9)]), day("2026-10-12", [row("opus", 100, 1)]), day("2026-10-18", [row("opus", 10, .1), row("mystery", 5, 0, true)])];

  it("computes period starts in local dates", () => {
    expect(periodStart("today", now)).toBe("2026-10-18");
    expect(periodStart("7d", now)).toBe("2026-10-12");
    expect(periodStart("30d", now)).toBe("2026-09-19");
    expect(periodStart("all", now)).toBeNull();
  });

  it("summarizes by period and marks incomplete costs", () => {
    const today = summarize(days, "today", now);
    expect(today.total.totalTokens).toBe(15);
    expect(today.total.costIncomplete).toBe(true);
    expect(today.models.map(m => m.id)).toEqual(["opus", "mystery"]);
    const week = summarize(days, "7d", now);
    expect(week.total.totalTokens).toBe(115);
    expect(week.models[0]).toMatchObject({ id: "opus", totalTokens: 110 });
    expect(summarize(days, "all", now).total.totalTokens).toBe(1114);
  });

  it("zero-fills the daily series", () => {
    const series = dailySeries(days, 7, now);
    expect(series).toHaveLength(7);
    expect(series[0]).toEqual({ date: "2026-10-12", tokens: 100, cost: 1 });
    expect(series[6].tokens).toBe(15);
    expect(series[3].tokens).toBe(0);
  });
});
