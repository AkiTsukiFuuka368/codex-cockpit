import type { ModelUsage } from "./tokeiUsage";

// The existing research colorbar, sampled continuously at equal positions.
const PALETTE = [[231, 98, 84], [239, 138, 71], [247, 170, 88], [255, 208, 111], [255, 230, 183], [170, 220, 224], [114, 188, 213], [82, 143, 173], [55, 103, 149], [30, 70, 110]];
export const UNRANKED_MODEL_COLOR = "#8c98a8";

export function colorbarSample(position: number): string {
  const x = Math.max(0, Math.min(1, position)) * (PALETTE.length - 1);
  const left = Math.floor(x);
  const right = Math.min(PALETTE.length - 1, left + 1);
  return `#${PALETTE[left].map((value, channel) => Math.round(value + (PALETTE[right][channel] - value) * (x - left)).toString(16).padStart(2, "0")).join("")}`;
}

const CLAUDE_TIERS: Record<string, number> = { mythos: 4, fable: 3, opus: 2, sonnet: 1, haiku: 0 };

function generation(model: Pick<ModelUsage, "id" | "name">): number[] | null {
  const text = `${model.id} ${model.name}`;
  const match = text.match(/(?:^|[/:\s])gpt[-\s]?(\d+)(?:\.(\d+))?(?:\.(\d+))?/i);
  if (match) return [Number(match[1]), Number(match[2] ?? 0), Number(match[3] ?? 0)];
  // Claude: newer versions first, then the larger tier within a version.
  const claude = text.match(/claude-(mythos|fable|opus|sonnet|haiku)-(\d+)(?:-(\d{1,2})(?!\d))?/i);
  return claude ? [Number(claude[2]), Number(claude[3] ?? 0), CLAUDE_TIERS[claude[1].toLowerCase()]] : null;
}

/** Numeric GPT generation, not usage rank, strength or inferred release dates. Same-generation names break ties. */
export function modelUsageColors(visibleModels: Pick<ModelUsage, "id" | "name">[]): Record<string, string> {
  const ranked = visibleModels.filter(model => generation(model) !== null).sort((a, b) => {
    const av = generation(a)!; const bv = generation(b)!;
    for (let i = 0; i < av.length; i++) if (av[i] !== bv[i]) return bv[i] - av[i];
    return a.id.localeCompare(b.id);
  });
  return Object.fromEntries(ranked.map((model, index) => [model.id, colorbarSample(index / Math.max(1, ranked.length - 1))]));
}
