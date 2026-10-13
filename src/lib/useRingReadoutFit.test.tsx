// @vitest-environment jsdom
import { act, cleanup, render } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ringReadoutFit, useRingReadoutFit } from "./useRingReadoutFit";

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("font-aware ring readouts", () => {
  it("keeps short values at the selected size and shrinks only when the actual glyphs need it", () => {
    expect(ringReadoutFit(66, 59, 174, false)).toBe(1);
    const decimal = ringReadoutFit(124, 59, 174, false);
    expect(decimal).toBeGreaterThan(.8);
    expect(decimal).toBeLessThan(1);
    expect(Math.hypot(124 * decimal, 59 * decimal)).toBeLessThanOrEqual(174 * 98 / 140 - 10 + .0001);
    expect(ringReadoutFit(94, 59, 174, false)).toBeGreaterThan(ringReadoutFit(124, 59, 174, false));
  });

  it("accounts for the Plus inner ring, higher text scales, and long values", () => {
    for (const [width, height] of [[66, 59], [180, 78], [290, 78]]) {
      for (const dual of [false, true]) {
        const scale = ringReadoutFit(width, height, 174, dual);
        expect(scale).toBeGreaterThan(0);
        expect(scale).toBeLessThanOrEqual(1);
        expect(Math.hypot(width * scale, height * scale)).toBeLessThanOrEqual(174 * (dual ? 76 : 98) / 140 - 10 + .0001);
      }
    }
  });

  it("does not invent measurements for hidden or not-yet-laid-out windows", () => {
    for (const invalid of [0, -1, NaN, Infinity]) {
      expect(ringReadoutFit(invalid, 59, 174, false)).toBe(1);
      expect(ringReadoutFit(66, invalid, 174, false)).toBe(1);
      expect(ringReadoutFit(66, 59, invalid, false)).toBe(1);
    }
  });

  it("refits on font, font-loading, resize and value changes, then disconnects on unmount", () => {
    let resizeCallback: () => void = () => {};
    let appearanceCallback: () => void = () => {};
    const resizeDisconnect = vi.fn();
    const appearanceDisconnect = vi.fn();
    vi.stubGlobal("ResizeObserver", class { constructor(callback: () => void) { resizeCallback = callback; } observe = vi.fn(); disconnect = resizeDisconnect; });
    vi.stubGlobal("MutationObserver", class { constructor(callback: () => void) { appearanceCallback = callback; } observe = vi.fn(); disconnect = appearanceDisconnect; });
    const fonts = new EventTarget();
    Object.assign(fonts, { ready: new Promise(() => {}) });
    const originalFonts = Object.getOwnPropertyDescriptor(document, "fonts");
    Object.defineProperty(document, "fonts", { configurable: true, value: fonts });
    let width = 66;
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      const fitted = Number(this.style.getPropertyValue("--ring-readout-fit")) || 1;
      return { width: this.className === "codex-ring-value" ? width * fitted : 174, height: 59 * fitted } as DOMRect;
    });
    function Readout({ value, dual = false }: { value: string; dual?: boolean }) {
      const ref = useRef<HTMLDivElement>(null);
      useRingReadoutFit(ref, value, dual);
      return <div><div className="codex-ring-value" ref={ref}>{value}%</div></div>;
    }
    try {
      const { container, rerender, unmount } = render(<Readout value="91" />);
      const readout = container.querySelector(".codex-ring-value") as HTMLElement;
      expect(readout.style.getPropertyValue("--ring-readout-fit")).toBe("1");
      width = 124;
      act(appearanceCallback);
      expect(Number(readout.style.getPropertyValue("--ring-readout-fit"))).toBeCloseTo(ringReadoutFit(width, 59, 174, false));
      width = 94;
      act(() => { fonts.dispatchEvent(new Event("loadingdone")); });
      expect(readout.style.getPropertyValue("--ring-readout-fit")).toBe("1");
      width = 180;
      act(resizeCallback);
      expect(Number(readout.style.getPropertyValue("--ring-readout-fit"))).toBeLessThan(.6);
      rerender(<Readout value="100" dual />);
      expect(Number(readout.style.getPropertyValue("--ring-readout-fit"))).toBeCloseTo(ringReadoutFit(width, 59, 174, true));
      unmount();
      expect(resizeDisconnect).toHaveBeenCalled();
      expect(appearanceDisconnect).toHaveBeenCalled();
      const measured = vi.mocked(HTMLElement.prototype.getBoundingClientRect).mock.calls.length;
      act(() => { fonts.dispatchEvent(new Event("loadingdone")); });
      expect(vi.mocked(HTMLElement.prototype.getBoundingClientRect)).toHaveBeenCalledTimes(measured);
    } finally {
      if (originalFonts) Object.defineProperty(document, "fonts", originalFonts);
      else Reflect.deleteProperty(document, "fonts");
    }
  });
});
