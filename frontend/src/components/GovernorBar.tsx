import { Show } from "solid-js";
import type { GovernorState } from "../api";
import { t } from "../i18n";

const fmt = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 0 });

const TONE: Record<string, { bg: string; dot: string; text: string }> = {
  halted: { bg: "bg-red-950/70 border-red-800", dot: "bg-red-500", text: "text-red-300" },
  paused: { bg: "bg-amber-950/60 border-amber-800", dot: "bg-amber-400", text: "text-amber-300" },
  scanning: { bg: "bg-sky-950/50 border-sky-900", dot: "bg-sky-400 animate-pulse", text: "text-sky-300" },
  trading: { bg: "bg-emerald-950/50 border-emerald-900", dot: "bg-emerald-400 animate-pulse", text: "text-emerald-300" },
  full: { bg: "bg-slate-900 border-slate-700", dot: "bg-indigo-400", text: "text-indigo-300" },
  manual: { bg: "bg-slate-900 border-slate-700", dot: "bg-slate-500", text: "text-slate-300" },
  signal: { bg: "bg-slate-900 border-slate-700", dot: "bg-slate-500", text: "text-slate-300" },
};

/** Governor status bar — clearly shows "what is happening right now and why" + kill-switch */
export default function GovernorBar(props: {
  gov: GovernorState | null;
  onTogglePause: () => void;
  busy?: boolean;
}) {
  const g = () => props.gov;
  const tone = () => TONE[g()?.state ?? "manual"] ?? TONE.manual;
  const lossPct = () => Math.round((g()?.loss_used ?? 0) * 100);

  return (
    <Show when={g()}>
      <div class={`mx-auto mt-3 max-w-6xl rounded-xl border px-4 py-2.5 ${tone().bg}`}>
        <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
          <div class="flex items-center gap-2">
            <span class={`h-2.5 w-2.5 rounded-full ${tone().dot}`} />
            <span class={`text-sm font-bold uppercase ${tone().text}`}>{t(`gov.${g()!.state}`)}</span>
          </div>
          <span class="text-sm text-slate-300">{g()!.reason}</span>

          <div class="ml-auto flex flex-wrap items-center gap-x-4 gap-y-2 text-xs text-slate-400">
            <span title={t("gov.buysHint")}>
              🛒 {t("gov.buysLeft")}: <b class="text-slate-200">{g()!.buys_remaining}</b>
            </span>
            <span title={t("gov.slotsHint")}>
              📊 {g()!.open_positions}/{g()!.max_open_positions}
            </span>
            <span>💰 {fmt(g()!.equity)}</span>
            <button
              class={`rounded-lg border px-3 py-1 font-semibold ${
                g()!.paused
                  ? "border-emerald-700 bg-emerald-900/40 text-emerald-300 hover:bg-emerald-900"
                  : "border-red-800 bg-red-950/40 text-red-300 hover:bg-red-900/50"
              } disabled:opacity-50`}
              disabled={props.busy}
              onClick={props.onTogglePause}
            >
              {g()!.paused ? t("gov.resume") : t("gov.kill")}
            </button>
          </div>
        </div>

        {/* Daily loss quota bar */}
        <Show when={g()!.loss_limit > 0}>
          <div class="mt-2 flex items-center gap-2">
            <span class="text-[11px] text-slate-500">{t("gov.lossBudget")}</span>
            <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-slate-800">
              <div
                class={`h-full rounded-full ${lossPct() >= 100 ? "bg-red-500" : lossPct() >= 60 ? "bg-amber-500" : "bg-emerald-500"}`}
                style={{ width: `${Math.min(100, lossPct())}%` }}
              />
            </div>
            <span class="text-[11px] text-slate-400">{lossPct()}%</span>
          </div>
        </Show>
      </div>
    </Show>
  );
}
