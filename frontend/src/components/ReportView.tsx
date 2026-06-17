import { createSignal, onMount, For } from "solid-js";
import { api, type DecisionRecord, type ReportSummary } from "../api";

const TONE: Record<string, string> = {
  BUY: "text-emerald-400",
  SELL: "text-red-400",
  HOLD: "text-amber-400",
};

function Stat(props: { label: string; value: string | number }) {
  return (
    <div class="rounded-xl border border-slate-800 bg-slate-900 p-3">
      <div class="text-xs text-slate-400">{props.label}</div>
      <div class="mt-1 text-xl font-bold text-slate-100">{props.value}</div>
    </div>
  );
}

/** Report page — summary statistics + history table from PostgreSQL (self-fetch) */
export default function ReportView(_props?: { summary?: ReportSummary | null; rows?: DecisionRecord[] }) {
  const [summary, setSummary] = createSignal<ReportSummary | null>(null);
  const [rowsData, setRowsData] = createSignal<DecisionRecord[]>([]);
  onMount(async () => {
    setSummary(await api.report().catch(() => null));
    setRowsData(await api.recentDecisions(100).catch(() => []));
  });
  const s = () => summary();
  return (
    <div class="space-y-4">
      <div class="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Stat label="Total Decisions" value={s()?.total_decisions ?? "—"} />
        <Stat label="Executed Orders" value={s()?.executed ?? "—"} />
        <Stat label="Avg Confidence" value={s() ? `${(s()!.avg_confidence * 100).toFixed(0)}%` : "—"} />
        <Stat label="Symbols Tracked" value={s()?.symbols_tracked ?? "—"} />
        <Stat label="BUY" value={s()?.buy ?? "—"} />
        <Stat label="SELL" value={s()?.sell ?? "—"} />
        <Stat label="HOLD" value={s()?.hold ?? "—"} />
        <Stat label="Vetoed" value={s()?.vetoed ?? "—"} />
      </div>

      <div class="overflow-hidden rounded-2xl border border-slate-800">
        <table class="w-full text-left text-sm">
          <thead class="bg-slate-900 text-xs uppercase text-slate-400">
            <tr>
              <th class="px-3 py-2">Time</th>
              <th class="px-3 py-2">Symbol</th>
              <th class="px-3 py-2">Decision</th>
              <th class="px-3 py-2">judge</th>
              <th class="px-3 py-2">Agreement</th>
              <th class="px-3 py-2">conf</th>
              <th class="px-3 py-2">Traded</th>
              <th class="px-3 py-2">Note</th>
            </tr>
          </thead>
          <tbody>
            <For each={rowsData()}>
              {(r) => (
                <tr class="border-t border-slate-800 bg-slate-950/40">
                  <td class="px-3 py-2 text-slate-400">{new Date(r.created_at).toLocaleString()}</td>
                  <td class="px-3 py-2 font-semibold text-slate-200">{r.symbol}</td>
                  <td class={`px-3 py-2 font-bold ${TONE[r.final_action]}`}>{r.final_action}</td>
                  <td class="px-3 py-2 text-slate-500">{r.judge_engine}</td>
                  <td class="px-3 py-2 text-slate-400">{r.agreement}/{r.voted}{r.vetoed ? " 🛑" : ""}</td>
                  <td class="px-3 py-2 text-slate-400">{(r.consensus_confidence * 100).toFixed(0)}%</td>
                  <td class="px-3 py-2">{r.executed ? "✅" : "—"}</td>
                  <td class="px-3 py-2 max-w-xs truncate text-slate-500" title={r.note}>{r.note}</td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
    </div>
  );
}
