import { createSignal, For, Show } from "solid-js";
import type { TraceStep } from "../api";
import { t } from "../i18n";

const STAGE_ICON: Record<string, string> = {
  data: "📊",
  web: "🌐",
  news: "📰",
  agent: "🤖",
  consensus: "🗳️",
  judge: "⚖️",
};
const STATUS_DOT: Record<string, string> = {
  done: "bg-emerald-500",
  warn: "bg-amber-400",
  error: "bg-red-500",
  thinking: "bg-sky-400 animate-pulse",
};

/** Display extra data from an agent step, e.g. RSI, EMA, momentum */
function AgentExtra(props: { data: Record<string, any> }) {
  const num = (n: number) =>
    typeof n === "number" ? n.toLocaleString(undefined, { maximumFractionDigits: 4 }) : String(n);

  const entries = () =>
    Object.entries(props.data).filter(
      ([k]) => !["action", "confidence", "veto", "ok", "abstained"].includes(k)
    );

  return (
    <Show when={entries().length > 0}>
      <div class="mt-1.5 flex flex-wrap gap-x-3 gap-y-0.5">
        <For each={entries()}>
          {([k, val]) => (
            <span class="text-[11px] text-slate-600">
              <span class="text-slate-500">{k}:</span>{" "}
              <span class="text-slate-400">{typeof val === "number" ? num(val) : String(val)}</span>
            </span>
          )}
        </For>
      </div>
    </Show>
  );
}

/** Display the vote tally table in a consensus step */
function ConsensusTally(props: { tally: Record<string, number> }) {
  const entries = () => Object.entries(props.tally).filter(([, v]) => v > 0);
  const colors: Record<string, string> = { BUY: "text-emerald-400", SELL: "text-red-400", HOLD: "text-amber-400" };
  return (
    <Show when={entries().length > 0}>
      <div class="mt-1 flex gap-3">
        <For each={entries()}>
          {([k, v]) => (
            <span class="text-xs">
              <span class={colors[k] ?? "text-slate-400"}>{k}</span>
              <span class="text-slate-600"> {v.toFixed(2)}</span>
            </span>
          )}
        </For>
      </div>
    </Show>
  );
}

/** Timeline showing "what the AI is thinking" step by step — click to expand details/thinking */
export default function ReasoningTrace(props: { steps: TraceStep[]; thinking?: string }) {
  const [openThink, setOpenThink] = createSignal(true);

  return (
    <div class="rounded-2xl border border-slate-800 bg-slate-900 p-4">
      <h3 class="mb-3 text-xs font-semibold uppercase tracking-wide text-slate-400">
        {t("trace.title")}
      </h3>

      <ol class="relative ml-3 border-l border-slate-700">
        <For each={props.steps}>
          {(s) => (
            <li class="mb-4 ml-5">
              <span
                class={`absolute -left-[7px] mt-1.5 h-3 w-3 rounded-full ring-4 ring-slate-900 ${STATUS_DOT[s.status] ?? "bg-slate-500"}`}
              />
              <div class="flex items-center gap-2">
                <span>{STAGE_ICON[s.stage] ?? "•"}</span>
                <span class="text-sm font-semibold text-slate-200">{s.title}</span>
                <span class="ml-auto text-xs text-slate-600">{s.elapsed_ms}ms</span>
              </div>
              <Show when={s.detail}>
                <p class="mt-1 whitespace-pre-line text-xs leading-relaxed text-slate-400">{s.detail}</p>
              </Show>

              {/* agent: display indicator data (RSI, EMA, sentiment score, etc.) */}
              <Show when={s.stage === "agent" && s.data && Object.keys(s.data).length > 0}>
                <AgentExtra data={s.data} />
              </Show>

              {/* consensus: display weighted vote tally */}
              <Show when={s.stage === "consensus" && s.data?.tally}>
                <ConsensusTally tally={s.data.tally as Record<string, number>} />
              </Show>

              {/* web snippets if available */}
              <Show when={s.stage === "web" && Array.isArray(s.data?.snippets) && s.data.snippets.length}>
                <ul class="mt-1 space-y-0.5">
                  <For each={s.data.snippets as string[]}>
                    {(sn) => <li class="text-xs text-slate-500">{sn}</li>}
                  </For>
                </ul>
              </Show>
            </li>
          )}
        </For>
      </ol>

      {/* thinking output from the Judge LLM */}
      <Show when={props.thinking}>
        <div class="mt-2 rounded-xl border border-sky-900 bg-sky-950/40">
          <button
            class="flex w-full items-center justify-between px-3 py-2 text-sm font-semibold text-sky-300"
            onClick={() => setOpenThink(!openThink())}
          >
            <span>{t("trace.thinking")}</span>
            <span>{openThink() ? "▲" : "▼"}</span>
          </button>
          <Show when={openThink()}>
            <pre class="max-h-72 overflow-auto whitespace-pre-wrap px-3 pb-3 text-xs leading-relaxed text-slate-400">
              {props.thinking}
            </pre>
          </Show>
        </div>
      </Show>
    </div>
  );
}
