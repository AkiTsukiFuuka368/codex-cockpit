type FrameScheduler = (callback: FrameRequestCallback) => number;

/** Keep the transition hook stable for App while leaving the painted surface
 * untouched. Cloning and opacity-fading the compact surface makes text and
 * rings look soft while Windows is already doing expensive native resizes. */
export function prepareSurfaceCrossfade(): { start: () => void; cancel: () => void } {
  return { start: () => {}, cancel: () => {} };
}

export function waitForSurfacePaint(scheduleFrame: FrameScheduler = window.requestAnimationFrame.bind(window)): Promise<void> {
  return new Promise((resolve) => {
    // The first frame commits the React surface; the second guarantees WebKit
    // has painted it before the native Tauri frame starts growing.
    scheduleFrame(() => scheduleFrame(() => resolve()));
  });
}
