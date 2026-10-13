import { isTauri } from "./bridge";
import darkIcon from "../../assets/app-icon-dark.png";
import lightIcon from "../../assets/app-icon-light.png";

export async function setAppIconTheme(theme: "light" | "dark"): Promise<void> {
  const favicon = document.querySelector<HTMLLinkElement>('link[rel="icon"]');
  if (favicon) favicon.href = theme === "dark" ? darkIcon : lightIcon;
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("set_app_icon_theme", { theme });
}
