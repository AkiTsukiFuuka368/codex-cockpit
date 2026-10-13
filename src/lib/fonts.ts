import type { UiFontFamily } from "../types";

export const UI_FONT_OPTIONS = [
  { id: "system", zh: "系统默认", en: "System default", localOnly: false },
  { id: "codex", zh: "Codex 风格", en: "Codex-style", localOnly: false },
  { id: "yahei", zh: "微软雅黑", en: "Microsoft YaHei", localOnly: true },
  { id: "smiley", zh: "得意黑 · Smiley Sans", en: "Smiley Sans", localOnly: false },
  { id: "wenkai", zh: "霞鹜文楷 · 屏幕版", en: "LXGW WenKai GB Screen", localOnly: false },
  { id: "times", zh: "Times New Roman", en: "Times New Roman", localOnly: true },
  { id: "fz-song", zh: "方正清刻本悦宋", en: "FZ QingKeBenYue Song", localOnly: true },
] as const satisfies readonly { id: UiFontFamily; zh: string; en: string; localOnly: boolean }[];

export function defaultFontFamilyForPlatform(userAgent: string): UiFontFamily {
  // Preserve the existing platform defaults when upgrading older preferences.
  return /Windows/i.test(userAgent) ? "system" : "smiley";
}

export function normalizeFontFamily(value: unknown, fallback: UiFontFamily): UiFontFamily {
  return UI_FONT_OPTIONS.some(option => option.id === value) ? value as UiFontFamily : fallback;
}
