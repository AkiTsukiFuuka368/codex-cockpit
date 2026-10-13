import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const stylesheet = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");

describe("control-center surface isolation", () => {
  it("ships both bundled-font licenses as readable resources on Mac and Windows", () => {
    const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
    for (const [source, target] of Object.entries(config.bundle.resources)) {
      expect(target).toMatch(/^font-licenses\//);
      expect(readFileSync(new URL(`../src-tauri/${source}`, import.meta.url), "utf8")).toContain("SIL OPEN FONT LICENSE");
    }
    expect(config.bundle.resources).toEqual({
      "../public/font-licenses/LXGW-OFL.txt": "font-licenses/LXGW-OFL.txt",
      "../public/font-licenses/Smiley-OFL-1.1.txt": "font-licenses/Smiley-OFL-1.1.txt",
    });
  });

  it("uses a readable common scale and native Windows text rendering", () => {
    expect(stylesheet).toMatch(/:root\s*{[^}]*--ui-font-scale:\s*1\.5;/s);
    expect(stylesheet).toMatch(/--ui-system-font:[^;]*"Segoe UI Variable Text",\s*"Segoe UI"/);
    expect(stylesheet).toMatch(/--ui-codex-font:\s*-apple-system,\s*BlinkMacSystemFont,\s*"Segoe UI",\s*sans-serif;/);
    expect(stylesheet).toMatch(/--ui-yahei-font:\s*"Microsoft YaHei UI",\s*"Microsoft YaHei",\s*"PingFang SC",\s*var\(--ui-system-font\);/);
    expect(stylesheet).toMatch(/html,\s*body,\s*#root\s*{[^}]*font-family:\s*var\(--ui-font-family\);/s);
    const windowsDefault = stylesheet.indexOf('html[data-platform="windows"] { --ui-font-family: var(--ui-system-font); }');
    expect(windowsDefault).toBeGreaterThan(-1);
    for (const id of ["system", "codex", "yahei", "smiley", "wenkai", "times", "fz-song"]) {
      expect(stylesheet.indexOf(`html[data-font-family="${id}"]`)).toBeGreaterThan(windowsDefault);
    }
    expect(stylesheet).toMatch(/button,\s*input,\s*select,\s*textarea\s*{[^}]*font-family:\s*inherit;/s);
    expect(stylesheet).toMatch(/font-family:\s*"LXGW WenKai GB Screen";\s*src:\s*url\("\.\/assets\/fonts\/LXGWWenKaiGBScreen\.woff2"\)/);
    expect(stylesheet).toMatch(/font-family:\s*"Cockpit FZ Song";\s*src:\s*local\("FZQingKeBenYueSongS-R-GB"\)/);
    expect(stylesheet).not.toMatch(/@font-face\s*{[^}]*font-family:\s*"Microsoft YaHei"/s);
  });

  it("anchors the percentage's intrinsic box at the circle center, not the full-height flex baseline", () => {
    const valueRule = stylesheet.match(/\.codex-ring-value\s*{([^}]*)}/s)?.[1] ?? "";
    expect(valueRule).toMatch(/top:\s*50%;/);
    expect(valueRule).toMatch(/left:\s*50%;/);
    expect(valueRule).toMatch(/transform:\s*translate\(-50%,\s*-50%\);/);
    expect(valueRule).not.toMatch(/inset:|align-self:|padding-top:/);
    expect(stylesheet).toMatch(/font-size:\s*calc\([^;]*--ring-readout-fit/);
    expect(stylesheet).not.toMatch(/--ring-value-length|--ring-number-space/);
  });

  it("gives reading fonts their own spacing and keeps all cockpit explanations readable", () => {
    for (const id of ["yahei", "wenkai", "times", "fz-song"]) {
      expect(stylesheet).toMatch(new RegExp(`html\\[data-font-family="${id}"\\]\\s*{[^}]*--cockpit-copy-scale:[^}]*--cockpit-copy-leading:[^}]*--cockpit-number-tracking:`, "s"));
    }
    expect(stylesheet).toMatch(/html\[data-font-family="times"\]\s*{[^}]*"Songti SC",\s*"SimSun"/);
    const caption = stylesheet.match(/\.codex-ring-caption\s*{([^}]*)}/s)?.[1] ?? "";
    expect(caption).toMatch(/font-size:\s*max\(12px,/);
    expect(caption).toMatch(/white-space:\s*normal;/);
    expect(caption).not.toMatch(/text-overflow:\s*ellipsis/);
    expect(stylesheet).toMatch(/\.codex-focus-card \.today-quota-advice-grid span\s*{[^}]*white-space:\s*normal;/s);
    expect(stylesheet).toMatch(/\.codex-ring-card--daily \.daily-budget-control p\s*{[^}]*flex-wrap:\s*wrap;/s);
  });

  it("can disable all backdrop blur from one persisted root setting", () => {
    expect(stylesheet).toMatch(/html\[data-background-blur="false"\],[\s\S]*html\[data-background-blur="false"\] \*,[\s\S]*html\[data-background-blur="false"\] \*::before,[\s\S]*html\[data-background-blur="false"\] \*::after\s*{[^}]*backdrop-filter:\s*none\s*!important;[^}]*-webkit-backdrop-filter:\s*none\s*!important;/s);
  });

  it("lets the control center replace the cockpit instead of revealing a second shell", () => {
    expect(stylesheet).toMatch(
      /\.codex-focus-card\.quota-card--overlay-open \.control-center--minimal\s*{[^}]*inset:\s*0;[^}]*border-radius:\s*inherit;/s,
    );
    expect(stylesheet).toMatch(
      /\.codex-focus-card\.quota-card--overlay-open\s*{[^}]*border:\s*0;[^}]*background:\s*transparent;[^}]*box-shadow:\s*none;/s,
    );
    expect(stylesheet).toMatch(
      /\.codex-focus-card\.quota-card--overlay-open::before,[\s\S]*> \.codex-focus-content\s*{[^}]*visibility:\s*hidden;[^}]*opacity:\s*0;/s,
    );
  });
});
