// Development-only preview. No credentials, local sessions or persisted settings.
import React, { useLayoutEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { QuotaCard } from "../src/components/QuotaCard";
import { UI_FONT_OPTIONS } from "../src/lib/fonts";
import { DEFAULT_WIDGET_PREFERENCES } from "../src/lib/preferences";
import { usageDateKey } from "../src/lib/usageDay";
import type { DailyRecommendation } from "../src/lib/dynamicQuota";
import type { Language, ProviderSnapshot, ResolvedAppearance, UiFontFamily } from "../src/types";
import "../src/styles.css";

const now = new Date();
const pro: ProviderSnapshot = {
  provider: "codex", displayName: "CODEX", plan: "PRO", status: "ok", message: null,
  shortWindow: null,
  weeklyWindow: { remainingPercent: 91, resetsAt: new Date(now.getTime() + 6.5 * 86400000).toISOString(), windowSeconds: 604800 },
  resetCredits: 3, updatedAt: now.toISOString(),
};
const plus: ProviderSnapshot = { ...pro, plan: "PLUS", shortWindow: { remainingPercent: 100, resetsAt: new Date(now.getTime() + 4.95 * 3600000).toISOString(), windowSeconds: 18000 } };
const recommendation: DailyRecommendation = {
  targetPercent: 7.6, safetyCapPercent: 100, resetRiskPercent: 9, fatigueKneePercent: 25,
  carryInPercent: 0, futureDays: 5, cycleAge: 1, weekday: 0, historySampleCount: 0,
  source: "local-fallback", policyVersion: "typography-preview",
};
const noop = () => {};
const consuming = new Set<string>();

function TypographyPreview() {
  const [font, setFont] = useState<UiFontFamily>("times");
  const [scale, setScale] = useState(1.5);
  const [appearance, setAppearance] = useState<ResolvedAppearance>("dark");
  const [language, setLanguage] = useState<Language>("zh-CN");
  useLayoutEffect(() => {
    document.documentElement.dataset.fontFamily = font;
    document.documentElement.style.setProperty("--ui-font-scale", String(scale));
  }, [font, scale]);
  return <>
    <div className="font-check-controls">
      <label>字体 <select aria-label="字体" value={font} onChange={event => setFont(event.target.value as UiFontFamily)}>{UI_FONT_OPTIONS.map(option => <option value={option.id} key={option.id}>{option.zh}</option>)}</select></label>
      <label>字号 <select aria-label="字号" value={scale} onChange={event => setScale(Number(event.target.value))}>{[1, 1.5, 2].map(value => <option key={value} value={value}>{value * 100}%</option>)}</select></label>
      <label>外观 <select aria-label="外观" value={appearance} onChange={event => setAppearance(event.target.value as ResolvedAppearance)}><option value="dark">深色</option><option value="light">浅色</option></select></label>
      <label>语言 <select aria-label="语言" value={language} onChange={event => setLanguage(event.target.value as Language)}><option value="zh-CN">中文</option><option value="en">English</option></select></label>
      <span>仅模拟数据；固定 520px 窗口宽度</span>
    </div>
    <div className="font-check-grid">
      {[pro, plus].map(snapshot => <div className="font-check-example" key={snapshot.plan}>
        <h2>{snapshot.plan === "PLUS" ? "Plus · 双环 / 100%" : "Pro · 单环 / 91% 与 118.4%"}</h2>
        <QuotaCard snapshot={snapshot} snapshots={[snapshot]} preferences={{ ...DEFAULT_WIDGET_PREFERENCES, fontFamily: font, fontScale: scale, language, colorTheme: "graphite", appearanceMode: appearance, dailyBudgetPercent: 7.6 }} resolvedAppearance={appearance}
          dailyRecommendation={recommendation}
          codexDailyUsage={{ localDate: usageDateKey(now), observedUsedPercent: 9, coverage: "complete", sampleCount: 1, firstObservedAt: now.toISOString(), lastObservedAt: now.toISOString(), source: "codex-session-rate-limits" }}
          onSelectProvider={noop} onLock={noop} onLanguage={noop} onDrag={noop} onHover={noop} consumingProviders={consuming} />
      </div>)}
    </div>
  </>;
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><TypographyPreview /></React.StrictMode>);
