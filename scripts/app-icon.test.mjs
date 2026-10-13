import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { decodePng, encodePng, iconPixelsMatch, roundIcon, ICON_ARTWORK_SCALE } from "./round-app-icons.mjs";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url));

function pngSize(path) {
  const bytes = read(path);
  expect(bytes.subarray(0, 8).toString("hex")).toBe("89504e470d0a1a0a");
  expect(bytes.subarray(12, 16).toString("ascii")).toBe("IHDR");
  return [bytes.readUInt32BE(16), bytes.readUInt32BE(20)];
}

describe("shared desktop app icon", () => {
  it("checks exact icon pixels across PNG encodings and rejects changed artwork", () => {
    const bytes = read("src-tauri/app-icon-dark.png");
    const decoded = decodePng(bytes);
    expect(iconPixelsMatch(bytes, decoded)).toBe(true);
    expect(iconPixelsMatch(encodePng(decoded), decoded)).toBe(true);
    const changed = { ...decoded, rgba: Buffer.from(decoded.rgba) };
    changed.rgba[0] ^= 1;
    expect(iconPixelsMatch(bytes, changed)).toBe(false);
    expect(iconPixelsMatch(bytes, { ...decoded, width: decoded.width - 1 })).toBe(false);
    expect(() => iconPixelsMatch(Buffer.from("invalid"), decoded)).toThrow();
  });
  it("fits the original artwork with extra breathing room and transparent corners", () => {
    for (const theme of ["light", "dark"]) {
      const original = decodePng(read(`src-tauri/app-icon-original-${theme}.png`));
      const actual = decodePng(read(`src-tauri/app-icon-${theme}.png`));
      expect([actual.width, actual.height]).toEqual([original.width, original.height]);
      expect(actual.rgba.equals(roundIcon(original).rgba)).toBe(true);
      const { width, height, rgba } = actual;
      for (const [x, y] of [[0, 0], [width - 1, 0], [0, height - 1], [width - 1, height - 1]]) {
        expect(rgba[(y * width + x) * 4 + 3]).toBe(0);
      }
      expect(rgba[((height >> 1) * width + (width >> 1)) * 4 + 3]).toBe(255);
      // Require a clean transparent perimeter, not noisy background extraction.
      const margin = Math.floor(width * 0.04);
      let cleanPerimeter = true;
      for (let y = 0; y < height; y += 1) {
        for (let x = 0; x < width; x += 1) {
          if (x < margin || y < margin || x >= width - margin || y >= height - margin) {
            if (rgba[(y * width + x) * 4 + 3] !== 0) cleanPerimeter = false;
          }
        }
      }
      expect(cleanPerimeter).toBe(true);
    }
  });

  it("leaves background space inside the tile instead of clipping more artwork", () => {
    const width = 128;
    const original = { width, height: width, rgba: Buffer.alloc(width * width * 4) };
    const background = [16, 24, 32, 255];
    const artwork = [220, 120, 60, 255];
    for (let pixel = 0; pixel < width * width; pixel += 1) original.rgba.set(artwork, pixel * 4);
    for (const pixel of [0, width - 1, width * (width - 1), width * width - 1]) {
      original.rgba.set(background, pixel * 4);
    }
    const actual = roundIcon(original);
    const sample = (x, y) => [...actual.rgba.subarray((y * width + x) * 4, (y * width + x) * 4 + 4)];
    expect(ICON_ARTWORK_SCALE).toBeLessThanOrEqual(0.84);
    expect(ICON_ARTWORK_SCALE).toBeGreaterThanOrEqual(0.8);
    expect(sample(9, 64)).toEqual(background);
    expect(sample(118, 64)).toEqual(background);
    expect(sample(64, 9)).toEqual(background);
    expect(sample(64, 118)).toEqual(background);
    expect(sample(64, 64)).toEqual(artwork);
    expect(sample(0, 0)[3]).toBe(0);
  });

  it("pairs light and dark sources with matching native formats", () => {
    expect(read("src-tauri/app-icon-dark.png").equals(read("src-tauri/app-icon.png"))).toBe(true);
    for (const theme of ["light", "dark"]) {
      const [width, height] = pngSize(`src-tauri/app-icon-${theme}.png`);
      expect(width).toBe(height);
      expect(width).toBeGreaterThanOrEqual(1024);
      // A fully opaque square must never return as a theme icon.
      expect(read(`src-tauri/app-icon-${theme}.png`)[25]).toBe(6);
      expect(read(`assets/app-icon-${theme}.png`).equals(read(`src-tauri/app-icon-${theme}.png`))).toBe(true);
    }
    expect(read("src-tauri/app-icon-light.png").equals(read("src-tauri/app-icon-dark.png"))).toBe(false);
    expect(pngSize("src-tauri/icons/light/icon.png")).toEqual([512, 512]);
    expect(read("src-tauri/icons/light/icon.icns").subarray(0, 4).toString()).toBe("icns");
    expect(read("src-tauri/icons/light/icon.ico").readUInt16LE(2)).toBe(1);
  });
  it("uses the same high-resolution square source for native and browser icons", () => {
    const [width, height] = pngSize("src-tauri/app-icon.png");
    expect(width).toBe(height);
    expect(width).toBeGreaterThanOrEqual(1024);
    expect(read("assets/app-icon.png").equals(read("src-tauri/app-icon.png"))).toBe(true);
    expect(read("index.html").toString()).toContain('type="image/png" href="/assets/app-icon.png"');
  });

  it("includes the exact desktop PNG sizes used by Tauri", () => {
    for (const [name, size] of [["icon.png", 512], ["32x32.png", 32], ["64x64.png", 64], ["128x128.png", 128], ["128x128@2x.png", 256]]) {
      expect(pngSize(`src-tauri/icons/${name}`)).toEqual([size, size]);
    }
    const config = JSON.parse(read("src-tauri/tauri.conf.json").toString());
    for (const path of config.bundle.icon) expect(read(`src-tauri/${path}`).length).toBeGreaterThan(0);
    expect(config.bundle.icon).toEqual(expect.arrayContaining(["icons/icon.icns", "icons/icon.ico"]));
  });

  it("provides macOS icon frames including 512 and 1024 pixels", () => {
    const bytes = read("src-tauri/icons/icon.icns");
    expect(bytes.subarray(0, 4).toString("ascii")).toBe("icns");
    expect(bytes.readUInt32BE(4)).toBe(bytes.length);
    const types = [];
    let offset = 8;
    while (offset < bytes.length) {
      const length = bytes.readUInt32BE(offset + 4);
      expect(length).toBeGreaterThanOrEqual(8);
      types.push(bytes.subarray(offset, offset + 4).toString("ascii"));
      offset += length;
    }
    expect(offset).toBe(bytes.length);
    expect(types).toEqual(expect.arrayContaining(["ic09", "ic10"]));
  });

  it("provides Windows icon frames for small and high-DPI displays", () => {
    const bytes = read("src-tauri/icons/icon.ico");
    expect(bytes.readUInt16LE(0)).toBe(0);
    expect(bytes.readUInt16LE(2)).toBe(1);
    const sizes = [];
    for (let i = 0; i < bytes.readUInt16LE(4); i += 1) {
      const entry = 6 + i * 16;
      sizes.push(bytes[entry] || 256);
      expect(bytes[entry + 1] || 256).toBe(sizes.at(-1));
      expect(bytes.readUInt32LE(entry + 12) + bytes.readUInt32LE(entry + 8)).toBeLessThanOrEqual(bytes.length);
    }
    expect(sizes).toEqual(expect.arrayContaining([16, 32, 256]));
  });
});
