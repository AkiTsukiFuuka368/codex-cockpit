import { isTauri } from "./bridge";

export interface ClaudeUsageRow {
  id: string;
  name: string;
  inputTokens: number;
  cacheWriteTokens: number;
  cacheReadTokens: number;
  outputTokens: number;
  totalTokens: number;
  /** Cost of tokens whose model has a known price. */
  estimatedCostUsd: number;
  /** Some tokens belong to a model without a known price. */
  costIncomplete: boolean;
}

export interface ClaudeDayUsage {
  date: string;
  requests: number;
  models: ClaudeUsageRow[];
  projects: ClaudeUsageRow[];
}

export interface ClaudePriceRow {
  name: string;
  input: number;
  cacheWrite5m: number;
  cacheWrite1h: number;
  cacheRead: number;
  output: number;
  note: string | null;
}

export interface ClaudeCollectorStatus {
  statuslineInstalled: boolean;
  statuslineConflict: boolean;
  wrapperPath: string | null;
  lastObservedAt: string | null;
}

export interface ClaudeUsage {
  status: "ready" | "empty" | "unavailable";
  directory: string | null;
  fetchedAt: string;
  transcriptFiles: number;
  days: ClaudeDayUsage[];
  prices: ClaudePriceRow[];
  priceSource: string;
  collector: ClaudeCollectorStatus;
}

export type ClaudePeriod = "today" | "7d" | "30d" | "all";

const pad = (value: number) => String(value).padStart(2, "0");
export const localDateKey = (date: Date) => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;

/** First local date included in a period, or null for all time. */
export function periodStart(period: ClaudePeriod, now = new Date()): string | null {
  if (period === "all") return null;
  const days = period === "today" ? 0 : period === "7d" ? 6 : 29;
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate() - days);
  return localDateKey(start);
}

const emptyRow = (id: string, name: string): ClaudeUsageRow => ({ id, name, inputTokens: 0, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 0, totalTokens: 0, estimatedCostUsd: 0, costIncomplete: false });

function addRow(target: ClaudeUsageRow, row: ClaudeUsageRow) {
  target.inputTokens += row.inputTokens;
  target.cacheWriteTokens += row.cacheWriteTokens;
  target.cacheReadTokens += row.cacheReadTokens;
  target.outputTokens += row.outputTokens;
  target.totalTokens += row.totalTokens;
  target.estimatedCostUsd += row.estimatedCostUsd;
  target.costIncomplete ||= row.costIncomplete;
}

export interface ClaudeSummary {
  total: ClaudeUsageRow;
  requests: number;
  models: ClaudeUsageRow[];
  projects: ClaudeUsageRow[];
}

export function summarize(days: readonly ClaudeDayUsage[], period: ClaudePeriod, now = new Date()): ClaudeSummary {
  const start = periodStart(period, now);
  const end = localDateKey(now);
  const total = emptyRow("total", "total");
  const models = new Map<string, ClaudeUsageRow>();
  const projects = new Map<string, ClaudeUsageRow>();
  let requests = 0;
  for (const day of days) {
    if ((start && day.date < start) || day.date > end) continue;
    requests += day.requests;
    for (const row of day.models) {
      addRow(total, row);
      const model = models.get(row.id) ?? emptyRow(row.id, row.name);
      addRow(model, row);
      models.set(row.id, model);
    }
    for (const row of day.projects) {
      const project = projects.get(row.id) ?? emptyRow(row.id, row.name);
      addRow(project, row);
      projects.set(row.id, project);
    }
  }
  const sorted = (map: Map<string, ClaudeUsageRow>) => [...map.values()].sort((a, b) => b.totalTokens - a.totalTokens || a.name.localeCompare(b.name));
  return { total, requests, models: sorted(models), projects: sorted(projects) };
}

/** The last `count` local dates, oldest first, with zero-filled totals. */
export function dailySeries(days: readonly ClaudeDayUsage[], count: number, now = new Date()): { date: string; tokens: number; cost: number }[] {
  const byDate = new Map(days.map(day => [day.date, day]));
  return Array.from({ length: count }, (_, index) => {
    const date = localDateKey(new Date(now.getFullYear(), now.getMonth(), now.getDate() - (count - 1 - index)));
    const day = byDate.get(date);
    return {
      date,
      tokens: day?.models.reduce((sum, row) => sum + row.totalTokens, 0) ?? 0,
      cost: day?.models.reduce((sum, row) => sum + row.estimatedCostUsd, 0) ?? 0,
    };
  });
}

function previewUsage(): ClaudeUsage {
  const today = new Date();
  const days: ClaudeDayUsage[] = Array.from({ length: 10 }, (_, index) => {
    const date = localDateKey(new Date(today.getFullYear(), today.getMonth(), today.getDate() - index));
    const scale = 1 + (index % 4);
    const opus: ClaudeUsageRow = { ...emptyRow("claude-opus-5-5", "Claude Opus 5.5"), inputTokens: 12000 * scale, cacheWriteTokens: 90000 * scale, cacheReadTokens: 1_400_000 * scale, outputTokens: 30000 * scale };
    opus.totalTokens = opus.inputTokens + opus.cacheWriteTokens + opus.cacheReadTokens + opus.outputTokens;
    opus.estimatedCostUsd = (opus.inputTokens * 4 + opus.cacheWriteTokens * 5 + opus.cacheReadTokens * .2 + opus.outputTokens * 20) / 1e6;
    const haiku: ClaudeUsageRow = { ...emptyRow("claude-haiku-5-5", "Claude Haiku 5.5"), inputTokens: 8000, cacheReadTokens: 50000, outputTokens: 2000 };
    haiku.totalTokens = 60000;
    haiku.estimatedCostUsd = (8000 * .1 + 50000 * .01 + 2000 * .5) / 1e6;
    const project = { ...opus, id: "demo", name: "示例项目" };
    addRow(project, haiku);
    return { date, requests: 40 * scale, models: [opus, haiku], projects: [project] };
  });
  return {
    status: "ready", directory: null, fetchedAt: today.toISOString(), transcriptFiles: 12, days, prices: [], priceSource: "sample",
    collector: { statuslineInstalled: false, statuslineConflict: false, wrapperPath: null, lastObservedAt: null },
  };
}

export async function getClaudeUsage(): Promise<ClaudeUsage> {
  if (!isTauri()) return previewUsage();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ClaudeUsage>("get_claude_usage");
}

export async function installClaudeCollector(replace = false): Promise<ClaudeCollectorStatus> {
  if (!isTauri()) throw new Error("preview");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ClaudeCollectorStatus>("install_claude_collector", { replace });
}
