import { useCallback, useEffect, useState } from "react";
import { isTauri } from "../lib/bridge";
import { getClaudeSetup, installClaudeCollector, type ClaudeSetup, type ClaudeWindowCheck } from "../lib/claudeUsage";

const price = (value: number) => `$${Number(value.toFixed(3))}`;

function tokens(value: number, zh: boolean): string {
  if (zh) {
    if (value >= 1e8) return `${(value / 1e8).toFixed(2)} 亿`;
    if (value >= 1e4) return `${(value / 1e4).toFixed(1)} 万`;
    return String(value);
  }
  if (value >= 1e9) return `${(value / 1e9).toFixed(2)}B`;
  if (value >= 1e6) return `${(value / 1e6).toFixed(2)}M`;
  if (value >= 1e3) return `${(value / 1e3).toFixed(1)}K`;
  return String(value);
}

const money = (value: number | null) => value === null ? "—" : `$${value.toFixed(2)}`;

function sourceLabel(kind: string, zh: boolean): string {
  if (kind === "cli") return "Claude Code";
  if (kind === "cowork") return "Cowork";
  return zh ? "Claude 桌面端" : "Claude desktop";
}

function when(value: string, zh: boolean): string {
  return new Date(value).toLocaleString(zh ? "zh-CN" : "en-US", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" });
}

function windowLine(check: ClaudeWindowCheck, zh: boolean): string {
  const name = check.window === "sevenDay" ? (zh ? "每周额度" : "Weekly") : (zh ? "5 小时额度" : "5-hour");
  const used = Math.round(check.usedPercent);
  const after = check.tokensAfterReading > 0
    ? (zh ? `读数之后本机又用了 ${tokens(check.tokensAfterReading, true)} Token（${money(check.costAfterReading)}），还没反映在百分比里。` : ` After the reading this computer used ${tokens(check.tokensAfterReading, false)} more tokens (${money(check.costAfterReading)}), not yet in the percentage.`)
    : "";
  return zh
    ? `${name}：已用 ${used}%（读数 ${when(check.observedAt, true)}，本窗口从 ${when(check.windowStart, true)} 开始）。同一时段本机只记录到 ${tokens(check.localTokens, true)} Token，约 ${money(check.localCostUsd)}。${after}`
    : `${name}: ${used}% used (read ${when(check.observedAt, false)}; window began ${when(check.windowStart, false)}). This computer recorded ${tokens(check.localTokens, false)} tokens (${money(check.localCostUsd)}) in that time.${after}`;
}

function errorText(error: unknown, zh: boolean): string {
  const code = String(error);
  if (code.includes("claude_statusline_conflict")) return zh ? "Claude Code 已有自定义状态栏，未作修改。" : "Claude Code already has a custom status line; nothing was changed.";
  if (code.includes("claude_settings_invalid")) return zh ? "Claude Code 的 settings.json 无法解析，未作修改。" : "Claude Code settings.json could not be parsed; nothing was changed.";
  if (code.includes("claude_dir_missing")) return zh ? "找不到 Claude Code 配置目录。" : "Claude Code config folder not found.";
  if (code.includes("preview")) return zh ? "预览模式下不可用。" : "Unavailable in preview.";
  return zh ? "写入失败，请检查文件权限。" : "Could not write the files. Check permissions.";
}

/** Claude-only extras under the Claude usage column: quota collector setup and the price table. */
export function ClaudeSetupPanel({ zh, onQuotaChanged = () => undefined }: { zh: boolean; onQuotaChanged?: () => void }) {
  const [setup, setSetup] = useState<ClaudeSetup | null>(null);
  const [installing, setInstalling] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const refresh = useCallback(async () => {
    try { setSetup(await getClaudeSetup()); } catch { setSetup(null); }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  if (!setup) return null;
  const collector = setup.collector;
  const lastObserved = collector.lastObservedAt ? new Date(collector.lastObservedAt).toLocaleString(zh ? "zh-CN" : "en-US", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }) : null;
  const wrapperCommand = collector.wrapperPath ? `powershell -NoProfile -ExecutionPolicy Bypass -File "${collector.wrapperPath}" "${zh ? "<任务内容>" : "<task>"}"` : null;

  const install = async (replace: boolean) => {
    setInstalling(true); setMessage(null);
    try {
      const next = await installClaudeCollector(replace);
      setSetup(current => current ? { ...current, collector: next } : current);
      setMessage(zh ? "已配置。下次在 Claude Code 里发出请求后，额度卡片就会出现。" : "Configured. The quota card appears after your next Claude Code request.");
      onQuotaChanged();
      void refresh();
    } catch (reason) {
      setMessage(errorText(reason, zh));
      void refresh();
    } finally { setInstalling(false); }
  };
  const copy = async () => {
    if (!wrapperCommand) return;
    try { await navigator.clipboard.writeText(wrapperCommand); setCopied(true); window.setTimeout(() => setCopied(false), 1500); } catch { setCopied(false); }
  };

  return <div className="claude-extras">
    <details className="usage-explanation claude-coverage" open>
      <summary>{zh ? "统计范围" : "What is counted"}</summary>
      <div>
        {setup.windows.map(check => <p key={check.window}>{windowLine(check, zh)}</p>)}
        <p>{zh ? "本机统计读取这些会话记录：" : "Local usage reads these transcript folders:"}</p>
        <ul>{setup.sources.length
          ? setup.sources.map(source => <li key={source.path}>{sourceLabel(source.kind, zh)} · <code>{source.path}</code> · {zh ? `${source.files} 个文件` : `${source.files} files`}</li>)
          : <li>{zh ? "没有找到会话记录目录" : "No transcript folders found"}</li>}</ul>
        <p className="usage-note">{zh
          ? "这部分不在本机统计范围内：claude.ai 网页和手机 App 的聊天；在云端运行的 Claude 会话（在 claude.ai 里让 Claude 动手做的任务、定时任务、网页版 Claude Code）；其他电脑。它们消耗同一份订阅额度，所以额度百分比通常高于本机统计。"
          : "Not counted here: claude.ai web and mobile chats; Claude sessions that run in the cloud (tasks Claude carries out from claude.ai, scheduled tasks, Claude Code on the web); other computers. They draw on the same subscription, so the quota percentage is usually higher than local usage suggests."}</p>
        <p>{zh
          ? "额度百分比只在你用交互式 Claude Code（终端里的 claude）发请求时更新。claude -p 和 claude-run.ps1 收到的额度事件不带百分比，只在接近上限或被限流时才会更新卡片。"
          : "The percentage updates only when interactive Claude Code (claude in a terminal) makes a request. Events from claude -p and claude-run.ps1 carry no percentage; they update the card only near or at the limit."}</p>
      </div>
    </details>
    <details className="usage-explanation claude-collector" open={!collector.statuslineInstalled}>
      <summary>{zh ? "订阅额度采集" : "Subscription quota"}{collector.statuslineInstalled ? <small>{lastObserved ? (zh ? ` · 最近采集 ${lastObserved}` : ` · last ${lastObserved}`) : (zh ? " · 已配置" : " · configured")}</small> : <small>{zh ? " · 未配置" : " · not set up"}</small>}</summary>
      <div>
        <p>{collector.statuslineInstalled
          ? (lastObserved ? (zh ? "额度卡片会在每次 Claude Code 请求后更新。" : "The quota card updates after each Claude Code request.") : (zh ? "已配置，等待 Claude Code 下一次请求。" : "Configured; waiting for the next Claude Code request."))
          : collector.statuslineConflict
            ? (zh ? "Claude Code 已有你自己的状态栏，没有改动它。可以替换（会先备份 settings.json）。" : "Claude Code already has your own status line. You can replace it; settings.json is backed up first.")
            : !setup.available
              ? (zh ? "这台电脑上还没有 Claude Code 的数据目录（~/.claude）。一键配置会创建它并写入脚本，之后 Claude Code 第一次运行就会开始记录。" : "No Claude Code folder (~/.claude) yet. Setup creates it and writes the scripts; recording starts with the first Claude Code run.")
              : (zh ? "5 小时和每周额度由 Claude Code 自己交给状态栏脚本。配置后驾驶舱只读取它记下的数字，不读取登录凭证。" : "Claude Code reports the 5-hour and weekly windows to status line scripts. The cockpit only reads the recorded numbers, never your credentials.")}</p>
        {isTauri() && <button type="button" className="usage-text-button" disabled={installing} onClick={() => void install(collector.statuslineConflict && !collector.statuslineInstalled)}>
          {installing ? (zh ? "配置中…" : "Configuring…") : collector.statuslineInstalled ? (zh ? "重新写入脚本" : "Rewrite scripts") : collector.statuslineConflict ? (zh ? "替换状态栏并配置" : "Replace and configure") : (zh ? "一键配置" : "Set up")}
        </button>}
        {message && <p className="usage-note" role="status">{message}</p>}
        {wrapperCommand && <>
          <p>{zh ? "让 Codex 用这条命令代替 claude -p：自动找到 claude.exe、读取 token，输出不变，同时记下额度。" : "Have Codex use this instead of claude -p: it finds claude.exe, loads your token, keeps the output and records the quota."}</p>
          <code className="claude-wrapper-command">{wrapperCommand}</code>
          <button type="button" className="usage-text-button" onClick={() => void copy()}>{copied ? (zh ? "已复制" : "Copied") : (zh ? "复制命令" : "Copy command")}</button>
        </>}
      </div>
    </details>
    {setup.prices.length > 0 && <details className="usage-explanation claude-prices">
      <summary>{zh ? "官方 API 价格（美元 / 百万 Token）" : "Official API prices (USD / million tokens)"}</summary>
      <div>
        <table className="claude-price-table">
          <thead><tr><th>{zh ? "模型" : "Model"}</th><th>{zh ? "输入" : "Input"}</th><th>{zh ? "写 5m" : "Write 5m"}</th><th>{zh ? "写 1h" : "Write 1h"}</th><th>{zh ? "缓存读" : "Read"}</th><th>{zh ? "输出" : "Output"}</th></tr></thead>
          <tbody>{setup.prices.map(row => <tr key={`${row.name}-${row.note ?? ""}`}><td>{row.name}{row.note ? <small> {row.note}</small> : null}</td><td>{price(row.input)}</td><td>{price(row.cacheWrite5m)}</td><td>{price(row.cacheWrite1h)}</td><td>{price(row.cacheRead)}</td><td>{price(row.output)}</td></tr>)}</tbody>
        </table>
        <p>{zh ? `来源：${setup.priceSource}。按每次请求的实际 Token（含缓存写入、缓存读取）乘官方单价自动计算；订阅不按此扣费，仅作参考。` : `Source: ${setup.priceSource}. Each request's tokens (including cache writes and reads) times the official rates; subscriptions are not billed this way.`}</p>
      </div>
    </details>}
  </div>;
}
