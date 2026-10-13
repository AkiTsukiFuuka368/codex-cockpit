import { describe, expect, it } from "vitest";
import { UI_FONT_OPTIONS, defaultFontFamilyForPlatform, normalizeFontFamily } from "./fonts";

describe("UI font choices", () => {
  it("keeps the five existing choices and adds the PR choices without changing platform defaults", () => {
    expect(UI_FONT_OPTIONS.map(option => option.id)).toEqual([
      "system", "codex", "yahei", "smiley", "wenkai", "times", "fz-song",
    ]);
    expect(defaultFontFamilyForPlatform("Windows NT 10.0")).toBe("system");
    expect(defaultFontFamilyForPlatform("Macintosh; Intel Mac OS X")).toBe("smiley");
  });

  it("preserves every allowlisted preference and safely rejects arbitrary CSS", () => {
    for (const { id } of UI_FONT_OPTIONS) expect(normalizeFontFamily(id, "smiley")).toBe(id);
    expect(normalizeFontFamily('url("https://example.invalid/font")', "system")).toBe("system");
  });

  it("describes the Codex option as a style and Microsoft YaHei as local-only", () => {
    expect(UI_FONT_OPTIONS.find(option => option.id === "codex")).toEqual({
      id: "codex", zh: "Codex 风格", en: "Codex-style", localOnly: false,
    });
    expect(UI_FONT_OPTIONS.find(option => option.id === "yahei")?.localOnly).toBe(true);
  });
});
