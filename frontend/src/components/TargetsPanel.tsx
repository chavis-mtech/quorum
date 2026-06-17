import { createSignal, onMount, onCleanup, For, Show } from "solid-js";
import { api, type TargetStatus } from "../api";
import { t } from "../i18n";

const fmt = (n: number) => (n > 0 ? n.toLocaleString(undefined, { maximumFractionDigits: 6 }) : "—");

const CHIP: Record<string, { cls: string; label: string }> = {
  holding: { cls: "bg-emerald-900 text-emerald-300", label: "📈 Holding" },
  plan_pending: { cls: "bg-amber-900 text-amber-300", label: "⏳ Pending entry" },
  candidate: { cls: "bg-sky-900 text-sky-300", label: "⭐ Buy candidate" },
  waiting: { cls: "bg-slate-700 text-slate-300", label: "🔸 Watching" },
  skipped: { cls: "bg-red-950 text-red-300", label: "🚫 Skipped" },
  queued: { cls: "bg-slate-800 text-slate-400", label: "🕓 Queued" },
};

// Display priority order (most actionable at the top)
const ORDER = ["holding", "plan_pending", "candidate", "waiting", "queued", "skipped"];

/** Targets the AI is currently tracking + reason why it hasn't entered yet — fixes the "waiting but nothing seems to happen" problem */
export default function TargetsPanel(props: { onAnalyze: (s: string) => void }) {
  const [items, setItems] = createSignal<TargetStatus[]>([]);
  const [loading, setLoading] = createSignal(false);

  async function load() {
    setLoading(true);
    try {
      const rows = await api.targets().catch(() => []);
      rows.sort((a, b) => ORDER.indexOf(a.state) - ORDER.indexOf(b.state) || b.confidence - a.confidence);
      setItems(rows);
    } finally {
      setLoading(false);
    }
  }
  onMount(() => {
    load();
    const iv = setInterval(load, 15000);
    onCleanup(() => clearInterval(iv));
  });

  return (
    <div class="space-y-3">
      <div class="flex items-center gap-3">
        <h3 class="text-sm font-bold text-slate-200">{t("target.title")}</h3>
        <span class="text-xs text-slate-500">{t("target.subtitle")}</span>
        <button class="ml-auto rounded-lg border border-slate-700 px-3 py-1 text-sm text-slate-300 hover:bg-slate-800" onClick={load}>
          {loading() ? t("common.loading") : t("common.refresh")}
        </button>
      </div>

      <Show
        when={items().length}
        fallback={<div class="rounded-2xl border border-dashed border-slate-800 p-10 text-center text-slate-500">{t("target.none")}</div>}
      >
        <div class="grid gap-3 md:grid-cols-2">
          <For each={items()}>
            {(it) => {
              const chip = CHIP[it.state] ?? CHIP.waiting;
              const hasLevels = it.entry_price > 0 || it.target_price > 0 || it.stop_price > 0;
              return (
                <div class="rounded-2xl border border-slate-800 bg-slate-900 p-4">
                  <div class="flex items-center gap-2">
                    <span class="text-lg font-extrabold text-slate-100">{it.symbol}</span>
                    <span class={`rounded-full px-2 py-0.5 text-xs font-semibold ${chip.cls}`}>{chip.label}</span>
                    <Show when={it.confidence > 0}>
                      <span class="text-xs text-slate-500">conf {(it.confidence * 100).toFixed(0)}%</span>
                    </Show>
                    <span class="ml-auto text-xs text-slate-500">{fmt(it.last_price)}</span>
                  </div>

                  <div class="mt-2 text-sm text-slate-300">{it.reason}</div>

                  <Show when={hasLevels}>
                    <div class="mt-3 grid grid-cols-3 gap-2 text-center text-sm">
                      <div class="rounded-lg bg-slate-800 p-2">
                        <div class="text-[11px] text-slate-500">{t("plan.entry")}</div>
                        <div class="font-semibold text-slate-200">{fmt(it.entry_price)}</div>
                      </div>
                      <div class="rounded-lg bg-emerald-950/40 p-2">
                        <div class="text-[11px] text-slate-500">{t("plan.target")}</div>
                        <div class="font-semibold text-emerald-400">{fmt(it.target_price)}</div>
                      </div>
                      <div class="rounded-lg bg-red-950/40 p-2">
                        <div class="text-[11px] text-slate-500">{t("plan.stop")}</div>
                        <div class="font-semibold text-red-400">{fmt(it.stop_price)}</div>
                      </div>
                    </div>
                  </Show>

                  <div class="mt-3 flex justify-end">
                    <button
                      class="rounded-lg border border-sky-700 bg-sky-950/40 px-3 py-1 text-xs font-semibold text-sky-300 hover:bg-sky-900/50"
                      onClick={() => props.onAnalyze(it.symbol)}
                    >
                      {t("live.analyzeNow")}
                    </button>
                  </div>
                </div>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
}
