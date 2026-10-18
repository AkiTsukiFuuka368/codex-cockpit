import { useCallback, useEffect, useMemo, useState } from "react";
import { formatTokens } from "../lib/tokeiUsage";
import { dailySeries, getClaudeUsage, installClaudeCollector, summarize, type ClaudePeriod, type ClaudeUsage, type ClaudeUsageRow } from "../lib/claudeUsage";
import { isTauri } from "../lib/bridge";

const PERIODS: readonly ClaudePeriod[] = ["today", "7d", "30d", "all"];
const periodLabel = (period: ClaudePeriod, zh: boolean) => ({ today: zh ? "今日" : "Today", "7d": zh ? "近 7 天" : "Last 7 days", "30d": zh ? "近 30 天" : "Last 30 days", all: zh ? "全部时间" : "All time" })[period];
const money = (value: number) => `$${value.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
const price = (value: number) => `$${Number(value.toFixed(3))}`;

function Row({ row, max, metric, zh, total }: { row: ClaudeUsageRow; max: number; metric: "tokens" | "cost"; zh: boolean; total: number }) {
  const value = metric === "cost" ? row.estimatedCostUsd : row.totalTokens;
  return <details className="usage-row">
    <summary>
      <span className="usage-row-name" title={row.id}>{row.name}</span>
      <strong>{metric === "cost" ? money(row.estimatedCostUsd) : formatTokens(row.totalTokens, zh)}
        {total > 0 && <small className="usage-share">{(value / total * 100).toFixed(1)}%</small>}
        {metric === "cost" && row.costIncomplete && <small className="usage-share">{zh ? "仅已知金额" : "Known only"}</small>}
      </strong>
      <span className="usage-bar" aria-hidden="true"><i style={{ width: `${max ? Math.min(100, value / max * 100) : 0}%` }} /></span>
    </summary>
    <dl className="usage-metric-details">
      <div><dt>{zh ? "输入（非缓存）" : "Input (uncached)"}</dt><dd>{formatTokens(row.inputTokens, zh)}</dd></div>
      <div><dt>{zh ? "缓存写入" : "Cache writes"}</dt><dd>{formatTokens(row.cacheWriteTokens, zh)}</dd></div>
      <div><dt>{zh ? "缓存读取" : "Cache reads"}</dt><dd>{formatTokens(row.cacheReadTokens, zh)}</dd></div>
      <div><dt>{zh ? "输出" : "Output"}</dt><dd>{formatTokens(row.outputTokens, zh)}</dd></div>
      <div><dt>{zh ? "估算成本（API 价）" : "Estimated API cost"}</dt><dd>{money(row.estimatedCostUsd)}</dd></div>
    </dl>
  </details>;
}

function errorText(error: unknown, zh: boolean): string {
  const code = String(error);
  if (code.includes("claude_statusline_conflict")) return zh ? "Claude Code 已有自定义状态栏，未作修改。" : "Claude Code already has a custom status line; nothing was changed.";
  if (code.includes("claude_settings_invalid")) return zh ? "Claude Code 的 settings.json 无法解析，未作修改。" : "Claude Code settings.json could not be parsed; nothing was changed.";
  if (code.includes("claude_dir_missing")) return zh ? "找不到 Claude Code 配置目录。" : "Claude Code config folder not found.";
  if (code.includes("preview")) return zh ? "预览模式下不可用。" : "Unavailable in preview.";
  return zh ? "写入失败，请检查文件权限。" : "Could not write the files. Check permissions.";
}

export function ClaudeUsagePanel({ zh, onQuotaChanged = () => undefined }: { zh: boolean; onQuotaChanged?: () => void }) {
  const [data, setData] = useState<ClaudeUsage | null>(null);
  const [error, setError] = useState(false);
  const [period, setPeriod] = useState<ClaudePeriod>("today");
  const [view, setView] = useState<"models" | "projects">("models");
  const [metric, setMetric] = useState<"tokens" | "cost">("tokens");
  const [showAll, setShowAll] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installMessage, setInstallMessage] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const refresh = useCallback(async () => {
    try { setData(await getClaudeUsage()); setError(false); } catch { setError(true); }
  }, []);

  useEffect(() => {
    let active = true;
    const run = () => { if (active && document.visibilityState !== "hidden") void refresh(); };
    run();
    const timer = window.setInterval(run, 60000);
    return () => { active = false; window.clearInterval(timer); };
  }, [refresh]);

  const summary = useMemo(() => data ? summarize(data.days, period) : null, [data, period]);
  const series = useMemo(() => data ? dailySeries(data.days, 14) : [], [data]);
  const seriesMax = Math.max(0, ...series.map(day => metric === "cost" ? day.cost : day.tokens));

  const install = async (replace: boolean) => {
    setInstalling(true); setInstallMessage(null);
    try {
      const collector = await installClaudeCollector(replace);
      setData(current => current ? { ...current, collector } : current);
      setInstallMessage(zh ? "已配置。下次在 Claude Code 里发出请求后，额度卡片就会出现。" : "Configured. The quota card appears after your next Claude Code request.");
      onQuotaChanged();
    } catch (reason) {
      setInstallMessage(errorText(reason, zh));
      void refresh();
    } finally { setInstalling(false); }
  };

  const wrapperCommand = data?.collector.wrapperPath
    ? `powershell -NoProfile -ExecutionPolicy Bypass -File "${data.collector.wrapperPath}" "${zh ? "<任务内容>" : "<task>"}"`
    : null;
  const copyWrapper = async () => {
    if (!wrapperCommand) return;
    try { await navigator.clipboard.writeText(wrapperCommand); setCopied(true); window.setTimeout(() => setCopied(false), 1500); } catch { setCopied(false); }
  };

  if (!data) return <section className="codex-usage-panel claude-usage-panel"><p className={error ? "usage-error" : "usage-note"} role="status">{error ? (zh ? "暂时无法读取 Claude 用量。" : "Claude usage is unavailable.") : (zh ? "正在读取 Claude 用量…" : "Loading Claude usage…")}</p></section>;

  const rows = view === "models" ? summary?.models ?? [] : summary?.projects ?? [];
  const sortedRows = [...rows].sort((a, b) => (metric === "cost" ? b.estimatedCostUsd - a.estimatedCostUsd : b.totalTokens - a.totalTokens) || a.name.localeCompare(b.name));
  const visibleRows = showAll ? sortedRows : sortedRows.slice(0, 5);
  const max = sortedRows.length ? (metric === "cost" ? sortedRows[0].estimatedCostUsd : sortedRows[0].totalTokens) : 0;
  const totalValue = summary ? (metric === "cost" ? summary.total.estimatedCostUsd : summary.total.totalTokens) : 0;
  const collector = data.collector;
  const lastObserved = collector.lastObservedAt ? new Date(collector.lastObservedAt).toLocaleString(zh ? "zh-CN" : "en-US", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }) : null;

  return <section className="codex-usage-panel claude-usage-panel" aria-label={zh ? "Claude 用量" : "Claude usage"}>
    <div className="usage-toolbar claude-usage-toolbar">
      <select aria-label={zh ? "统计时间" : "Usage period"} value={period} onChange={(event) => { setPeriod(event.target.value as ClaudePeriod); setShowAll(false); }}>
        {PERIODS.map((value) => <option key={value} value={value}>{periodLabel(value, zh)}</option>)}
      </select>
      <select aria-label={zh ? "查看方式" : "Usage view"} value={view} onChange={(event) => { setView(event.target.value as typeof view); setShowAll(false); }}>
        <option value="models">{zh ? "按模型" : "By model"}</option>
        <option value="projects">{zh ? "按项目" : "By project"}</option>
      </select>
    </div>

    {data.status === "unavailable" && <p className="usage-note" role="status">{zh ? "这台电脑上还没有 Claude Code 的数据目录（~/.claude）。在 Claude 桌面端用过 Code 功能或装好 Claude Code 后，这里会自动出现统计。" : "No Claude Code folder (~/.claude) on this computer yet. Statistics appear after you use Claude Code."}</p>}

    <div className="usage-total">
      <span>{metric === "tokens" ? (zh ? "Token 用量" : "Token usage") : (zh ? "按官方 API 价估算" : "API-equivalent estimate")}</span>
      <strong>{summary && summary.total.totalTokens > 0 ? (metric === "cost" ? money(summary.total.estimatedCostUsd) : formatTokens(summary.total.totalTokens, zh)) : "—"}</strong>
      <small>{metric === "tokens" ? `${zh ? "估算成本" : "Estimated cost"} ${summary ? money(summary.total.estimatedCostUsd) : "—"}` : `${summary ? formatTokens(summary.total.totalTokens, zh) : "—"} Tokens`}{summary?.requests ? ` · ${summary.requests} ${zh ? "次请求" : "requests"}` : ""}{summary?.total.costIncomplete ? (zh ? " · 含未知价格模型" : " · includes unpriced models") : ""}</small>
      <button type="button" className="usage-text-button" onClick={() => setMetric(current => current === "tokens" ? "cost" : "tokens")}>{metric === "tokens" ? (zh ? "看成本" : "Costs") : (zh ? "看 Token" : "Tokens")}</button>
    </div>

    <div className="usage-ranking">
      {visibleRows.map(row => <Row key={row.id} row={row} max={max} metric={metric} zh={zh} total={totalValue} />)}
      {!sortedRows.length && <p className="usage-empty">{zh ? "这段时间没有 Claude Code 用量记录。" : "No Claude Code usage in this period."}</p>}
      {sortedRows.length > 5 && <button type="button" className="usage-text-button" onClick={() => setShowAll(value => !value)}>{showAll ? (zh ? "收起" : "Show less") : (zh ? `其余 ${sortedRows.length - 5} 项` : `${sortedRows.length - 5} more`)}</button>}
    </div>

    {series.some(day => day.tokens > 0) && <div className="claude-daily" aria-label={zh ? "近 14 天" : "Last 14 days"}>
      <span className="claude-daily-title">{zh ? "近 14 天" : "Last 14 days"}</span>
      <div className="claude-daily-bars">
        {series.map(day => {
          const value = metric === "cost" ? day.cost : day.tokens;
          return <i key={day.date} title={`${day.date} · ${metric === "cost" ? money(day.cost) : formatTokens(day.tokens, zh)}`} style={{ height: `${seriesMax ? Math.max(value > 0 ? 4 : 0, value / seriesMax * 100) : 0}%` }} />;
        })}
      </div>
    </div>}

    <section className="minimal-section claude-collector" aria-labelledby="claude-collector-title">
      <header className="minimal-section-header"><strong id="claude-collector-title">{zh ? "订阅额度采集" : "Subscription quota"}</strong></header>
      <p className="usage-note">{collector.statuslineInstalled
        ? (lastObserved ? (zh ? `已配置 · 最近一次采集 ${lastObserved}` : `Configured · last recorded ${lastObserved}`) : (zh ? "已配置 · 等待 Claude Code 下一次请求" : "Configured · waiting for the next Claude Code request"))
        : collector.statuslineConflict
          ? (zh ? "Claude Code 已有你自己的状态栏，没有改动它。可以替换（会先备份 settings.json）。" : "Claude Code already has your own status line. You can replace it; settings.json is backed up first.")
          : (zh ? "额度百分比由 Claude Code 自己提供：它会把 5 小时和每周额度交给状态栏脚本。一键配置后，这里只读取它写下的数字，不读取你的登录凭证。" : "Claude Code itself reports the 5-hour and weekly windows to status line scripts. After setup the cockpit only reads the numbers it records, never your credentials.")}</p>
      <div className="claude-collector-actions">
        {!collector.statuslineInstalled && isTauri() && data.status !== "unavailable" && <button type="button" className="usage-text-button" disabled={installing} onClick={() => void install(collector.statuslineConflict)}>{installing ? (zh ? "配置中…" : "Configuring…") : collector.statuslineConflict ? (zh ? "替换状态栏并配置" : "Replace and configure") : (zh ? "一键配置" : "Set up")}</button>}
        {collector.statuslineInstalled && isTauri() && <button type="button" className="usage-text-button" disabled={installing} onClick={() => void install(false)}>{zh ? "重新写入脚本" : "Rewrite scripts"}</button>}
      </div>
      {installMessage && <p className="usage-note" role="status">{installMessage}</p>}
      {wrapperCommand && <details className="usage-explanation">
        <summary>{zh ? "让 Codex 调用时也采集额度" : "Record quota from Codex runs too"}</summary>
        <div>
          <p>{zh ? "状态栏只在交互式会话里运行。让 Codex 用下面的脚本代替 claude -p：它会自动找到 claude.exe、读取你设置的 token，输出和 claude -p 一样，同时记下额度。" : "The status line only runs in interactive sessions. Have Codex call this script instead of claude -p: it finds claude.exe, loads your token, prints the same output and records the quota."}</p>
          <code className="claude-wrapper-command">{wrapperCommand}</code>
          <button type="button" className="usage-text-button" onClick={() => void copyWrapper()}>{copied ? (zh ? "已复制" : "Copied") : (zh ? "复制命令" : "Copy command")}</button>
        </div>
      </details>}
    </section>

    <details className="usage-explanation claude-prices">
      <summary>{zh ? "官方 API 价格参考（美元 / 百万 Token）" : "Official API prices (USD / million tokens)"}</summary>
      <div>
        <table className="claude-price-table">
          <thead><tr><th>{zh ? "模型" : "Model"}</th><th>{zh ? "输入" : "Input"}</th><th>{zh ? "缓存写 5m" : "Write 5m"}</th><th>{zh ? "缓存写 1h" : "Write 1h"}</th><th>{zh ? "缓存读" : "Read"}</th><th>{zh ? "输出" : "Output"}</th></tr></thead>
          <tbody>{data.prices.map(row => <tr key={`${row.name}-${row.note ?? ""}`}><td>{row.name}{row.note ? <small> {row.note}</small> : null}</td><td>{price(row.input)}</td><td>{price(row.cacheWrite5m)}</td><td>{price(row.cacheWrite1h)}</td><td>{price(row.cacheRead)}</td><td>{price(row.output)}</td></tr>)}</tbody>
        </table>
        <p>{zh ? `来源：${data.priceSource}。金额按标准 API 价格估算，订阅用户不会被这样扣费，仅作参考；未收录的模型不计入金额。` : `Source: ${data.priceSource}. API-equivalent estimates only; subscriptions are not billed this way. Unlisted models are not priced.`}</p>
      </div>
    </details>

    <footer className="usage-footer"><span>{!isTauri() ? (zh ? "示例数据" : "Sample data") : `${zh ? "已扫描" : "Scanned"} ${data.transcriptFiles} ${zh ? "份会话记录" : "transcripts"} · ${new Date(data.fetchedAt).toLocaleTimeString(zh ? "zh-CN" : "en-US", { hour: "2-digit", minute: "2-digit" })}`}</span>
      <button type="button" className="usage-text-button" onClick={() => void refresh()}>{zh ? "刷新" : "Refresh"}</button>
    </footer>
  </section>;
}
