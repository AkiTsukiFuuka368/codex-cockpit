import { readFileSync, writeFileSync, copyFileSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { deflateSync, inflateSync } from "node:zlib";

const MAGIC = Buffer.from("89504e470d0a1a0a", "hex");
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const ICON_PADDING = 0.06;
export const ICON_CORNER_RADIUS = 0.22;
export const ICON_ARTWORK_SCALE = 0.84;

// Decode only the 8-bit RGB/RGBA PNG formats used by the checked-in icon sources.
export function decodePng(bytes) {
  if (!bytes.subarray(0, 8).equals(MAGIC)) throw new Error("Invalid PNG source");
  const width = bytes.readUInt32BE(16);
  const height = bytes.readUInt32BE(20);
  const channels = bytes[25] === 6 ? 4 : bytes[25] === 2 ? 3 : 0;
  if (!width || !height || width > 4096 || height > 4096 ||
      bytes[24] !== 8 || !channels || bytes[28] !== 0) {
    throw new Error("Icon source must be a bounded, non-interlaced 8-bit RGB/RGBA PNG");
  }
  const compressed = [];
  for (let offset = 8; offset + 12 <= bytes.length;) {
    const length = bytes.readUInt32BE(offset);
    if (offset + length + 12 > bytes.length) throw new Error("Truncated PNG chunk");
    if (bytes.subarray(offset + 4, offset + 8).toString() === "IDAT") {
      compressed.push(bytes.subarray(offset + 8, offset + 8 + length));
    }
    offset += length + 12;
  }
  const stride = width * channels;
  const expected = (stride + 1) * height;
  const raw = inflateSync(Buffer.concat(compressed), { maxOutputLength: expected });
  if (raw.length !== expected) throw new Error("Invalid PNG scanline size");
  const decoded = Buffer.alloc(stride * height);
  const paeth = (a, b, c) => {
    const p = a + b - c;
    const pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
    return pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
  };
  let cursor = 0;
  for (let y = 0; y < height; y += 1) {
    const filter = raw[cursor++];
    if (filter > 4) throw new Error("Invalid PNG filter");
    for (let x = 0; x < stride; x += 1) {
      const index = y * stride + x;
      const left = x >= channels ? decoded[index - channels] : 0;
      const up = y ? decoded[index - stride] : 0;
      const upperLeft = y && x >= channels ? decoded[index - stride - channels] : 0;
      const predictors = [0, left, up, Math.floor((left + up) / 2), paeth(left, up, upperLeft)];
      decoded[index] = (raw[cursor++] + predictors[filter]) & 255;
    }
  }
  const rgba = Buffer.alloc(width * height * 4);
  for (let pixel = 0; pixel < width * height; pixel += 1) {
    decoded.copy(rgba, pixel * 4, pixel * channels, pixel * channels + 3);
    rgba[pixel * 4 + 3] = channels === 4 ? decoded[pixel * 4 + 3] : 255;
  }
  return { width, height, rgba };
}

const crcTable = Array.from({ length: 256 }, (_, value) => {
  for (let bit = 0; bit < 8; bit += 1) value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  return value >>> 0;
});
function chunk(type, data) {
  const name = Buffer.from(type);
  let crc = 0xffffffff;
  for (const byte of Buffer.concat([name, data])) crc = crcTable[(crc ^ byte) & 255] ^ (crc >>> 8);
  const result = Buffer.alloc(data.length + 12);
  result.writeUInt32BE(data.length);
  name.copy(result, 4);
  data.copy(result, 8);
  result.writeUInt32BE((crc ^ 0xffffffff) >>> 0, result.length - 4);
  return result;
}
export function encodePng({ width, height, rgba }) {
  if (rgba.length !== width * height * 4) throw new Error("Invalid RGBA buffer");
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;
  header[9] = 6;
  const stride = width * 4;
  const scanlines = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y += 1) {
    rgba.copy(scanlines, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  return Buffer.concat([MAGIC, chunk("IHDR", header), chunk("IDAT", deflateSync(scanlines)), chunk("IEND", Buffer.alloc(0))]);
}

export function iconPixelsMatch(bytes, expected) {
  const actual = decodePng(bytes);
  return actual.width === expected.width && actual.height === expected.height && actual.rgba.equals(expected.rgba);
}

// Fit the original artwork inside its tile, then apply a geometric alpha mask.
// Bilinear resampling changes size only; it does not redraw or recolor the image.
export function roundIcon(source) {
  const { width, height } = source;
  if (width !== height) throw new Error("Icon source must be square");
  const rgba = Buffer.alloc(source.rgba.length);
  const corners = [0, width - 1, width * (height - 1), width * height - 1];
  const background = [0, 1, 2].map((channel) =>
    Math.round(corners.reduce((sum, pixel) => sum + source.rgba[pixel * 4 + channel], 0) / 4));
  const fill = Uint8Array.from([...background, 255]);
  const inset = width * (1 - ICON_ARTWORK_SCALE) / 2;
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const index = (y * width + x) * 4;
      rgba.set(fill, index);
      if (x + 0.5 < inset || y + 0.5 < inset ||
          x + 0.5 >= width - inset || y + 0.5 >= height - inset) continue;
      const sx = Math.max(0, Math.min(width - 1, (x + 0.5 - width / 2) / ICON_ARTWORK_SCALE + width / 2 - 0.5));
      const sy = Math.max(0, Math.min(height - 1, (y + 0.5 - height / 2) / ICON_ARTWORK_SCALE + height / 2 - 0.5));
      const x0 = Math.floor(sx), y0 = Math.floor(sy);
      const x1 = Math.min(width - 1, x0 + 1), y1 = Math.min(height - 1, y0 + 1);
      const dx = sx - x0, dy = sy - y0;
      for (let channel = 0; channel < 4; channel += 1) {
        const top = source.rgba[(y0 * width + x0) * 4 + channel] * (1 - dx) +
          source.rgba[(y0 * width + x1) * 4 + channel] * dx;
        const bottom = source.rgba[(y1 * width + x0) * 4 + channel] * (1 - dx) +
          source.rgba[(y1 * width + x1) * 4 + channel] * dx;
        rgba[index + channel] = Math.round(top * (1 - dy) + bottom * dy);
      }
    }
  }
  const half = width * (0.5 - ICON_PADDING);
  const radius = half * 2 * ICON_CORNER_RADIUS;
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const qx = Math.abs(x + 0.5 - width / 2) - half + radius;
      const qy = Math.abs(y + 0.5 - height / 2) - half + radius;
      const distance = Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) +
        Math.min(Math.max(qx, qy), 0) - radius;
      const coverage = Math.max(0, Math.min(1, 0.5 - distance));
      const alphaIndex = (y * width + x) * 4 + 3;
      rgba[alphaIndex] = Math.round(rgba[alphaIndex] * coverage);
    }
  }
  return { width, height, rgba };
}

