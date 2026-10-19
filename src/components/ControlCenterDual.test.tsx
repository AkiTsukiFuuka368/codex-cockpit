// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { DEFAULT_WIDGET_PREFERENCES } from "../lib/preferences";
import { ControlCenter } from "./ControlCenter";

afterEach(cleanup);

it("shows Codex and Claude usage side by side with the same panel", async () => {
  render(<ControlCenter preferences={{ ...DEFAULT_WIDGET_PREFERENCES, language: "zh-CN" }} language="zh-CN" onClose={vi.fn()} onRefresh={vi.fn()} onPreferences={vi.fn()} autostartEnabled={false} onAutostart={vi.fn()} />);
  const codex = screen.getByRole("region", { name: "Codex" });
  const claude = screen.getByRole("region", { name: "Claude" });
  expect(await within(claude).findByLabelText("Claude 用量")).toBeInTheDocument();
  expect(within(codex).getByLabelText("Codex 用量")).toBeInTheDocument();
  // Same controls in both columns.
  expect(within(claude).getByLabelText("统计时间")).toBeInTheDocument();
  expect(within(claude).getByLabelText("查看方式")).toBeInTheDocument();
  expect(await within(claude).findByText("订阅额度采集", { exact: false })).toBeInTheDocument();
});
