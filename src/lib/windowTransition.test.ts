// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { prepareSurfaceCrossfade, waitForSurfacePaint } from "./windowTransition";

describe("waitForSurfacePaint", () => {
  it("waits for two animation frames before releasing the native resize", async () => {
    const frames: FrameRequestCallback[] = [];
    const schedule = vi.fn((callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    const ready = vi.fn();

    void waitForSurfacePaint(schedule).then(ready);
    expect(frames).toHaveLength(1);
    frames.shift()?.(0);
    await Promise.resolve();
    expect(ready).not.toHaveBeenCalled();
    expect(frames).toHaveLength(1);
    frames.shift()?.(16);
    await Promise.resolve();
    expect(ready).toHaveBeenCalledOnce();
  });
});

describe("surface crossfade", () => {
  it("keeps the live surface crisp without cloning or opacity animation", () => {
    document.body.innerHTML = '<div id="root"><main class="quota-card"><button id="action">Open</button></main></div>';
    const animate = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "animate", { configurable: true, value: animate });
    const reveal = prepareSurfaceCrossfade();
    reveal.start();
    reveal.cancel();
    expect(document.querySelectorAll("body > .quota-card, body > .quota-island")).toHaveLength(0);
    expect(animate).not.toHaveBeenCalled();
  });
});
