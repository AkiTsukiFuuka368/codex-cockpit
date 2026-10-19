import { isTauri } from "./bridge";
import { createUsagePreview, type DailyModelUsage, type ProjectUsageSnapshot, type TokeiUsage } from "./tokeiUsage";

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

export interface ClaudeSetup {
  available: boolean;
  directory: string | null;
  prices: ClaudePriceRow[];
  priceSource: string;
  collector: ClaudeCollectorStatus;
}

export const CLAUDE_GROUP_ID = "claude-local";
const PREVIEW_DEVICE = "Demo PC";
const PREVIEW_MODELS: Record<string, { id: string; name: string }> = {
  "demo-a": { id: "claude-opus-5-5", name: "Claude Opus 5.5" },
  "demo-b": { id: "claude-haiku-5-5", name: "Claude Haiku 5.5" },
};

/** Sample data in the browser preview: the Codex sample with Claude model names. */
function previewDaily(): Record<string, DailyModelUsage> {
  const sample = createUsagePreview().devices[0]?.daily ?? {};
  return Object.fromEntries(Object.entries(sample).map(([date, day]) => [date, { ...day, models: day.models.map(model => ({ ...model, ...(PREVIEW_MODELS[model.id] ?? {}) })) }]));
}

export async function getClaudeTokeiUsage(): Promise<TokeiUsage> {
  if (!isTauri()) {
    const daily = previewDaily();
    return {
      groups: [{ id: CLAUDE_GROUP_ID, name: PREVIEW_DEVICE, deviceIds: [PREVIEW_DEVICE] }], defaultGroupId: CLAUDE_GROUP_ID, localGroupId: CLAUDE_GROUP_ID,
      fetchedAt: new Date().toISOString(), status: "ready", warnings: [], projectBreakdownAvailable: true,
      devices: [{ id: PREVIEW_DEVICE, updatedAt: new Date().toISOString(), stale: false, daily, ranges: {} }],
    };
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<TokeiUsage>("get_claude_tokei_usage");
}

export async function getClaudeProjectUsage(): Promise<ProjectUsageSnapshot> {
  if (!isTauri()) {
    const daily = previewDaily();
    return {
      deviceId: PREVIEW_DEVICE, updatedAt: new Date().toISOString(), status: "ready", coverage: "local", scannedFiles: 4,
      pricingSource: "bundled-catalog", pricingUpdatedAt: "2026-10-18", taskMetadataCoverage: "complete", warnings: [],
      projects: [{ id: "demo", name: "示例项目", daily }],
      tasks: [{ id: "demo-session", name: "修复登录页面的样式问题", projectName: "示例项目", relation: "root", rootId: "demo-session", daily }],
      peerTasks: [],
    };
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ProjectUsageSnapshot>("get_claude_project_usage");
}

export async function getClaudeSetup(): Promise<ClaudeSetup> {
  if (!isTauri()) {
    return { available: true, directory: null, prices: [], priceSource: "sample", collector: { statuslineInstalled: false, statuslineConflict: false, wrapperPath: null, lastObservedAt: null } };
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ClaudeSetup>("get_claude_setup");
}

export async function installClaudeCollector(replace = false): Promise<ClaudeCollectorStatus> {
  if (!isTauri()) throw new Error("preview");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ClaudeCollectorStatus>("install_claude_collector", { replace });
}
