import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { aggregateGlobalUsage, aggregateGroupUsage, createUsagePreview, LOCAL_CODEX_DEVICE_ID, type ProjectUsageSnapshot, type TokeiUsage } from "./tokeiUsage";

let getProjectUsage: typeof import("./tokeiBridge")["getProjectUsage"];
let getTokeiUsage: typeof import("./tokeiBridge")["getTokeiUsage"];

const api = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: api.invoke }));

const now = new Date(2026, 8, 10, 12);
function scaleProjects(projects: ProjectUsageSnapshot, factor: number) {
  for (const project of projects.projects) for (const day of Object.values(project.daily)) {
    for (const row of [day, ...day.models]) {
      for (const key of ["inputTokens", "cachedInputTokens", "outputTokens", "reasoningTokens", "totalTokens"] as const) row[key] *= factor;
      if (row.estimatedCostUsd !== null) row.estimatedCostUsd *= factor;
    }
  }
}
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

beforeEach(async () => {
  vi.resetModules();
  vi.stubGlobal("window", { __TAURI_INTERNALS__: { invoke: api.invoke } });
  api.invoke.mockReset();
  ({ getProjectUsage, getTokeiUsage } = await import("./tokeiBridge"));
});
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

  it("keeps an unidentified retained local snapshot without collecting it again", async () => {
    const { usage } = standalone();
    usage.devices = [{ ...createUsagePreview(now).devices[0], id: "Previous-PC" }];
    usage.status = "partial";
    expect(await getTokeiUsage()).toBe(usage);
    expect(aggregateGlobalUsage(usage, "all", now).totalTokens).toBe(2_000_000);
    expect(api.invoke.mock.calls.map(([command]) => command)).toEqual(["get_tokei_usage"]);
  });

  it("keeps a completed scan during a smaller rescan, then adopts the completed correction", async () => {
    const { usage, projects } = standalone();
    const complete = await getTokeiUsage();
    projects.updatedAt = "2026-09-10T08:00:00Z";
    projects.warnings.push("source_reindexed", "scan_limit");
    scaleProjects(projects, .5);
    usage.groups = [{ id: "mine", name: "Synthetic member", deviceIds: [LOCAL_CODEX_DEVICE_ID] }];
    usage.defaultGroupId = "mine";
    const scanning = await getTokeiUsage();
    expect(aggregateGlobalUsage(scanning, "all", now).totalTokens).toBe(2_000_000);
    expect(aggregateGroupUsage(scanning, "mine", "all", now).totalTokens).toBe(2_000_000);
    expect(scanning.groups).toBe(usage.groups);
    expect(scanning.devices).toBe(complete.devices);
    expect(scanning.devices[0].updatedAt).toBe(complete.devices[0].updatedAt);
    expect(scanning.localGroupId).toBeNull();
    projects.warnings = projects.warnings.filter(warning => warning !== "scan_limit");
    const corrected = await getTokeiUsage();
    expect(aggregateGlobalUsage(corrected, "all", now).totalTokens).toBe(1_000_000);
    expect(corrected.warnings).toContain("source_reindexed");
  });

  it("keeps token coverage ready when only task titles are missing", async () => {
    const { projects } = standalone();
    projects.warnings.push("task_metadata_title_missing");
    const data = await getTokeiUsage();
    expect(aggregateGlobalUsage(data, "all", now).totalTokens).toBe(2_000_000);
    expect(data.status).toBe("ready");
    expect(data.devices[0].collectionPartial).toBe(false);
    expect(data.warnings).toContain("task_metadata_title_missing");
  });

  it("drops the standalone baseline on configured reads, including an empty cloud snapshot", async () => {
    const { usage, projects } = standalone();
    await getTokeiUsage();
    usage.warnings = [];
    projects.deviceId = "Cloud-PC";
    const cloud = await getTokeiUsage();
    expect(cloud).toBe(usage);
    expect(cloud.devices).toEqual([]);
    expect((await getProjectUsage()).deviceId).toBe("Cloud-PC");
    usage.warnings = ["config_unavailable", "sync_unavailable"];
    projects.deviceId = null;
    projects.warnings.push("scan_limit");
    scaleProjects(projects, .5);
    expect(aggregateGlobalUsage(await getTokeiUsage(), "all", now).totalTokens).toBe(1_000_000);
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
