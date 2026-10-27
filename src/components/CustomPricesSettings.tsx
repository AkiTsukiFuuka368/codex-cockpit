import { useCallback, useEffect, useState } from "react";
import { findUnpricedModels, getCustomPrices, saveCustomPrices, type CustomPrice } from "../lib/customPrices";

interface Row { model: string; input: string; cacheRead: string; output: string; cacheWrite: string }

const toRow = (entry: CustomPrice): Row => ({ model: entry.model, input: String(entry.input), cacheRead: String(entry.cacheRead), output: String(entry.output), cacheWrite: entry.cacheWrite == null ? "" : String(entry.cacheWrite) });
const EMPTY: Row = { model: "", input: "", cacheRead: "", output: "", cacheWrite: "" };
const number = (text: string) => text.trim() === "" ? NaN : Number(text);

function toEntry(row: Row): CustomPrice | null {
  const model = row.model.trim();
  const input = number(row.input), cacheRead = row.cacheRead.trim() === "" ? 0 : number(row.cacheRead), output = number(row.output);
  const cacheWrite = row.cacheWrite.trim() === "" ? null : number(row.cacheWrite);
  if (!model || ![input, cacheRead, output].every(value => Number.isFinite(value) && value >= 0)) return null;
  if (cacheWrite !== null && !(Number.isFinite(cacheWrite) && cacheWrite >= 0)) return null;
  return { model, input, cacheRead, output, cacheWrite };
}

/** Settings → custom model prices: the user's own rates, used before any built-in price. */
export function CustomPricesSettings({ zh }: { zh: boolean }) {
  const [rows, setRows] = useState<Row[]>([]);
  const [saved, setSaved] = useState<string>("[]");
  const [unpriced, setUnpriced] = useState<{ codex: string[]; claude: string[] }>({ codex: [], claude: [] });
  const [message, setMessage] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    try {
      const entries = await getCustomPrices();
      setRows(entries.map(toRow));
      setSaved(JSON.stringify(entries.map(toRow)));
    } catch { setMessage(zh ? "读取自定义价格失败。" : "Could not read custom prices."); }
    void findUnpricedModels().then(setUnpriced).catch(() => undefined);
  }, [zh]);
  useEffect(() => { void load(); }, [load]);

  const dirty = JSON.stringify(rows) !== saved;
  const invalid = rows.some(row => (row.model.trim() || row.input || row.output) && !toEntry(row));
  const listed = new Set(rows.map(row => row.model.trim().toLowerCase()));
  const suggestions = [...unpriced.codex, ...unpriced.claude].filter(model => !listed.has(model.toLowerCase()));

  const update = (index: number, key: keyof Row, value: string) => setRows(current => current.map((row, i) => i === index ? { ...row, [key]: value } : row));
  const save = async () => {
    const entries = rows.map(toEntry).filter((entry): entry is CustomPrice => entry !== null);
    setSaving(true); setMessage(null);
    try {
      const result = await saveCustomPrices(entries);
      setRows(result.map(toRow)); setSaved(JSON.stringify(result.map(toRow)));
      setMessage(zh ? "已保存。用量页下次刷新时按新价格重算（Codex 会重新扫描一遍本机记录）。" : "Saved. Usage is recalculated on the next refresh.");
      void findUnpricedModels().then(setUnpriced).catch(() => undefined);
    } catch { setMessage(zh ? "保存失败，请检查文件权限。" : "Could not save. Check file permissions."); }
    finally { setSaving(false); }
  };

  return <section className="minimal-section custom-prices">
    <header className="minimal-section-header"><strong>{zh ? "自定义模型价格" : "Custom model prices"}</strong><button type="button" onClick={() => setRows(current => [...current, EMPTY])}>{zh ? "添加" : "Add"}</button></header>
    <p className="custom-prices-hint">{zh
      ? "新模型出来时自己填价格，不用等更新。单位：美元 / 百万 Token。这里的价格优先于内置和共享价格表，只存在本机；价格表本身不会同步，但按它算出的金额会照常出现在同步的用量里。模型名用 Codex / Claude 记录里的 ID，结尾加 * 表示前缀匹配（如 gpt-7*）。缓存写入只对 Claude 有用，留空则按输入价的 1.25 倍（5 分钟）/ 2 倍（1 小时）算。Codex 单次输入超过 272K 时仍按 2 倍 / 1.5 倍计。"
      : "Enter prices for new models yourself. USD per million tokens. These override built-in and shared prices and stay on this computer; the table itself is never synced, though amounts computed from it appear in synced usage. Use the model id from Codex / Claude records; end with * to match a prefix (e.g. gpt-7*). Cache write applies to Claude only; blank means 1.25× input (5 min) / 2× input (1 h). Codex requests over 272K input still use 2× / 1.5×."}</p>
    {suggestions.length > 0 && <p className="custom-prices-missing">{zh ? "这些模型有用量但没有价格：" : "Used but unpriced: "}{suggestions.map(model => <button key={model} type="button" className="usage-text-button" onClick={() => setRows(current => [...current, { ...EMPTY, model }])}>{model}</button>)}</p>}
    {rows.length > 0 && <div className="custom-prices-table" role="table" aria-label={zh ? "自定义模型价格" : "Custom model prices"}>
      <div role="row" className="custom-prices-head"><span>{zh ? "模型" : "Model"}</span><span>{zh ? "输入" : "Input"}</span><span>{zh ? "缓存读" : "Cache read"}</span><span>{zh ? "输出" : "Output"}</span><span>{zh ? "缓存写" : "Cache write"}</span><span /></div>
      {rows.map((row, index) => <div role="row" key={index} className={toEntry(row) || !(row.model.trim() || row.input || row.output) ? undefined : "custom-prices-invalid"}>
        <input aria-label={zh ? "模型" : "Model"} value={row.model} placeholder="gpt-7-terra" onChange={event => update(index, "model", event.target.value)} />
        <input aria-label={zh ? "输入" : "Input"} inputMode="decimal" value={row.input} placeholder="1.00" onChange={event => update(index, "input", event.target.value)} />
        <input aria-label={zh ? "缓存读" : "Cache read"} inputMode="decimal" value={row.cacheRead} placeholder="0.10" onChange={event => update(index, "cacheRead", event.target.value)} />
        <input aria-label={zh ? "输出" : "Output"} inputMode="decimal" value={row.output} placeholder="5.00" onChange={event => update(index, "output", event.target.value)} />
        <input aria-label={zh ? "缓存写" : "Cache write"} inputMode="decimal" value={row.cacheWrite} placeholder={zh ? "可空" : "optional"} onChange={event => update(index, "cacheWrite", event.target.value)} />
        <button type="button" aria-label={zh ? "删除" : "Remove"} title={zh ? "删除" : "Remove"} onClick={() => setRows(current => current.filter((_, i) => i !== index))}>×</button>
      </div>)}
    </div>}
    {(dirty || message) && <div className="custom-prices-actions">
      {dirty && <button type="button" className="usage-text-button" disabled={saving || invalid} onClick={() => void save()}>{saving ? (zh ? "保存中…" : "Saving…") : (zh ? "保存" : "Save")}</button>}
      {dirty && invalid && <small>{zh ? "有一行没填完整：模型、输入、输出必填，价格不能为负。" : "Each row needs a model, input and output; prices cannot be negative."}</small>}
      {message && <small role="status">{message}</small>}
    </div>}
  </section>;
}