// Tauri also generates mobile assets; keep those outside the desktop checkout.
function generateNativeIcons() {
  const temporary = mkdtempSync(resolve(tmpdir(), "cockpit-native-icons-"));
  try {
    for (const theme of ["dark", "light"]) {
      const output = resolve(temporary, theme);
      const result = spawnSync(process.execPath, [
        resolve(ROOT, "node_modules/@tauri-apps/cli/tauri.js"), "icon",
        resolve(ROOT, "src-tauri/app-icon-" + theme + ".png"),
        "--output", output, "--ios-color", theme === "dark" ? "#000000" : "#ffffff",
      ], { cwd: ROOT, encoding: "utf8" });
      if (result.error || result.status !== 0) {
        throw new Error("Native icon generation failed: " + (result.error?.message ?? result.stderr));
      }
      const destination = resolve(ROOT, "src-tauri/icons", theme === "light" ? "light" : ".");
      mkdirSync(destination, { recursive: true });
      for (const file of readdirSync(output, { withFileTypes: true })) {
        if (file.isFile() && /\.(png|ico|icns)$/.test(file.name)) {
          copyFileSync(resolve(output, file.name), resolve(destination, file.name));
        }
      }
    }
  } finally {
    // This is the unique scratch directory created above, never a user folder.
    rmSync(temporary, { recursive: true, force: true });
  }
  console.log("Mac ICNS and Windows ICO generated from the same rounded theme icons.");
}

function main() {
  const write = process.argv.includes("--write");
  const native = process.argv.includes("--native");
  if (process.argv.slice(2).some((arg) => !["--write", "--check", "--native"].includes(arg)) ||
      (write && process.argv.includes("--check")) || (native && !write)) {
    throw new Error("Usage: node scripts/round-app-icons.mjs --check | --write [--native]");
  }
  for (const theme of ["dark", "light"]) {
    const source = decodePng(readFileSync(resolve(ROOT, "src-tauri/app-icon-original-" + theme + ".png")));
    const rounded = roundIcon(source);
    const output = encodePng(rounded);
    const paths = ["src-tauri/app-icon-" + theme + ".png", "assets/app-icon-" + theme + ".png"];
    if (theme === "dark") paths.push("src-tauri/app-icon.png", "assets/app-icon.png");
    for (const path of paths) {
      if (write) writeFileSync(resolve(ROOT, path), output);
      // zlib versions may encode identical pixels differently across platforms.
      // Require every RGBA byte and both dimensions to match the original artwork.
      else if (!iconPixelsMatch(readFileSync(resolve(ROOT, path)), rounded)) throw new Error("Stale rounded icon: " + path);
    }
  }
  console.log(write ? "Rounded theme icons generated from original artwork with extra breathing room." : "Rounded theme icons match the padded original-artwork layout.");
  if (native) generateNativeIcons();
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  main();
}
