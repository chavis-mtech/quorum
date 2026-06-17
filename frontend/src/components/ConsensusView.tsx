import { Show } from "solid-js";
import type { Analysis } from "../api";
import { t } from "../i18n";

const COLOR: Record<string, string> = {
  BUY: "text-emerald-400",
  SELL: "text-red-400",
  HOLD: "text-amber-400",
};

const REGIME_STYLE: Record<string, { badge: string; label: string; rr: string }> = {
  trending:    { badge: "bg-emerald-950/60 text-emerald-300 border-emerald-800", label: "Trending",   rr: "RR ≥ 1.5" },
  "weak-trend":{ badge: "bg-sky-950/60 text-sky-300 border-sky-800",            label: "Weak Trend", rr: "RR ≥ 2.0" },
  ranging:     { badge: "bg-amber-950/60 text-amber-300 border-amber-800",       label: "Ranging",    rr: "RR ≥ 2.5" },
  unknown:     { badge: "bg-slate-800 text-slate-400 border-slate-700",          label: "Unknown",    rr: "" },
};

export default function ConsensusView(props: { a: Analysis }) {
  const c = () => props.a.consensus;
  const v = () => props.a.verdict;

  const abstained = () => (c().votes ?? []).filter((vt) => !vt.ok).length;
  const regime = () => (props.a.regime ?? c().regime ?? "unknown").toLowerCase();
  const regimeStyle = () => REGIME_STYLE[regime()] ?? REGIME_STYLE["unknown"];

  // Calculate RR from entry/target/stop
  const rr = () => {
    const vd = v();
    if (vd.entry_price > 0 && vd.target_price > 0 && vd.stop_price > 0) {
      const reward = Math.abs(vd.target_price - vd.entry_price);
      const risk = Math.abs(vd.entry_price - vd.stop_price);
      return risk > 0 ? reward / risk : 0;
    }
    return 0;
  };

  return (
    <div class="rounded-2xl border border-slate-800 bg-slate-900 p-5">
      {/* regime + RR badges */}
      <div class="mb-3 flex flex-wrap items-center gap-2">
        <span class={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-semibold ${regimeStyle().badge}`}>
          <span class="h-1.5 w-1.5 rounded-full bg-current opacity-80" />
          {regimeStyle().label}
        </span>
        <Show when={regimeStyle().rr}>
          <span class="rounded-full border border-slate-700 bg-slate-800 px-2.5 py-0.5 text-xs text-slate-400">
            {regimeStyle().rr}
          </span>
        </Show>
        <Show when={rr() > 0}>
          <span class={`rounded-full border px-2.5 py-0.5 text-xs font-semibold ${
            rr() >= 2.5 ? "border-emerald-800 bg-emerald-950/60 text-emerald-300"
            : rr() >= 1.5 ? "border-sky-800 bg-sky-950/60 text-sky-300"
            : "border-red-900 bg-red-950/60 text-red-400"
          }`}>
            RR {rr().toFixed(2)}
          </span>
        </Show>
      </div>

      {/* Consensus → Verdict */}
      <div class="flex items-center gap-6">
        <div>
          <div class="text-xs text-slate-400">{t("cons.consensus")}</div>
          <div class={`text-3xl font-extrabold ${COLOR[c().action]}`}>{c().action}</div>
          <div class="text-xs text-slate-400">
            {c().agreement}/{c().voted} voted
            <Show when={abstained() > 0}>
              <span class="ml-1 text-slate-600">· {abstained()} abstained</span>
            </Show>
            {" "}· conf {(c().confidence * 100).toFixed(0)}%
            {c().vetoed ? " · 🛑 VETO" : c().passed_threshold ? " · " + t("cons.passed") : " · " + t("cons.notPassed")}
          </div>
        </div>
        <div class="text-2xl text-slate-600">→</div>
        <div>
          <div class="text-xs text-slate-400">{t("cons.verdict")}</div>
          <div class={`text-3xl font-extrabold ${COLOR[v().action]}`}>{v().action}</div>
          <div class="text-xs text-slate-400">
            conf {(v().confidence * 100).toFixed(0)}% · [{v().engine}]
          </div>
        </div>
        <Show when={v().suggested_size_pct > 0}>
          <div class="ml-auto text-right">
            <div class="text-xs text-slate-400">Suggested size</div>
            <div class="text-xl font-bold text-sky-400">{(v().suggested_size_pct * 100).toFixed(0)}%</div>
            <div class="text-xs text-slate-600">of portfolio</div>
          </div>
        </Show>
      </div>

      {/* reasoning */}
      <p class="mt-4 whitespace-pre-line text-sm leading-relaxed text-slate-300">{v().reasoning}</p>

      {/* Trade plan */}
      <Show when={v().thesis || v().target_price > 0 || v().entry_price > 0}>
        <div class="mt-3 rounded-xl border border-slate-800 bg-slate-950/50 p-3 space-y-2">
          <Show when={v().thesis}>
            <div class="text-xs text-slate-400">
              <b class="text-slate-300">{t("plan.thesis")}:</b> {v().thesis}
            </div>
          </Show>
          <div class="grid grid-cols-3 gap-2 text-center text-sm">
            <div class="rounded-lg bg-slate-800 p-2">
              <div class="text-[11px] text-slate-500">{t("plan.entry")} ({v().entry_type})</div>
              <div class="font-semibold text-slate-200">{num(v().entry_price)}</div>
            </div>
            <div class="rounded-lg bg-emerald-950/40 p-2">
              <div class="text-[11px] text-slate-500">{t("plan.target")}</div>
              <div class="font-semibold text-emerald-400">{num(v().target_price)}</div>
            </div>
            <div class="rounded-lg bg-red-950/40 p-2">
              <div class="text-[11px] text-slate-500">{t("plan.stop")}</div>
              <div class="font-semibold text-red-400">{num(v().stop_price)}</div>
            </div>
          </div>
          <Show when={v().invalidation}>
            <div class="rounded-lg border border-amber-900/40 bg-amber-950/20 px-3 py-2 text-xs">
              <span class="font-semibold text-amber-400">⚠ Invalidation: </span>
              <span class="text-slate-400">{v().invalidation}</span>
            </div>
          </Show>
          <Show when={v().next_step}>
            <div class="text-xs text-slate-500">➡️ {t("plan.next")}: {v().next_step}</div>
          </Show>
        </div>
      </Show>

      {/* Show invalidation even without a plan (e.g. HOLD) */}
      <Show when={v().invalidation && !(v().thesis || v().target_price > 0 || v().entry_price > 0)}>
        <div class="mt-3 rounded-lg border border-amber-900/40 bg-amber-950/20 px-3 py-2 text-xs">
          <span class="font-semibold text-amber-400">⚠ Invalidation: </span>
          <span class="text-slate-400">{v().invalidation}</span>
        </div>
      </Show>
    </div>
  );
}

const num = (n: number) => (n > 0 ? n.toLocaleString(undefined, { maximumFractionDigits: 4 }) : "—");
