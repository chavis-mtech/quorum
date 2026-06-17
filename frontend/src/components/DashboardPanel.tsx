import { createSignal, onCleanup, onMount, Show, For } from "solid-js";
import { api, type AlertRecord, type TradeStats, type WalletView } from "../api";
import { t } from "../i18n";

const fmt = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 2 });

/** Dashboard win/loss rate (simulation mode) + reset session to start fresh count */
export default function DashboardPanel() {
  const [stats, setStats] = createSignal<TradeStats | null>(null);
  const [wallet, setWallet] = createSignal<WalletView | null>(null);
  const [busy, setBusy] = createSignal(false);

  async function load() {
    // load wallet first (live: trigger reconcile + backfill realized) then read the updated stats
    setWallet(await api.getWallet().catch(() => null));
    setStats(await api.stats().catch(() => null));
  }
  onMount(() => {
    load();
    // periodic refresh so numbers update with background trading without manually refreshing the page
    const timer = setInterval(load, 15000);
    onCleanup(() => clearInterval(timer));
  });

  async function resetSession() {
    if (!confirm(t("dash.resetKeep") + " ?")) return;
    setBusy(true);
    try { await api.resetStats(); await load(); } finally { setBusy(false); }
  }
  async function resetWallet() {
    if (!confirm(t("dash.resetAll") + " ?")) return;
    setBusy(true);
    try { await api.resetWallet(100000); await load(); } finally { setBusy(false); }
  }

  return (
    <Show when={stats()} fallback={<div class="text-slate-500">Loading...</div>}>
      {(() => {
        const s = stats()!;
        const wr = s.win_rate * 100;
        const positions = () => wallet()?.positions ?? [];
        const posUnreal = (p: { last_price: number; avg_price: number; amount_base: number }) =>
          (p.last_price - p.avg_price) * p.amount_base;
        const unrealized = wallet()
          ? positions().reduce((sum, p) => sum + posUnreal(p), 0)
          : 0;
        return (
          <div class="space-y-5">
            <div class="flex flex-wrap items-center gap-3">
              <h3 class="text-sm font-bold text-slate-200">{t("dash.title")}</h3>
              {/* live/paper badge — makes it unambiguous WHICH account these win/loss numbers count,
                  so live trades are never mistaken for an empty paper dashboard (or vice-versa) */}
              <Show when={wallet()}>
                <span class={`rounded px-2 py-0.5 text-[11px] font-semibold ${wallet()!.simulated ? "bg-slate-700 text-slate-300" : "bg-emerald-900/60 text-emerald-300"}`}>
                  {wallet()!.simulated ? t("dash.modePaper") : t("dash.modeLive")}
                </span>
              </Show>
              <span class="text-xs text-slate-500">{t("dash.since")} {new Date(s.session_start).toLocaleString()}</span>
              <div class="ml-auto flex gap-2">
                <button class="rounded-lg border border-slate-600 px-3 py-1.5 text-sm text-slate-300 hover:bg-slate-800" disabled={busy()} onClick={resetSession}>
                  {t("dash.resetKeep")}
                </button>
                <button class="rounded-lg bg-amber-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-amber-500" disabled={busy()} onClick={resetWallet}>
                  {t("dash.resetAll")}
                </button>
              </div>
            </div>

            {/* win rate ring + main numbers */}
            <div class="grid gap-4 md:grid-cols-[auto_1fr]">
              <div class="flex items-center justify-center rounded-2xl border border-slate-800 bg-slate-900 p-6">
                <Ring pct={wr} />
              </div>
              <div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
                <Stat label={t("dash.wins")} value={`${s.wins}`} tone="up" />
                <Stat label={t("dash.losses")} value={`${s.losses}`} tone="down" />
                <Stat label={t("dash.closed")} value={`${s.closed}`} />
                <Stat label={t("dash.realized")} value={fmt(s.gross_pnl)} tone={s.gross_pnl >= 0 ? "up" : "down"} />
                <Stat label={t("dash.pf")} value={s.profit_factor ? s.profit_factor.toFixed(2) : "—"} hint={t("dash.pfHint")} />
                <Stat label={t("dash.unrealized")} value={fmt(unrealized)} tone={unrealized >= 0 ? "up" : "down"} />
                <Stat label={t("dash.avgWin")} value={fmt(s.avg_win)} tone="up" />
                <Stat label={t("dash.avgLoss")} value={fmt(s.avg_loss)} tone="down" />
                <Stat label={t("dash.bestWorst")} value={`${fmt(s.best)} / ${fmt(s.worst)}`} />
                <Stat label={t("dash.openCount")} value={`${positions().length}`} tone={positions().length > 0 ? "up" : undefined} />
                <Stat label={t("dash.buys")} value={`${s.buys}`} />
              </div>
            </div>

            {/* currently held positions (from real/simulated wallet) — show activity even before selling */}
            <div class="rounded-2xl border border-slate-800 bg-slate-900/60 p-4">
              <div class="mb-1 flex items-baseline gap-2">
                <h4 class="text-sm font-bold text-slate-200">{t("dash.openTitle")}</h4>
                <span class="text-xs text-slate-500">{t("dash.openHint")}</span>
              </div>
              <Show
                when={positions().length > 0}
                fallback={<div class="py-3 text-center text-sm text-slate-500">{t("dash.noOpen")}</div>}
              >
                <div class="overflow-x-auto">
                  <table class="w-full text-sm">
                    <thead>
                      <tr class="border-b border-slate-800 text-left text-xs text-slate-500">
                        <th class="py-1.5 pr-3 font-medium">{t("dash.colAsset")}</th>
                        <th class="py-1.5 pr-3 text-right font-medium">{t("dash.colAmount")}</th>
                        <th class="py-1.5 pr-3 text-right font-medium">{t("dash.colAvg")}</th>
                        <th class="py-1.5 pr-3 text-right font-medium">{t("dash.colLast")}</th>
                        <th class="py-1.5 pr-3 text-right font-medium">{t("dash.colValue")}</th>
                        <th class="py-1.5 text-right font-medium">{t("dash.colUnreal")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {positions().map((p) => {
                        const u = posUnreal(p);
                        return (
                          <tr class="border-b border-slate-800/50 last:border-0">
                            <td class="py-1.5 pr-3 font-semibold text-slate-200">{p.symbol}</td>
                            <td class="py-1.5 pr-3 text-right text-slate-300">{p.amount_base.toLocaleString(undefined, { maximumFractionDigits: 8 })}</td>
                            <td class="py-1.5 pr-3 text-right text-slate-300">{fmt(p.avg_price)}</td>
                            <td class="py-1.5 pr-3 text-right text-slate-300">{fmt(p.last_price)}</td>
                            <td class="py-1.5 pr-3 text-right text-slate-300">{fmt(p.amount_base * p.last_price)}</td>
                            <td class={`py-1.5 text-right font-semibold ${u >= 0 ? "text-emerald-400" : "text-red-400"}`}>
                              {u >= 0 ? "+" : ""}{fmt(u)}
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              </Show>
            </div>

            <Show when={s.closed === 0}>
              <div class="rounded-xl border border-dashed border-slate-800 p-4 text-center text-sm text-slate-500">
                {t("dash.noClosed")}
                <div class="mt-1 text-xs text-slate-600">{t("dash.noClosedHint")}</div>
              </div>
            </Show>

            <div class="text-xs text-slate-600">{t("dash.tip")}</div>

            <AlertsFeed />
          </div>
        );
      })()}
    </Show>
  );
}

/** log of system-notified events (insufficient funds, order failed, plan cancelled, etc.) — reads from /api/alerts */
function AlertsFeed() {
  const [alerts, setAlerts] = createSignal<AlertRecord[]>([]);
  onMount(async () => setAlerts(await api.alerts().catch(() => [])));

  const ICON: Record<string, string> = { error: "🚨", warn: "⚠️", info: "ℹ️" };
  const fmtTime = (iso: string) => new Date(iso).toLocaleString();

  return (
    <Show when={alerts().length > 0}>
      <div class="rounded-2xl border border-slate-800 bg-slate-900 p-4">
        <h3 class="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-400">
          {t("dash.alerts")}
        </h3>
        <div class="max-h-64 space-y-1 overflow-y-auto">
          <For each={alerts()}>
            {(a) => (
              <div class="flex items-start gap-2 border-t border-slate-800/60 py-1.5 text-xs">
                <span class="shrink-0">{ICON[a.level] ?? "ℹ️"}</span>
                <span class="min-w-0 flex-1 leading-relaxed text-slate-400">{a.message}</span>
                <span class="shrink-0 text-[10px] text-slate-600">{fmtTime(a.created_at)}</span>
              </div>
            )}
          </For>
        </div>
      </div>
    </Show>
  );
}

function Ring(props: { pct: number }) {
  const r = 52, c = 2 * Math.PI * r;
  const off = () => c * (1 - Math.max(0, Math.min(100, props.pct)) / 100);
  const color = () => (props.pct >= 50 ? "#10b981" : props.pct > 0 ? "#f59e0b" : "#475569");
  return (
    <div class="relative h-36 w-36">
      <svg class="h-36 w-36 -rotate-90" viewBox="0 0 120 120">
        <circle cx="60" cy="60" r={r} fill="none" stroke="#1e293b" stroke-width="12" />
        <circle cx="60" cy="60" r={r} fill="none" stroke={color()} stroke-width="12"
          stroke-dasharray={String(c)} stroke-dashoffset={String(off())} stroke-linecap="round" />
      </svg>
      <div class="absolute inset-0 flex flex-col items-center justify-center">
        <span class="text-2xl font-extrabold text-slate-100">{props.pct.toFixed(0)}%</span>
        <span class="text-xs text-slate-500">{t("dash.winRate")}</span>
      </div>
    </div>
  );
}

function Stat(props: { label: string; value: string; tone?: "up" | "down"; hint?: string }) {
  const c = props.tone === "up" ? "text-emerald-400" : props.tone === "down" ? "text-red-400" : "text-slate-100";
  return (
    <div class="rounded-xl border border-slate-800 bg-slate-900 p-3">
      <div class="text-xs text-slate-400">{props.label}</div>
      <div class={`mt-1 text-lg font-bold ${c}`}>{props.value}</div>
      <Show when={props.hint}><div class="text-[11px] text-slate-600">{props.hint}</div></Show>
    </div>
  );
}
