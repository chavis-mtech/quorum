import { createSignal, onMount, For, Show } from "solid-js";
import { api, type WalletView } from "../api";
import { t } from "../i18n";

const fmt = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 2 });

/** Paper wallet + reset balance button */
export default function WalletPanel() {
  const [w, setW] = createSignal<WalletView | null>(null);
  const [resetAmt, setResetAmt] = createSignal(100000);
  const [busy, setBusy] = createSignal(false);

  async function load() {
    try {
      setW(await api.getWallet());
    } catch {
      /* ignore */
    }
  }
  onMount(load);

  async function doReset() {
    if (!confirm(`Reset paper wallet to ${fmt(resetAmt())} THB? (clears all positions)`)) return;
    setBusy(true);
    try {
      await api.resetWallet(resetAmt());
      await load();
    } finally {
      setBusy(false);
    }
  }

  return (
    <Show when={w()} fallback={<div class="text-slate-500">Loading wallet...</div>}>
      <div class="space-y-4">
        <div class="grid grid-cols-2 gap-3 md:grid-cols-4">
          <Stat label={t("wallet.cash")} value={fmt(w()!.cash_quote)} />
          <Stat label={t("wallet.posValue")} value={fmt(w()!.positions_value)} />
          <Stat label={t("wallet.equity")} value={fmt(w()!.equity)} />
          <Stat
            label={t("wallet.pnl")}
            value={`${w()!.pnl >= 0 ? "+" : ""}${fmt(w()!.pnl)} (${(w()!.pnl_pct * 100).toFixed(2)}%)`}
            tone={w()!.pnl >= 0 ? "up" : "down"}
          />
        </div>

        <div class="flex items-end gap-2 rounded-xl border border-slate-800 bg-slate-900 p-3">
          <label class="text-sm">
            <span class="text-xs text-slate-400">{t("wallet.resetTo")}</span>
            <input
              type="number"
              step={1000}
              class="mt-1 w-40 rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100"
              value={resetAmt()}
              onInput={(e) => setResetAmt(Number(e.currentTarget.value))}
            />
          </label>
          <button class="rounded-lg bg-amber-600 px-4 py-2 text-sm font-semibold text-white hover:bg-amber-500 disabled:opacity-50" disabled={busy()} onClick={doReset}>
            {t("wallet.resetBtn")}
          </button>
          <button class="rounded-lg border border-slate-700 px-3 py-2 text-sm text-slate-300 hover:bg-slate-800" onClick={load}>
            {t("common.refresh")}
          </button>
        </div>

        <div class="overflow-hidden rounded-2xl border border-slate-800">
          <table class="w-full text-left text-sm">
            <thead class="bg-slate-900 text-xs uppercase text-slate-400">
              <tr><th class="px-3 py-2">{t("wallet.colCoin")}</th><th class="px-3 py-2">{t("wallet.colAmount")}</th><th class="px-3 py-2">{t("wallet.colAvg")}</th><th class="px-3 py-2">{t("wallet.colLast")}</th><th class="px-3 py-2">{t("wallet.colValue")}</th><th class="px-3 py-2">P&L</th></tr>
            </thead>
            <tbody>
              <For each={w()!.positions} fallback={<tr><td colspan="6" class="px-3 py-6 text-center text-slate-600">{t("wallet.noPos")}</td></tr>}>
                {(p) => {
                  const value = p.amount_base * p.last_price;
                  const pnl = (p.last_price - p.avg_price) * p.amount_base;
                  return (
                    <tr class="border-t border-slate-800">
                      <td class="px-3 py-2 font-semibold text-slate-200">{p.symbol}</td>
                      <td class="px-3 py-2 text-slate-400">{fmt(p.amount_base)}</td>
                      <td class="px-3 py-2 text-slate-400">{fmt(p.avg_price)}</td>
                      <td class="px-3 py-2 text-slate-400">{fmt(p.last_price)}</td>
                      <td class="px-3 py-2 text-slate-300">{fmt(value)}</td>
                      <td class={`px-3 py-2 font-medium ${pnl >= 0 ? "text-emerald-400" : "text-red-400"}`}>
                        {pnl >= 0 ? "+" : ""}{fmt(pnl)}
                      </td>
                    </tr>
                  );
                }}
              </For>
            </tbody>
          </table>
        </div>
      </div>
    </Show>
  );
}

function Stat(props: { label: string; value: string; tone?: "up" | "down" }) {
  const c = props.tone === "up" ? "text-emerald-400" : props.tone === "down" ? "text-red-400" : "text-slate-100";
  return (
    <div class="rounded-xl border border-slate-800 bg-slate-900 p-3">
      <div class="text-xs text-slate-400">{props.label}</div>
      <div class={`mt-1 text-lg font-bold ${c}`}>{props.value}</div>
    </div>
  );
}
