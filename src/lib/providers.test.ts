import { describe, expect, it } from "vitest";
import { normalizeProviderOrder } from "./providers";

describe("provider ordering", () => {
  it("preserves a custom order and appends missing providers", () => {
    expect(normalizeProviderOrder(["volcengine", "codex"])).toEqual([
      "volcengine",
      "codex",
      "claude",
      "qoder",
      "trae",
      "workbuddy",
      "antigravity",
    ]);
  });

  it("deduplicates provider ids", () => {
    expect(normalizeProviderOrder(["qoder", "qoder", "codex"])).toEqual([
      "qoder",
      "codex",
      "claude",
      "trae",
      "workbuddy",
      "volcengine",
      "antigravity",
    ]);
  });
});
