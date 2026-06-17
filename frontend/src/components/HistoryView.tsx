import { createSignal, onMount, For, Show } from "solid-js";
import { api, type Analysis, type DecisionRecord } from "../api";
import ReasoningTrace from "./ReasoningTrace";
import { t } from "../i18n";

const TONE: Record<string, string> = { BUY: "text-emerald-400", SELL: "text-red-400", HOLD: "text-amber-400" };

/** AI analysis history — click to see what it was thinking each time (trace + thinking) */
export default function HistoryView() {
  const [rows, setRows] = createSignal<DecisionRecord[]>([]);
  const [openId, setOpenId] = createSignal<number | null>(null);
  const [analysis, setAnalysis] = createSignal<Analysis | null>(null);

  onMount(async () => setRows(await api.recentDecisions(150)));

  async function open(r: DecisionRecord) {
    setOpenId(r.id);
    setAnalysis(null);
    try {
      setAnalysis(await api.decisionAnalysis(r.id));
    } catch {
      /* ignore */
    }
  }

  return (
    <div class="grid gap-4 lg:grid-cols-[1fr_1.1fr]">
      <div class="overflow-hidden rounded-2xl border border-slate-800">
        <table class="w-full text-left text-sm">
          <thead class="bg-slate-900 text-xs uppercase text-slate-400">
            <tr><th class="px-3 py-2">{t("hist.colTime")}</th><th class="px-3 py-2">{t("hist.colCoin")}</th><th class="px-3 py-2">{t("hist.colVerdict")}</th><th class="px-3 py-2">conf</th><th class="px-3 py-2">engine</th></tr>
          </thead>
          <tbody>
            <For each={rows()}>
              {(r) => (
                <tr
                  class="cursor-pointer border-t border-slate-800 hover:bg-slate-900"
                  classList={{ "bg-slate-900": openId() === r.id }}
                  onClick={() => open(r)}
                >
                  <td class="px-3 py-2 text-slate-500">{new Date(r.created_at).toLocaleString()}</td>
                  <td class="px-3 py-2 font-semibold text-slate-200">{r.symbol}</td>
                  <td class={`px-3 py-2 font-bold ${TONE[r.final_action]}`}>{r.final_action}{r.vetoed ? " 🛑" : ""}</td>
                  <td class="px-3 py-2 text-slate-400">{(r.consensus_confidence * 100).toFixed(0)}%</td>
                  <td class="px-3 py-2 text-slate-600">{r.judge_engine}</td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>

      <div>
        <Show
          when={openId() !== null}
          fallback={<div class="rounded-2xl border border-dashed border-slate-800 p-10 text-center text-slate-600">{t("hist.pickRow")}</div>}
        >
          <Show when={analysis()} fallback={<div class="text-slate-500">{t("common.loading")}</div>}>
            <ReasoningTrace steps={analysis()!.trace} thinking={analysis()!.verdict.thinking} />
          </Show>
        </Show>
      </div>
    </div>
  );
}
