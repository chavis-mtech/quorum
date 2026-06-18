import { For, Show } from "solid-js";
import type { Vote } from "../api";
import { t } from "../i18n";

const TONE: Record<string, string> = {
  BUY: "bg-emerald-500 text-emerald-950",
  SELL: "bg-red-500 text-red-950",
  HOLD: "bg-amber-400 text-amber-950",
};
const BAR: Record<string, string> = {
  BUY: "bg-emerald-500",
  SELL: "bg-red-500",
  HOLD: "bg-amber-400",
};

export default function AgentVotesPanel(props: { votes: Vote[] }) {
  const active = () => props.votes.filter((v) => v.ok);
  const abstained = () => props.votes.filter((v) => !v.ok);

  return (
    <div class="rounded-2xl border border-slate-800 bg-slate-900 p-4">
      <div class="mb-3 flex items-center justify-between">
        <h3 class="text-xs font-semibold uppercase tracking-wide text-slate-400">
          {t("cons.title")}
        </h3>
        <Show when={abstained().length > 0}>
          <span class="text-xs text-slate-600">
            {active().length} voted · {abstained().length} abstained
          </span>
        </Show>
      </div>

      {/* agents that voted */}
      <div class="space-y-1">
        <For each={active()}>
          {(v) => (
            <div class="border-t border-slate-800 py-2">
              <div class="flex items-center gap-3">
                <span class={`shrink-0 rounded px-2 py-0.5 text-center text-xs font-bold ${TONE[v.action]}`}>
                  {v.veto ? "🛑 " : ""}{v.action}
                </span>
                <div class="min-w-0 flex-1">
                  <div class="flex items-center justify-between">
                    <span class="text-sm font-semibold text-slate-200">{v.agent}</span>
                    <span class="ml-2 shrink-0 text-xs text-slate-400">{(v.confidence * 100).toFixed(0)}%</span>
                  </div>
                  <div class="mt-1 h-1 w-full rounded bg-slate-800">
                    <div class={`h-1 rounded ${BAR[v.action]}`} style={{ width: `${v.confidence * 100}%` }} />
                  </div>
                </div>
              </div>
              <p class="mt-1.5 whitespace-pre-line text-xs leading-relaxed text-slate-400">{v.reasoning}</p>
            </div>
          )}
        </For>
      </div>

      {/* agents that abstained (fallback) */}
      <Show when={abstained().length > 0}>
        <div class="mt-3 border-t border-slate-800 pt-2">
          <div class="mb-1 text-xs text-slate-600">{t("cons.abstainedNoData")}</div>
          <For each={abstained()}>
            {(v) => (
              <div class="py-1 opacity-40">
                <div class="flex items-center gap-2">
                  <span class="shrink-0 rounded border border-slate-700 px-2 py-0.5 text-xs text-slate-500">–</span>
                  <span class="text-sm text-slate-500">{v.agent}</span>
                </div>
                <p class="mt-0.5 text-xs leading-relaxed text-slate-600">{v.reasoning}</p>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}
