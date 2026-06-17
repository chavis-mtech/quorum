import { createSignal, For, Show, onCleanup } from "solid-js";
import { api, type SymbolTicker } from "../api";
import { t as tr } from "../i18n";

/** Autocomplete search field for stocks/coins + displays price/percentage 24h */
export default function SymbolSearch(props: {
  onPick: (symbol: string) => void;
  onAdd: (symbol: string) => void;
}) {
  const [q, setQ] = createSignal("");
  const [results, setResults] = createSignal<SymbolTicker[]>([]);
  const [open, setOpen] = createSignal(false);
  const [loading, setLoading] = createSignal(false);
  let timer: number | undefined;

  function onInput(val: string) {
    setQ(val);
    setOpen(true);
    if (timer) clearTimeout(timer);
    timer = window.setTimeout(async () => {
      setLoading(true);
      try {
        setResults(await api.searchSymbols(val, 12));
      } catch {
        setResults([]);
      } finally {
        setLoading(false);
      }
    }, 200); // debounce
  }
  onCleanup(() => timer && clearTimeout(timer));

  const fmt = (n: number) =>
    n >= 1 ? n.toLocaleString(undefined, { maximumFractionDigits: 2 }) : n.toPrecision(4);

  return (
    <div class="relative">
      <div class="flex gap-2">
        <input
          class="flex-1 rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500"
          placeholder={tr("live.searchPlaceholder")}
          value={q()}
          onInput={(e) => onInput(e.currentTarget.value)}
          onFocus={() => q() && setOpen(true)}
          onBlur={() => setTimeout(() => setOpen(false), 150)}
        />
      </div>

      <Show when={open() && (results().length > 0 || loading())}>
        <div class="absolute z-20 mt-1 max-h-80 w-full overflow-auto rounded-xl border border-slate-700 bg-slate-900 shadow-2xl">
          <Show when={loading()}>
            <div class="px-3 py-2 text-sm text-slate-500">{tr("common.loading")}</div>
          </Show>
          <For each={results()}>
            {(t) => {
              const up = t.change_24h_pct >= 0;
              return (
                <div class="flex items-center gap-3 border-t border-slate-800 px-3 py-2 hover:bg-slate-800">
                  <button class="flex-1 text-left" onMouseDown={() => props.onPick(t.symbol)}>
                    <div class="flex items-baseline gap-2">
                      <span class="font-bold text-slate-100">{t.symbol}</span>
                      <span class="text-xs text-slate-500">/{t.quote}</span>
                    </div>
                    <div class="text-xs text-slate-500">
                      H {fmt(t.high_24h)} · L {fmt(t.low_24h)} · vol {fmt(t.volume_24h)}
                    </div>
                  </button>
                  <div class="text-right">
                    <div class="font-semibold text-slate-200">{fmt(t.last)}</div>
                    <div class={`text-xs font-medium ${up ? "text-emerald-400" : "text-red-400"}`}>
                      {up ? "▲" : "▼"} {Math.abs(t.change_24h_pct).toFixed(2)}%
                    </div>
                  </div>
                  <button
                    class="rounded-md border border-slate-600 px-2 py-1 text-xs text-slate-300 hover:bg-slate-700"
                    onMouseDown={() => props.onAdd(t.symbol)}
                    title="Add to watchlist"
                  >
                    {tr("live.addWatch")}
                  </button>
                </div>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
}
