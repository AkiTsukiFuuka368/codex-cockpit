import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getProjectUsage, getTokeiUsage } from "./tokeiBridge";
import { aggregateGlobalUsage, createUsagePreview, type ProjectUsageSnapshot, type TokeiUsage } from "./tokeiUsage";

const api = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: api.invoke }));

const now = new Date(2026, 8, 10, 12);
function standalone() {
  const preview = createUsagePreview(now);
  const usage: TokeiUsage = { ...preview, status: "unavailable", devices: [], groups: [], defaultGroupId: null, localGroupId: null, warnings: ["config_unavailable", "sync_unavailable"] };
  const projects: ProjectUsageSnapshot = { deviceId: null, updatedAt: preview.fetchedAt, status: "partial", coverage: "local", scannedFiles: 1, pricingSource: "builtin", pricingUpdatedAt: null, warnings: ["device_identity_unavailable"], projects: [{ id: "synthetic-project", name: "Synthetic project", daily: preview.devices[0].daily }], tasks: [] };
  api.invoke.mockImplementation(async (command: string) => {
    if (command === "get_tokei_usage") return usage;
    if (command === "get_codex_project_usage") return projects;
    throw new Error(`Unexpected command: ${command}`);
  });
  return { usage, projects };
}

beforeEach(() => { vi.stubGlobal("window", { __TAURI_INTERNALS__: { invoke: api.invoke } }); api.invoke.mockReset(); });
afterEach(() => vi.unstubAllGlobals());

describe("native standalone usage", () => {
  it("shows local tokens without Tokei, sync settings or a saved snapshot", async () => {
    const { usage, projects } = standalone();
    const result = await getTokeiUsage();
    const local = await getProjectUsage();
    const summary = aggregateGlobalUsage(result, "all", now);
    expect(summary.hasData).toBe(true);
    expect(summary.totalTokens).toBe(2_000_000);
    expect(result.devices).toHaveLength(1);
    expect(result.devices[0].id).toBe(local.deviceId);
    expect(local.deviceId).toBeTruthy();
    expect(result.status).toBe("ready");
    expect(local.status).toBe("ready");
    expect(result.warnings).toEqual([]);
    expect(usage.devices).toEqual([]);
    expect(projects.deviceId).toBeNull();
    expect(api.invoke.mock.calls.every(([command]) => ["get_tokei_usage", "get_codex_project_usage"].includes(command))).toBe(true);
  });

  it("shares concurrent native collection between aggregate and task readers", async () => {
    standalone();
    await Promise.all([getTokeiUsage(), getProjectUsage(), getProjectUsage()]);
    expect(api.invoke.mock.calls.filter(([command]) => command === "get_codex_project_usage")).toHaveLength(1);
  });

  it("keeps partial coverage and unknown cost visible", async () => {
    const { projects } = standalone();
    projects.warnings.push("scan_limit");
    for (const day of Object.values(projects.projects[0].daily)) {
      day.estimatedCostUsd = null;
      day.models.forEach(model => { model.estimatedCostUsd = null; });
    }
    const result = await getTokeiUsage();
    expect(result.status).toBe("partial");
    expect(result.devices[0].collectionPartial).toBe(true);
    expect(result.warnings).toContain("scan_limit");
    expect(aggregateGlobalUsage(result, "all", now).estimatedCostUsd).toBeNull();
  });

  it("rejects failed local reads so callers retain last-known-good data, then retries", async () => {
    const { usage } = standalone();
    api.invoke.mockImplementation(async (command: string) => {
      if (command === "get_tokei_usage") return usage;
      throw new Error("collector_busy");
    });
    await expect(getTokeiUsage()).rejects.toThrow("collector_busy");
    standalone();
    expect((await getTokeiUsage()).devices).toHaveLength(1);
  });

  it("keeps existing peer snapshots available if local collection fails", async () => {
    const { usage } = standalone();
    usage.devices = createUsagePreview(now).devices;
    api.invoke.mockImplementation(async (command: string) => {
      if (command === "get_tokei_usage") return usage;
      throw new Error("collector_busy");
    });
    expect(await getTokeiUsage()).toBe(usage);
  });

  it("preserves configured identity and does not duplicate an existing local device", async () => {
    const { usage, projects } = standalone();
    const preview = createUsagePreview(now);
    Object.assign(usage, preview);
    projects.deviceId = preview.devices[0].id;
    projects.status = "ready";
    projects.warnings = [];
    const result = await getTokeiUsage();
    expect(result.devices).toHaveLength(1);
    expect(result.groups).toEqual(preview.groups);
    expect((await getProjectUsage()).deviceId).toBe(preview.devices[0].id);
  });
});
