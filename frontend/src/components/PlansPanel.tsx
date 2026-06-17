import { createSignal, onMount, For, Show } from "solid-js";
import { api, type TradePlan } from "../api";
import { t } from "../i18n";

const fmt = (n: number) => (n > 0 ? n.toLocaleString(undefined, { maximumFractionDigits: 4 }) : "—");

/** Trade plans laid out by the AI — shows what it is waiting for, what the target is, and why */
export default function PlansPanel() {
  const [plans, setPlans] = createSignal<TradePlan[]>([]);
  async function load() { setPlans(await api.plans().catch(() => [])); }
  onMount(load);

  return (
    <div class="space-y-3">
      <div class="flex items-center gap-3">
        <h3 class="text-sm font-bold text-slate-200">{t("plan.title")}</h3>
        <button class="ml-auto rounded-lg border border-slate-700 px-3 py-1 text-sm text-slate-300 hover:bg-slate-800" onClick={load}>
          {t("plan.refresh")}
        </button>
      </div>

      <Show when={plans().length} fallback={
        <div class="rounded-2xl border border-dashed border-slate-800 p-10 text-center text-slate-500">{t("plan.none")}</div>
      }>
        <div class="grid gap-3 md:grid-cols-2">
          <For each={plans()}>
            {(p) => {
              const pending = p.state === "pending";
              return (
                <div class="rounded-2xl border border-slate-800 bg-slate-900 p-4">
                  <div class="flex items-center gap-2">
                    <span class="text-lg font-extrabold text-slate-100">{p.symbol}</span>
                    <span class={`rounded-full px-2 py-0.5 text-xs font-semibold ${pending ? "bg-amber-900 text-amber-300" : "bg-emerald-900 text-emerald-300"}`}>
                      {pending ? t("plan.pending") : t("plan.open")}
                    </span>
                    <span class="ml-auto text-xs text-slate-500">conf {(p.confidence * 100).toFixed(0)}%</span>
                  </div>

                  <Show when={pending && p.entry_price > 0}>
                    <div class="mt-2 rounded-lg bg-amber-950/40 px-3 py-1.5 text-sm text-amber-200">
                      {t("plan.waitingBuy")} <b>{fmt(p.entry_price)}</b>
                      <span class="text-amber-400/70"> · {t("plan.now")} {fmt(p.last_price)}</span>
                    </div>
                  </Show>

                  <div class="mt-2 grid grid-cols-3 gap-2 text-center text-sm">
                    <div class="rounded-lg bg-slate-800 p-2">
                      <div class="text-[11px] text-slate-500">{t("plan.entry")}</div>
                      <div class="font-semibold text-slate-200">{fmt(p.entry_price)}</div>
                    </div>
                    <div class="rounded-lg bg-emerald-950/40 p-2">
                      <div class="text-[11px] text-slate-500">{t("plan.target")}</div>
                      <div class="font-semibold text-emerald-400">{fmt(p.target_price)}</div>
                    </div>
                    <div class="rounded-lg bg-red-950/40 p-2">
                      <div class="text-[11px] text-slate-500">{t("plan.stop")}</div>
                      <div class="font-semibold text-red-400">{fmt(p.stop_price)}</div>
                    </div>
                  </div>

                  <Show when={p.thesis}>
                    <div class="mt-2 text-xs text-slate-400"><b class="text-slate-300">{t("plan.thesis")}:</b> {p.thesis}</div>
                  </Show>
                  <Show when={p.next_step}>
                    <div class="mt-1 text-xs text-slate-500">➡️ {t("plan.next")}: {p.next_step}</div>
                  </Show>
                </div>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
}
