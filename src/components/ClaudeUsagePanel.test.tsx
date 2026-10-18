// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ClaudeUsagePanel } from "./ClaudeUsagePanel";

afterEach(cleanup);

it("shows preview usage by model and project, and toggles to cost", async () => {
  render(<ClaudeUsagePanel zh />);
  expect(await screen.findByText("Claude Opus 5.5")).toBeInTheDocument();
  expect(screen.getByText("Claude Haiku 5.5")).toBeInTheDocument();
  expect(screen.getByText("订阅额度采集")).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("查看方式"), { target: { value: "projects" } });
  expect(screen.getByText("示例项目")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "看成本" }));
  expect(screen.getByText("按官方 API 价估算")).toBeInTheDocument();
});
