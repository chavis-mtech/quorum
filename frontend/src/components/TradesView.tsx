import { createSignal, onMount, For, Show } from "solid-js";
import { api, type Analysis, type TradeRecord } from "../api";
import ReasoningTrace from "./ReasoningTrace";
import { createSortable } from "./sortable";
import { t } from "../i18n";

const fmt = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 4 });
const TONE: Record<string, string> = { BUY: "text-emerald-400", SELL: "text-red-400", HOLD: "text-amber-400" };

/** Trade history — sortable columns + click to view the underlying analysis */
export default function TradesView() {
  const [trades, setTrades] = createSignal<TradeRecord[]>([]);
  const [analysis, setAnalysis] = createSignal<Analysis | null>(null);
  const [openId, setOpenId] = createSignal<number | null>(null);
  const s = createSortable<TradeRecord>(trades, "created_at");

  async function load() { setTrades(await api.recentTrades(200)); }
  onMount(load);

  async function showAnalysis(tr: TradeRecord) {
    if (!tr.decision_id) return;
    setOpenId(tr.id);
    setAnalysis(null);
    try { setAnalysis(await api.decisionAnalysis(tr.decision_id)); } catch {}
  }
  async function clearAll() {
    if (!confirm(t("trades.clear") + " ?")) return;
    await api.clearTrades();
    await load();
  }

  const Th = (p: { k: keyof TradeRecord; label: string; right?: boolean }) => (
    <th class="cursor-pointer select-none px-3 py-2 hover:text-slate-200" classList={{ "text-right": p.right }} onClick={() => s.toggle(p.k)}>
      {p.label}{s.arrow(p.k)}
    </th>
  );

  return (
    <div class="space-y-4">
      <div class="flex items-center gap-3">
        <h3 class="text-sm font-bold text-slate-200">{t("trades.title")}</h3>
        <button class="ml-auto rounded-lg border border-slate-700 px-3 py-1.5 text-sm text-slate-300 hover:bg-slate-800" onClick={clearAll}>
          {t("trades.clear")}
        </button>
      </div>

      <div class="overflow-hidden rounded-2xl border border-slate-800">
        <table class="w-full text-left text-sm">
          <thead class="bg-slate-900 text-xs uppercase text-slate-400">
            <tr>
              <Th k="created_at" label={t("trades.colTime")} />
              <Th k="symbol" label={t("trades.colCoin")} />
              <Th k="side" label={t("trades.colSide")} />
              <Th k="mode" label={t("trades.colMode")} />
              <Th k="amount_base" label={t("trades.colAmount")} right />
              <Th k="price" label={t("trades.colPrice")} right />
              <Th k="realized_pnl" label={t("trades.colPnl")} right />
              <Th k="status" label={t("trades.colStatus")} />
              <th class="px-3 py-2"></th>
            </tr>
          </thead>
          <tbody>
            <For each={s.sorted()} fallback={<tr><td colspan="9" class="px-3 py-6 text-center text-slate-600">{t("trades.none")}</td></tr>}>
              {(tr) => (
                <tr class="border-t border-slate-800">
                  <td class="px-3 py-2 text-slate-500">{new Date(tr.created_at).toLocaleString()}</td>
                  <td class="px-3 py-2 font-semibold text-slate-200">{tr.symbol}</td>
                  <td class={`px-3 py-2 font-bold ${TONE[tr.side]}`}>{tr.side}</td>
                  <td class="px-3 py-2 text-slate-400">{tr.mode}{tr.simulated ? " *" : ""}</td>
                  <td class="px-3 py-2 text-right text-slate-400">{fmt(tr.amount_base)}</td>
                  <td class="px-3 py-2 text-right text-slate-400">{fmt(tr.price)}</td>
                  <td class="px-3 py-2 text-right font-medium" classList={{
                    "text-emerald-400": tr.realized_pnl > 0, "text-red-400": tr.realized_pnl < 0, "text-slate-600": tr.realized_pnl === 0,
                  }}>
                    {tr.realized_pnl !== 0 ? `${tr.realized_pnl > 0 ? "+" : ""}${fmt(tr.realized_pnl)}` : "—"}
                  </td>
                  <td class="px-3 py-2">
                    <span classList={{ "text-emerald-400": tr.status === "filled", "text-red-400": tr.status !== "filled" }}>
                      {tr.status === "filled" ? t("trades.ok") : t("trades.fail")}
                    </span>
                    <Show when={tr.status !== "filled" && tr.note}>
                      <div class="mt-0.5 max-w-[20rem] whitespace-normal break-words text-xs leading-snug text-amber-400/80" title={tr.note}>
                        {tr.note}
                      </div>
                    </Show>
                  </td>
                  <td class="px-3 py-2">
                    <Show when={tr.decision_id}>
                      <button class="rounded border border-slate-600 px-2 py-0.5 text-xs text-sky-300 hover:bg-slate-800" onClick={() => showAnalysis(tr)}>{t("trades.viewThink")}</button>
                    </Show>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>

      <Show when={openId() !== null}>
        <div class="rounded-2xl border border-sky-900 bg-slate-950 p-4">
          <div class="mb-2 flex items-center justify-between">
            <span class="text-sm font-semibold text-sky-300">{t("trades.behind")} #{openId()}</span>
            <button class="text-slate-500 hover:text-slate-300" onClick={() => setOpenId(null)}>{t("common.close")} ✕</button>
          </div>
          <Show when={analysis()} fallback={<div class="text-slate-500">{t("common.loading")}</div>}>
            <ReasoningTrace steps={analysis()!.trace} thinking={analysis()!.verdict.thinking} />
          </Show>
        </div>
      </Show>
    </div>
  );
}
