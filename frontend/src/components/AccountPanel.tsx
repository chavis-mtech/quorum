import { createSignal, onMount, For, Show } from "solid-js";
import { api, type Balance } from "../api";
import { t } from "../i18n";

const fmt = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 8 });

/** Live balance from Bitkub (uses the configured API key) */
export default function AccountPanel() {
  const [balances, setBalances] = createSignal<Balance[]>([]);
  const [err, setErr] = createSignal("");
  const [loading, setLoading] = createSignal(false);

  async function load() {
    setLoading(true);
    setErr("");
    try {
      const r = await api.accountBalance();
      setBalances(r.balances);
    } catch (e) {
      setErr(String(e));
      setBalances([]);
    } finally {
      setLoading(false);
    }
  }
  onMount(load);

  return (
    <div class="space-y-3">
      <div class="flex items-center gap-3">
        <h3 class="text-sm font-bold text-slate-200">{t("acct.title")}</h3>
        <button class="rounded-lg border border-slate-700 px-3 py-1 text-sm text-slate-300 hover:bg-slate-800" onClick={load}>
          {t("common.refresh")}
        </button>
      </div>

      <Show when={err()}>
        <div class="rounded-lg bg-red-950 px-3 py-2 text-sm text-red-300">
          {err()}
          <div class="mt-1 text-xs text-red-400/70">
            You must configure a Bitkub API key (use the "API Settings" button above) and enable balance read permission on the key.
          </div>
        </div>
      </Show>

      <Show when={loading()}><div class="text-slate-500">{t("common.loading")}</div></Show>

      <Show when={!loading() && !err() && balances().length === 0}>
        <div class="rounded-2xl border border-dashed border-slate-800 p-8 text-center">
          <div class="text-3xl">🪙</div>
          <div class="mt-2 font-semibold text-slate-300">{t("acct.empty")}</div>
          <div class="mt-1 text-sm text-slate-500">{t("acct.emptyHint")}</div>
        </div>
      </Show>

      <Show when={balances().length}>
        <div class="overflow-hidden rounded-2xl border border-slate-800">
          <table class="w-full text-left text-sm">
            <thead class="bg-slate-900 text-xs uppercase text-slate-400">
              <tr><th class="px-3 py-2">{t("acct.asset")}</th><th class="px-3 py-2 text-right">{t("acct.available")}</th></tr>
            </thead>
            <tbody>
              <For each={balances()}>
                {(b) => (
                  <tr class="border-t border-slate-800">
                    <td class="px-3 py-2 font-semibold text-slate-200">{b.asset}</td>
                    <td class="px-3 py-2 text-right text-slate-300">{fmt(b.available)}</td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
      </Show>
    </div>
  );
}
