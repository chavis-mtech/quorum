import { createSignal, onMount, For, Show } from "solid-js";
import { api, type MarketScanItem } from "../api";
import { t } from "../i18n";

const fmt = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 4 });

/** AI market scanner — finds interesting symbols in the current period */
export default function DiscoveryPanel(props: {
  items?: MarketScanItem[];
  onAdd: (s: string) => void;
  onAnalyze: (s: string) => void;
}) {
  const [items, setItems] = createSignal<MarketScanItem[]>(props.items ?? []);
  const [loading, setLoading] = createSignal(false);

  async function scan() {
    setLoading(true);
    try {
      setItems(await api.marketScan(12));
    } finally {
      setLoading(false);
    }
  }
  onMount(() => { if (!items().length) scan(); });

  return (
    <div class="space-y-3">
      <div class="flex items-center gap-3">
        <h3 class="text-sm font-bold text-slate-200">{t("disc.title")}</h3>
        <button class="rounded-lg border border-slate-700 px-3 py-1 text-sm text-slate-300 hover:bg-slate-800" disabled={loading()} onClick={scan}>
          {loading() ? t("disc.scanning") : t("disc.scanAgain")}
        </button>
      </div>
      <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
        <For each={items()} fallback={<div class="text-slate-600">{t("disc.none")}</div>}>
          {(it) => {
            const up = it.change_24h >= 0;
            return (
              <div class="rounded-xl border border-slate-800 bg-slate-900 p-3">
                <div class="flex items-center justify-between">
                  <span class="font-bold text-slate-100">{it.symbol}</span>
                  <span class={`text-sm font-medium ${up ? "text-emerald-400" : "text-red-400"}`}>
                    {up ? "▲" : "▼"} {Math.abs(it.change_24h).toFixed(2)}%
                  </span>
                </div>
                <div class="mt-1 text-xs text-slate-500">{fmt(it.last_price)} THB · score {it.score.toFixed(1)}</div>
                <div class="mt-1 text-xs text-slate-600">{it.reason}</div>
                <div class="mt-2 flex gap-2">
                  <button class="rounded bg-sky-600 px-2 py-1 text-xs font-semibold text-white hover:bg-sky-500" onClick={() => props.onAnalyze(it.symbol)}>{t("disc.analyze")}</button>
                  <button class="rounded border border-slate-600 px-2 py-1 text-xs text-slate-300 hover:bg-slate-800" onClick={() => props.onAdd(it.symbol)}>{t("live.addWatch")}</button>
                </div>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}
