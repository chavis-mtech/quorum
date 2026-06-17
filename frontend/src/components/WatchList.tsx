import { For } from "solid-js";
import { t } from "../i18n";

/** Displays the watchlist of stocks/coins + an analyze-now button (items added via autocomplete above) */
export default function WatchList(props: {
  symbols: string[];
  current: string | null;
  onSetSymbols: (s: string[]) => void;
  onAnalyze: (s: string) => void;
}) {
  function remove(s: string) {
    props.onSetSymbols(props.symbols.filter((x) => x !== s));
  }

  return (
    <div class="rounded-2xl border border-slate-800 bg-slate-900 p-4">
      <h3 class="mb-3 text-xs font-semibold uppercase tracking-wide text-slate-400">
        {t("live.watch")}
      </h3>
      <div class="flex flex-wrap gap-2">
        <For each={props.symbols}>
          {(s) => (
            <span
              class="group flex items-center gap-2 rounded-full border px-3 py-1 text-sm"
              classList={{
                "border-sky-500 bg-sky-950 text-sky-300": props.current === s,
                "border-slate-700 bg-slate-800 text-slate-300": props.current !== s,
              }}
            >
              <button class="font-semibold hover:underline" onClick={() => props.onAnalyze(s)} title="Analyze now">
                {props.current === s ? "⟳ " : ""}{s}
              </button>
              <button class="text-slate-500 hover:text-red-400" onClick={() => remove(s)}>×</button>
            </span>
          )}
        </For>
      </div>
    </div>
  );
}
