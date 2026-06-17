/**
 * TrackingPanel — combines "trade plans" + "targets" in one place
 *
 * /api/targets  → every symbol in watchlist + state/reason + price
 * /api/plans    → plans released by AI (with thesis/invalidation/next_step)
 *
 * merged by symbol: targets are the backbone, plans add detail on top
 */
import { createSignal, onMount, onCleanup, For, Show, createMemo } from "solid-js";
import { api, type TradePlan, type TargetStatus } from "../api";
import { t } from "../i18n";

// ---------- formatters ----------
const fmtPrice = (n: number, decimals = 4) =>
  n > 0 ? n.toLocaleString(undefined, { maximumFractionDigits: decimals }) : "—";

function fmtDist(from: number, to: number): string {
  if (from <= 0 || to <= 0) return "";
  const pct = ((to - from) / from) * 100;
  return (pct >= 0 ? "+" : "") + pct.toFixed(1) + "%";
}

function fmtAge(iso: string | null): string {
  if (!iso) return "";
  const diff = (Date.now() - new Date(iso).getTime()) / 1000;
  if (diff < 60) return `${Math.floor(diff)}s`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

function calcRR(entry: number, target: number, stop: number): string {
  const upside = target - entry;
  const downside = entry - stop;
  if (downside <= 0 || upside <= 0) return "";
  return (upside / downside).toFixed(1) + ":1";
}

// ---------- chips ----------
const STATE_CHIP: Record<string, { cls: string; label: string; ring: string }> = {
  holding:      { cls: "bg-emerald-900 text-emerald-300", ring: "ring-emerald-800", label: "📈 Holding" },
  plan_pending: { cls: "bg-amber-900 text-amber-300",    ring: "ring-amber-800",   label: "⏳ Pending entry" },
  candidate:    { cls: "bg-sky-900 text-sky-300",        ring: "ring-sky-800",     label: "⭐ Candidate" },
  waiting:      { cls: "bg-slate-700 text-slate-300",    ring: "ring-slate-700",   label: "🔸 Watching" },
  skipped:      { cls: "bg-red-950 text-red-400",        ring: "ring-red-900",     label: "🚫 Skipped" },
  queued:       { cls: "bg-slate-800 text-slate-400",    ring: "ring-slate-700",   label: "🕓 Queued" },
};
const ORDER = ["holding", "plan_pending", "candidate", "waiting", "queued", "skipped"];

// ---------- merged row ----------
interface Row extends TargetStatus {
  plan?: TradePlan;
}

// ---------- PriceGrid ----------
function PriceGrid(props: {
  entry: number; target: number; stop: number;
  last: number; entryType?: string;
}) {
  const rr = () => calcRR(props.entry, props.target, props.stop);
  const dEntry  = () => fmtDist(props.last,  props.entry);
  const dTarget = () => fmtDist(props.entry, props.target);
  const dStop   = () => fmtDist(props.entry, props.stop);

  return (
    <div class="mt-3 space-y-1">
      <div class="grid grid-cols-3 gap-1.5 text-center">
        <div class="rounded-lg bg-slate-800 px-2 py-2">
          <div class="text-[10px] text-slate-500 sm:text-[11px]">
            {t("plan.entry")}{props.entryType ? ` (${props.entryType})` : ""}
          </div>
          <div class="mt-0.5 text-sm font-semibold text-slate-200 sm:text-base">
            {fmtPrice(props.entry)}
          </div>
          <Show when={dEntry()}>
            <div class={`text-[10px] ${props.entry < props.last ? "text-emerald-500" : "text-amber-500"}`}>
              {dEntry()} from market
            </div>
          </Show>
        </div>
        <div class="rounded-lg bg-emerald-950/40 px-2 py-2">
          <div class="text-[10px] text-slate-500 sm:text-[11px]">{t("plan.target")}</div>
          <div class="mt-0.5 text-sm font-semibold text-emerald-400 sm:text-base">
            {fmtPrice(props.target)}
          </div>
          <Show when={dTarget()}>
            <div class="text-[10px] text-emerald-600">{dTarget()} profit</div>
          </Show>
        </div>
        <div class="rounded-lg bg-red-950/40 px-2 py-2">
          <div class="text-[10px] text-slate-500 sm:text-[11px]">{t("plan.stop")}</div>
          <div class="mt-0.5 text-sm font-semibold text-red-400 sm:text-base">
            {fmtPrice(props.stop)}
          </div>
          <Show when={dStop()}>
            <div class="text-[10px] text-red-600">{dStop()} risk</div>
          </Show>
        </div>
      </div>
      <Show when={rr()}>
        <div class="flex items-center justify-end gap-2 text-[11px]">
          <span class="text-slate-600">reward:risk</span>
          <span class={`font-semibold ${parseFloat(rr()) >= 1.5 ? "text-emerald-400" : parseFloat(rr()) >= 1.0 ? "text-amber-400" : "text-red-400"}`}>
            {rr()}
          </span>
        </div>
      </Show>
    </div>
  );
}

// ---------- main component ----------
export default function TrackingPanel(props: { onAnalyze: (s: string) => void }) {
  const [targets, setTargets] = createSignal<TargetStatus[]>([]);
  const [plans, setPlans]     = createSignal<TradePlan[]>([]);
  const [loading, setLoading] = createSignal(false);

  async function load() {
    setLoading(true);
    try {
      const [t2, p2] = await Promise.all([
        api.targets().catch(() => [] as TargetStatus[]),
        api.plans().catch(() => [] as TradePlan[]),
      ]);
      setTargets(t2);
      setPlans(p2);
    } finally {
      setLoading(false);
    }
  }

  onMount(() => {
    load();
    const iv = setInterval(load, 15_000);
    onCleanup(() => clearInterval(iv));
  });

  // merge: build plan map by symbol (latest plan wins)
  const rows = createMemo<Row[]>(() => {
    const planMap = new Map<string, TradePlan>();
    for (const p of plans()) {
      const existing = planMap.get(p.symbol);
      if (!existing || new Date(p.updated_at) > new Date(existing.updated_at)) {
        planMap.set(p.symbol, p);
      }
    }
    const merged: Row[] = targets().map((tgt) => ({
      ...tgt,
      plan: planMap.get(tgt.symbol),
    }));
    merged.sort(
      (a, b) =>
        ORDER.indexOf(a.state) - ORDER.indexOf(b.state) ||
        b.confidence - a.confidence
    );
    return merged;
  });

  // stats bar
  const stats = createMemo(() => {
    const rs = rows();
    return {
      holding:  rs.filter((r) => r.state === "holding").length,
      pending:  rs.filter((r) => r.state === "plan_pending").length,
      candidate: rs.filter((r) => r.state === "candidate").length,
      total:    rs.length,
    };
  });

  return (
    <div class="space-y-4">
      {/* header */}
      <div class="flex flex-wrap items-center gap-3">
        <h3 class="text-sm font-bold text-slate-200">{t("tracking.title")}</h3>
        <div class="flex flex-wrap gap-2 text-xs">
          <Show when={stats().holding > 0}>
            <span class="rounded-full bg-emerald-900/60 px-2 py-0.5 text-emerald-300">
              📈 {stats().holding} holding
            </span>
          </Show>
          <Show when={stats().pending > 0}>
            <span class="rounded-full bg-amber-900/60 px-2 py-0.5 text-amber-300">
              ⏳ {stats().pending} pending
            </span>
          </Show>
          <Show when={stats().candidate > 0}>
            <span class="rounded-full bg-sky-900/60 px-2 py-0.5 text-sky-300">
              ⭐ {stats().candidate} candidate
            </span>
          </Show>
        </div>
        <button
          class="ml-auto rounded-lg border border-slate-700 px-3 py-1.5 text-sm text-slate-300 hover:bg-slate-800 active:bg-slate-700"
          onClick={load}
          disabled={loading()}
        >
          {loading() ? "…" : t("common.refresh")}
        </button>
      </div>

      <Show
        when={rows().length}
        fallback={
          <div class="rounded-2xl border border-dashed border-slate-800 p-10 text-center text-slate-500">
            {t("target.none")}
          </div>
        }
      >
        <div class="grid gap-3 sm:grid-cols-1 md:grid-cols-2">
          <For each={rows()}>
            {(row) => {
              const chip  = STATE_CHIP[row.state] ?? STATE_CHIP.waiting;
              const plan  = row.plan;

              // entry/target/stop prices: plan is more precise than target when available
              const entry  = plan?.entry_price  || row.entry_price;
              const target = plan?.target_price || row.target_price;
              const stop   = plan?.stop_price   || row.stop_price;
              const hasLevels = entry > 0 || target > 0 || stop > 0;

              // thesis/invalidation/next_step come from plan only
              const thesis      = plan?.thesis;
              const invalidation = plan?.invalidation;
              const nextStep    = plan?.next_step;
              const entryType   = plan?.entry_type;

              const age = fmtAge(row.updated_at ?? plan?.updated_at ?? null);

              // active position management indicators (trailing stop / breakeven)
              const trailing = !!plan?.trail_active && row.state === "holding";
              const riskFree = trailing && (plan?.stop_price ?? 0) >= (plan?.entry_price ?? 0);
              const highWater = plan?.high_water_mark ?? 0;

              return (
                <div
                  class={`rounded-2xl border bg-slate-900 p-4 ring-1 ${chip.ring} border-slate-800`}
                >
                  {/* row 1: symbol + badge + conf + price + age */}
                  <div class="flex flex-wrap items-center gap-2">
                    <span class="text-lg font-extrabold text-slate-100">{row.symbol}</span>
                    <span class={`rounded-full px-2 py-0.5 text-xs font-semibold ${chip.cls}`}>
                      {chip.label}
                    </span>
                    <Show when={row.confidence > 0}>
                      <span class="text-xs text-slate-500">
                        conf {(row.confidence * 100).toFixed(0)}%
                      </span>
                    </Show>
                    <Show when={trailing}>
                      <span class="rounded-full bg-emerald-800/70 px-2 py-0.5 text-[10px] font-semibold text-emerald-200">
                        🛡️ {riskFree ? t("plan.riskFree") : t("plan.trailing")}
                      </span>
                    </Show>
                    <span class="ml-auto flex items-baseline gap-1.5 text-sm font-semibold text-slate-200">
                      {fmtPrice(row.last_price, 6)}
                    </span>
                    <Show when={age}>
                      <span class="text-[10px] text-slate-600">{age}</span>
                    </Show>
                  </div>

                  {/* reason — why this state */}
                  <p class="mt-2 text-xs leading-relaxed text-slate-400">{row.reason}</p>

                  {/* price levels */}
                  <Show when={hasLevels}>
                    <PriceGrid
                      entry={entry} target={target} stop={stop}
                      last={row.last_price} entryType={entryType}
                    />
                  </Show>

                  {/* active management: trailing stop status */}
                  <Show when={trailing}>
                    <div class="mt-1.5 flex items-center justify-end gap-2 text-[10px] text-emerald-500">
                      <span>🛡️ {t("plan.trailingStop")}</span>
                      <Show when={highWater > 0}>
                        <span class="text-slate-600">· {t("plan.peak")} {fmtPrice(highWater, 6)}</span>
                      </Show>
                    </div>
                  </Show>

                  {/* thesis (plan) */}
                  <Show when={thesis}>
                    <div class="mt-3 rounded-lg border border-slate-700 bg-slate-950/50 px-3 py-2 text-xs leading-relaxed">
                      <span class="font-semibold text-slate-300">💡 {t("plan.thesis")}: </span>
                      <span class="text-slate-400">{thesis}</span>
                    </div>
                  </Show>

                  {/* invalidation */}
                  <Show when={invalidation}>
                    <div class="mt-2 rounded-lg border border-amber-900/40 bg-amber-950/20 px-3 py-2 text-xs leading-relaxed">
                      <span class="font-semibold text-amber-400">⚠️ Invalidation: </span>
                      <span class="text-slate-400">{invalidation}</span>
                    </div>
                  </Show>

                  {/* next_step */}
                  <Show when={nextStep}>
                    <div class="mt-2 text-xs text-slate-500">➡️ {t("plan.next")}: {nextStep}</div>
                  </Show>

                  {/* footer: analyze button + plan meta */}
                  <div class="mt-3 flex flex-wrap items-center gap-2 border-t border-slate-800 pt-2">
                    <Show when={plan}>
                      <span class="text-[10px] text-slate-600">
                        plan #{plan!.id} · {plan!.entry_type}
                      </span>
                    </Show>
                    <button
                      class="ml-auto min-h-[36px] rounded-lg border border-sky-700 bg-sky-950/40 px-3 py-1.5 text-xs font-semibold text-sky-300 hover:bg-sky-900/50 active:bg-sky-900"
                      onClick={() => props.onAnalyze(row.symbol)}
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
