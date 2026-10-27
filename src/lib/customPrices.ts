import { isTauri } from "./bridge";
import { getTokeiUsage } from "./tokeiBridge";
import { getClaudeTokeiUsage } from "./claudeUsage";
import type { TokeiUsage } from "./tokeiUsage";

/** USD per million tokens. A model ending in `*` matches by prefix. */
export interface CustomPrice {
  model: string;
  input: number;
  cacheRead: number;
  output: number;
  /** Claude cache writes; when absent, 1.25× input (5 min) and 2× input (1 h). */
  cacheWrite?: number | null;
}

let previewPrices: CustomPrice[] = [];

export async function getCustomPrices(): Promise<CustomPrice[]> {
  if (!isTauri()) return previewPrices;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<CustomPrice[]>("get_custom_prices");
}

export async function saveCustomPrices(entries: CustomPrice[]): Promise<CustomPrice[]> {
  const cleaned = entries.map(entry => ({ ...entry, model: entry.model.trim(), cacheWrite: entry.cacheWrite ?? undefined }));
  if (!isTauri()) { previewPrices = cleaned; return cleaned; }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<CustomPrice[]>("save_custom_prices", { entries: cleaned });
}

/** Model ids that have tokens but no price, newest generation first by name. */
export function unpricedModels(usage: TokeiUsage | null): string[] {
  const found = new Set<string>();
  for (const device of usage?.devices ?? []) {
    for (const day of Object.values(device.daily)) {
      for (const model of day.models) {
        if (model.estimatedCostUsd === null && model.totalTokens > 0) found.add(model.id.replace(/^openai\//i, ""));
      }
    }
  }
  return [...found].sort((a, b) => b.localeCompare(a));
}

export async function findUnpricedModels(): Promise<{ codex: string[]; claude: string[] }> {
  const [codex, claude] = await Promise.all([getTokeiUsage().catch(() => null), getClaudeTokeiUsage().catch(() => null)]);
  return { codex: unpricedModels(codex), claude: unpricedModels(claude) };
}
