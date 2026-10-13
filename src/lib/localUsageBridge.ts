import { isTauri } from "./bridge";

export interface LocalUsageSource {
  deviceId: string;
  directory: string;
  selection: "automatic" | "manual";
  state: "ready" | "missing" | "unreadable" | "unsafe" | "invalid" | "preview";
}
export async function getLocalUsageSource(): Promise<LocalUsageSource> {
  if (!isTauri()) return { deviceId: "Demo Mac", directory: "~/.codex", selection: "automatic", state: "preview" };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke("get_local_usage_source");
}
export async function chooseCodexDirectory(): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({ directory: true, multiple: false, title: "选择 Codex 数据目录 / Select Codex data directory" });
  return typeof selected === "string" ? selected : null;
}
export async function saveLocalUsageSource(directory: string | null): Promise<LocalUsageSource> {
  if (!isTauri()) throw new Error("native_app_required");
  const { invoke } = await import("@tauri-apps/api/core");
  const next = await invoke<LocalUsageSource>("save_local_usage_source", { directory, confirmed: true });
  window.dispatchEvent(new Event("codex-source-changed"));
  window.dispatchEvent(new Event("comfort-sync-updated"));
  return next;
}
export function sourceErrorText(error: unknown, zh: boolean): string {
  const code = String(error);
  if (code === "not_codex_directory") return zh ? "这个文件夹不是 Codex 数据目录。请选择包含 sessions 或 auth.json 的文件夹。" : "Choose the Codex data folder containing sessions or auth.json.";
  if (code === "codex_directory_missing") return zh ? "这个文件夹已不存在，请重新选择。" : "This folder no longer exists. Choose it again.";
  if (code === "collector_busy") return zh ? "正在读取用量，请稍等片刻再确认。" : "Usage collection is running. Try again shortly.";
  if (code === "unsafe_source") return zh ? "请选择 Codex 数据目录本身，不要选择整个用户目录或链接文件夹。" : "Select the Codex folder itself, not your home directory or a linked folder.";
  return zh ? "暂时无法读取该目录，请检查系统权限后重试。" : "Unable to read this folder. Check system permissions and try again.";
}
