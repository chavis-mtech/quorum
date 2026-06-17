import { createEffect, createSignal, For, onMount, Show } from "solid-js";
import { api, type AiCompareResult, type AiProvider, type AiCredentialStatus, type BrokerCredentialStatus, type TradingSettings } from "../api";
import { t, lang, setLang } from "../i18n";

const fmtWhen = (iso?: string | null) => (iso ? new Date(iso).toLocaleString() : "");

/** Detailed trading settings — used by the AI during automated trading */
export default function SettingsPanel(props: {
  onSaved?: (s: TradingSettings) => void;
  bitkubStatus?: BrokerCredentialStatus | null;
  onManageBroker?: () => void;
}) {
  const [s, setS] = createSignal<TradingSettings | null>(null);
  const [saving, setSaving] = createSignal(false);
  const [msg, setMsg] = createSignal("");
  const [aiKey, setAiKey] = createSignal("");
  const [aiMsg, setAiMsg] = createSignal("");
  const [aiStatus, setAiStatus] = createSignal<AiCredentialStatus | null>(null);
  const [compareSymbol, setCompareSymbol] = createSignal("BTC");
  const [compareBusy, setCompareBusy] = createSignal(false);
  const [compare, setCompare] = createSignal<AiCompareResult | null>(null);
  const [models, setModels] = createSignal<string[]>([]);
  const [modelsSource, setModelsSource] = createSignal<"ollama" | "catalog" | null>(null);
  const [modelsBusy, setModelsBusy] = createSignal(false);
  const [customModel, setCustomModel] = createSignal(false);
  // List for the select — always include the current value (in case it's not in the catalog)
  const modelOptions = () => {
    const cur = s()?.ai_judge_model?.trim();
    const list = models();
    return cur && !list.includes(cur) ? [cur, ...list] : list;
  };

  onMount(async () => {
    const raw = await api.getSettings();
    setS(raw);
  });
  const upd = (patch: Partial<TradingSettings>) => setS({ ...s()!, ...patch });

  let lastAiProvider = "";
  createEffect(() => {
    const provider = s()?.ai_judge_provider;
    if (!provider || provider === lastAiProvider) return;
    lastAiProvider = provider;
    setAiKey("");
    setCompare(null);
    setCustomModel(false);
    refreshAiStatus(provider);
    loadModels(provider);
  });

  async function refreshAiStatus(provider: AiProvider) {
    setAiStatus(await api.aiCredentialsStatus(provider).catch(() => null));
  }

  async function loadModels(provider: AiProvider) {
    if (provider === "none") {
      setModels([]);
      setModelsSource(null);
      return;
    }
    setModelsBusy(true);
    try {
      const r = await api.aiModels(provider);
      setModels(r.models ?? []);
      setModelsSource(r.source);
    } catch {
      setModels([]);
      setModelsSource(null);
    } finally {
      setModelsBusy(false);
    }
  }

  const providerNeedsKey = (provider?: AiProvider) => !["ollama", "none"].includes(provider ?? "ollama");
  const providerDefaultModel = (provider: AiProvider) =>
    ({
      ollama: "qwen3:14b",
      openai: "gpt-4o-mini",
      anthropic: "claude-3-5-haiku-latest",
      groq: "llama-3.3-70b-versatile",
      openrouter: "openai/gpt-4o-mini",
      custom: "",
      none: "",
    })[provider];
  const providerDefaultBaseUrl = (provider: AiProvider) =>
    ({
      ollama: "",
      openai: "https://api.openai.com/v1",
      anthropic: "",
      groq: "https://api.groq.com/openai/v1",
      openrouter: "https://openrouter.ai/api/v1",
      custom: "",
      none: "",
    })[provider];
  function setProvider(provider: AiProvider) {
    upd({
      ai_judge_provider: provider,
      ai_judge_model: providerDefaultModel(provider),
      ai_judge_base_url: providerDefaultBaseUrl(provider),
      ai_judge_enabled: provider !== "none",
    });
  }

  async function save() {
    setSaving(true);
    setMsg("");
    try {
      await api.putSettings(s()!);
      setMsg(t("set.saved"));
      props.onSaved?.(s()!);
    } catch (e) {
      setMsg("" + e);
    } finally {
      setSaving(false);
    }
  }

  async function saveAiKey() {
    const provider = s()!.ai_judge_provider;
    if (!providerNeedsKey(provider)) return;
    setAiMsg("");
    try {
      await api.setAiCredentials(provider, aiKey().trim());
      setAiKey("");
      await refreshAiStatus(provider);
      setAiMsg(t("set.aiKeySaved"));
    } catch (e) {
      setAiMsg("" + e);
    }
  }

  async function testCompare() {
    setCompareBusy(true);
    setCompare(null);
    setAiMsg("");
    try {
      await api.putSettings(s()!);
      const provider = s()!.ai_judge_provider;
      if (providerNeedsKey(provider) && aiKey().trim()) {
        await api.setAiCredentials(provider, aiKey().trim());
        setAiKey("");
        await refreshAiStatus(provider);
      }
      setCompare(await api.compareAi(compareSymbol().trim().toUpperCase() || "BTC"));
    } catch (e) {
      setAiMsg("" + e);
    } finally {
      setCompareBusy(false);
    }
  }

  const Num = (p: { label: string; val: number; step?: number; pct?: boolean; onIn: (n: number) => void; hint?: string }) => (
    <label class="block">
      <span class="text-xs text-slate-400">{p.label}</span>
      <input
        type="number"
        step={p.step ?? 1}
        class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500"
        value={p.pct ? p.val * 100 : p.val}
        onInput={(e) => p.onIn(p.pct ? Number(e.currentTarget.value) / 100 : Number(e.currentTarget.value))}
      />
      <Show when={p.hint}><span class="text-[11px] text-slate-600">{p.hint}</span></Show>
    </label>
  );
  const Toggle = (p: { label: string; val: boolean; onT: (b: boolean) => void; hint?: string }) => (
    <button
      class="flex items-center justify-between rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-left"
      onClick={() => p.onT(!p.val)}
    >
      <div>
        <div class="text-sm text-slate-200">{p.label}</div>
        <Show when={p.hint}><div class="text-[11px] text-slate-600">{p.hint}</div></Show>
      </div>
      <span class={`h-5 w-9 rounded-full p-0.5 transition ${p.val ? "bg-emerald-500" : "bg-slate-600"}`}>
        <span class={`block h-4 w-4 rounded-full bg-white transition ${p.val ? "translate-x-4" : ""}`} />
      </span>
    </button>
  );
  const CompareLeg = (p: { leg: AiCompareResult["local"] }) => (
    <div class="rounded-lg border border-slate-800 bg-slate-950/50 p-3">
      <div class="flex items-center gap-2">
        <b class="text-sm text-slate-200">{p.leg.label === "local" ? t("set.aiLocal") : t("set.aiSelected")}</b>
        <span class="rounded bg-slate-800 px-2 py-0.5 text-[11px] text-slate-400">{p.leg.provider}</span>
        <span class="ml-auto text-[11px] text-slate-500">{p.leg.elapsed_ms}ms</span>
      </div>
      <Show when={p.leg.ok} fallback={<p class="mt-2 text-sm text-red-300">{p.leg.error}</p>}>
        <div class="mt-2 flex items-center gap-2 text-sm">
          <span class="rounded bg-sky-950 px-2 py-0.5 font-bold text-sky-300">{p.leg.action}</span>
          <span class="text-slate-400">{Math.round((p.leg.confidence ?? 0) * 100)}%</span>
          <span class="text-xs text-slate-600">{p.leg.engine}</span>
        </div>
        <p class="mt-2 text-sm leading-relaxed text-slate-300">{p.leg.thesis || p.leg.reasoning}</p>
        <Show when={p.leg.next_step}><p class="mt-1 text-xs text-slate-500">{p.leg.next_step}</p></Show>
      </Show>
    </div>
  );

  return (
    <Show when={s()} fallback={<div class="text-slate-500">Loading...</div>}>
      <div class="space-y-4 rounded-2xl border border-slate-800 bg-slate-900 p-5">
        <div class="flex items-center gap-3">
          <h3 class="text-sm font-bold text-slate-200">{t("set.title")}</h3>
          <div class="ml-auto flex items-center gap-2">
            <span class="text-xs text-slate-400">{t("set.language")}</span>
            <div class="flex overflow-hidden rounded-lg border border-slate-700">
              <button class="px-3 py-1 text-sm" classList={{ "bg-sky-600 text-white": lang() === "th", "text-slate-300 hover:bg-slate-800": lang() !== "th" }} onClick={() => setLang("th")}>TH</button>
              <button class="px-3 py-1 text-sm" classList={{ "bg-sky-600 text-white": lang() === "en", "text-slate-300 hover:bg-slate-800": lang() !== "en" }} onClick={() => setLang("en")}>EN</button>
            </div>
          </div>
        </div>

        <div class="grid gap-3 sm:grid-cols-3">
          <label class="block">
            <span class="text-xs text-slate-400">{t("set.mode")}</span>
            <select
              class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100"
              value={s()!.mode}
              onChange={(e) => upd({ mode: e.currentTarget.value as any })}
            >
              <option value="signal-only">{t("set.modeSignal")}</option>
              <option value="paper">{t("set.modePaper")}</option>
              <option value="live">{t("set.modeLive")}</option>
            </select>
          </label>
          <label class="block">
            <span class="text-xs text-slate-400">{t("set.broker")}</span>
            <select
              class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100"
              value={s()!.broker ?? "bitkub"}
              onChange={(e) => upd({ broker: e.currentTarget.value as any })}
            >
              <option value="bitkub">Bitkub (THB) ✓</option>
              <option value="binance">Binance — {t("set.brokerSoon")}</option>
            </select>
            <span class="mt-0.5 block text-[11px] text-slate-600">{t("set.brokerHint2")}</span>
          </label>
          <Toggle label={t("set.autoTrade")} val={s()!.auto_trade} onT={(b) => upd({ auto_trade: b })} hint={t("set.autoTradeHint")} />
          <Toggle label={t("set.allowSell")} val={s()!.allow_sell} onT={(b) => upd({ allow_sell: b })} hint={t("set.allowSellHint")} />
        </div>

        <div class="grid gap-3 sm:grid-cols-3">
          <Num label={t("set.amount")} val={s()!.trade_amount_quote} step={100} onIn={(n) => upd({ trade_amount_quote: n })} />
          <Num label={t("set.minConf")} val={s()!.min_confidence} pct step={1} onIn={(n) => upd({ min_confidence: n })} hint={t("set.minConfHint")} />
          <Num label={t("set.maxPos")} val={s()!.max_position_pct} pct step={1} onIn={(n) => upd({ max_position_pct: n })} />
          <Num label={t("set.dailyLoss")} val={s()!.daily_loss_limit} pct step={1} onIn={(n) => upd({ daily_loss_limit: n })} />
          <Num label={t("set.maxOpen")} val={s()!.max_open_positions} step={1} onIn={(n) => upd({ max_open_positions: n })} />
          <Num label={t("set.tp")} val={s()!.take_profit_pct} pct step={1} onIn={(n) => upd({ take_profit_pct: n })} />
          <Num label={t("set.sl")} val={s()!.stop_loss_pct} pct step={1} onIn={(n) => upd({ stop_loss_pct: n })} />
        </div>

        <div class="grid gap-3 sm:grid-cols-2">
          <Toggle label={t("set.discovery")} val={s()!.discovery_enabled} onT={(b) => upd({ discovery_enabled: b })} hint={t("set.discoveryHint")} />
          <Num label={t("set.discoveryN")} val={s()!.discovery_top_n} step={1} onIn={(n) => upd({ discovery_top_n: n })} />
        </div>

        {/* active position management — trailing stop / breakeven */}
        <div class="grid gap-3 sm:grid-cols-2">
          <label class="block">
            <span class="text-xs text-slate-400">{t("set.manageStyle")}</span>
            <select
              class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100"
              value={s()!.manage_style ?? "conservative"}
              onChange={(e) => upd({ manage_style: e.currentTarget.value as any })}
            >
              <option value="off">{t("set.manageOff")}</option>
              <option value="conservative">{t("set.manageConservative")}</option>
              <option value="balanced">{t("set.manageBalanced")}</option>
              <option value="aggressive">{t("set.manageAggressive")}</option>
            </select>
            <span class="mt-0.5 block text-[11px] text-slate-600">{t("set.manageHint")}</span>
          </label>
          <Toggle label={t("set.letWinnersRun")} val={s()!.let_winners_run ?? true} onT={(b) => upd({ let_winners_run: b })} hint={t("set.letWinnersRunHint")} />
        </div>

        <div class="space-y-3 border-t border-slate-800 pt-4">
          <div class="flex flex-wrap items-center gap-2">
            <h4 class="text-sm font-bold text-slate-200">{t("set.connTitle")}</h4>
            <span class="text-xs text-slate-500">{t("set.connHint")}</span>
          </div>

          <div class="flex flex-wrap items-center gap-3 rounded-lg border border-slate-800 bg-slate-950/50 p-3">
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span class={`h-2 w-2 rounded-full ${props.bitkubStatus?.configured ? "bg-emerald-400" : "bg-slate-500"}`} />
                <span class="text-sm font-semibold text-slate-200">{t("set.brokerBitkub")}</span>
                <span
                  class="rounded px-2 py-0.5 text-[11px]"
                  classList={{
                    "bg-emerald-950 text-emerald-300": !!props.bitkubStatus?.configured,
                    "bg-slate-800 text-slate-400": !props.bitkubStatus?.configured,
                  }}
                >
                  {props.bitkubStatus?.configured ? t("common.connected") : t("common.notConnected")}
                </span>
              </div>
              <Show
                when={props.bitkubStatus?.configured}
                fallback={<p class="mt-1 text-[11px] text-slate-500">{t("set.brokerHint")}</p>}
              >
                <div class="mt-1 flex flex-wrap items-center gap-x-3 text-[11px] text-slate-500">
                  <span class="font-mono text-slate-300">{props.bitkubStatus?.api_key_hint || "••••"}</span>
                  <Show when={props.bitkubStatus?.updated_at}>
                    <span>· {t("common.setOn")} {fmtWhen(props.bitkubStatus?.updated_at)}</span>
                  </Show>
                </div>
              </Show>
            </div>
            <button
              class="rounded-lg border border-slate-700 px-3 py-1.5 text-sm font-semibold text-slate-200 hover:bg-slate-800"
              onClick={() => props.onManageBroker?.()}
            >
              {props.bitkubStatus?.configured ? t("set.manage") : t("set.connect")}
            </button>
          </div>
        </div>

        <div class="space-y-3 border-t border-slate-800 pt-4">
          <div class="flex flex-wrap items-center gap-2">
            <h4 class="text-sm font-bold text-slate-200">{t("set.aiTitle")}</h4>
            <span
              class="rounded px-2 py-0.5 text-[11px]"
              classList={{
                "bg-emerald-950 text-emerald-300": !!aiStatus()?.configured,
                "bg-red-950 text-red-300": aiStatus()?.configured === false,
                "bg-slate-800 text-slate-400": !aiStatus(),
              }}
            >
              {aiStatus()?.needs_key ? (aiStatus()?.configured ? t("set.aiKeyReady") : t("set.aiKeyMissing")) : t("set.aiNoKey")}
            </span>
            <span class="text-xs text-slate-500">{t("set.aiHint")}</span>
          </div>

          <div class="grid gap-3 sm:grid-cols-3">
            <Toggle label={t("set.aiEnabled")} val={s()!.ai_judge_enabled} onT={(b) => upd({ ai_judge_enabled: b })} />
            <label class="block">
              <span class="text-xs text-slate-400">{t("set.aiProvider")}</span>
              <select
                class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100"
                value={s()!.ai_judge_provider}
                onChange={(e) => setProvider(e.currentTarget.value as AiProvider)}
              >
                <option value="ollama">Ollama local</option>
                <option value="openai">OpenAI</option>
                <option value="anthropic">Anthropic Claude</option>
                <option value="groq">Groq</option>
                <option value="openrouter">OpenRouter</option>
                <option value="custom">Custom OpenAI-compatible</option>
                <option value="none">Rule-based fallback</option>
              </select>
            </label>
            <label class="block">
              <span class="flex items-center gap-1.5 text-xs text-slate-400">
                {t("set.aiModel")}
                <Show when={s()!.ai_judge_provider !== "none"}>
                  <span class="text-[10px] text-slate-600">
                    {modelsSource() === "ollama"
                      ? `· ${models().length} local`
                      : modelsSource() === "catalog"
                        ? "· recommended"
                        : ""}
                  </span>
                  <button
                    type="button"
                    class="ml-auto rounded px-1.5 text-[11px] text-slate-500 hover:bg-slate-800 hover:text-slate-300 disabled:opacity-40"
                    title="Reload model list"
                    disabled={modelsBusy()}
                    onClick={() => loadModels(s()!.ai_judge_provider)}
                  >
                    {modelsBusy() ? "…" : "⟳"}
                  </button>
                </Show>
              </span>
              <Show
                when={!customModel() && modelOptions().length > 0}
                fallback={
                  <input
                    class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500"
                    value={s()!.ai_judge_model}
                    onInput={(e) => upd({ ai_judge_model: e.currentTarget.value })}
                    placeholder={providerDefaultModel(s()!.ai_judge_provider)}
                  />
                }
              >
                <select
                  class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500"
                  value={s()!.ai_judge_model}
                  onChange={(e) => {
                    const v = e.currentTarget.value;
                    if (v === "__custom__") { setCustomModel(true); return; }
                    upd({ ai_judge_model: v });
                  }}
                >
                  <For each={modelOptions()}>{(m) => <option value={m}>{m}</option>}</For>
                  <option value="__custom__">✏️ Type manually…</option>
                </select>
              </Show>
              <div class="mt-1 flex items-center gap-2 text-[11px]">
                <Show when={customModel() && models().length > 0}>
                  <button type="button" class="text-sky-500 hover:underline" onClick={() => setCustomModel(false)}>
                    ← Select from list
                  </button>
                </Show>
                <Show when={modelsSource() === "ollama" && !modelsBusy() && models().length === 0}>
                  <span class="text-amber-600/80">
                    No local models found — check that Ollama is running + URL is correct, then click ⟳
                  </span>
                </Show>
              </div>
            </label>
          </div>

          <div class="grid gap-3 sm:grid-cols-2">
            <label class="block">
              <span class="text-xs text-slate-400">{t("set.aiOllamaUrl")}</span>
              <input
                class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500"
                value={s()!.ai_judge_ollama_url}
                onInput={(e) => upd({ ai_judge_ollama_url: e.currentTarget.value })}
              />
            </label>
            <Show when={providerNeedsKey(s()!.ai_judge_provider)}>
              <label class="block">
                <span class="text-xs text-slate-400">{t("set.aiBaseUrl")}</span>
                <input
                  class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500"
                  value={s()!.ai_judge_base_url}
                  onInput={(e) => upd({ ai_judge_base_url: e.currentTarget.value })}
                  placeholder={providerDefaultBaseUrl(s()!.ai_judge_provider)}
                />
              </label>
            </Show>
          </div>

          <div class="grid gap-3 sm:grid-cols-2">
            <Toggle label={t("set.aiThinking")} val={s()!.ai_judge_thinking} onT={(b) => upd({ ai_judge_thinking: b })} hint={t("set.aiThinkingHint")} />
            <Show when={providerNeedsKey(s()!.ai_judge_provider)}>
              <div>
                <div class="flex gap-2">
                  <label class="min-w-0 flex-1">
                    <span class="text-xs text-slate-400">{t("set.aiApiKey")}</span>
                    <input
                      type="password"
                      class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500"
                      value={aiKey()}
                      onInput={(e) => setAiKey(e.currentTarget.value)}
                      placeholder={aiStatus()?.configured ? t("set.aiKeyReplace") : t("set.aiKeyPlaceholder")}
                    />
                  </label>
                  <button
                    class="mt-5 rounded-lg border border-slate-700 px-3 py-2 text-sm font-semibold text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                    disabled={!aiKey().trim()}
                    onClick={saveAiKey}
                  >
                    {t("common.save")}
                  </button>
                </div>
                <Show
                  when={aiStatus()?.configured}
                  fallback={<p class="mt-1 text-[11px] text-amber-500/80">{t("set.aiKeyMissing")}</p>}
                >
                  <p class="mt-1 text-[11px] text-slate-500">
                    {t("set.aiKeyActive")}: <span class="font-mono text-slate-300">{aiStatus()?.api_key_hint || "••••"}</span>
                    <Show when={aiStatus()?.updated_at}><span> · {t("common.setOn")} {fmtWhen(aiStatus()?.updated_at)}</span></Show>
                  </p>
                </Show>
              </div>
            </Show>
          </div>

          <div class="flex flex-wrap items-end gap-2">
            <label class="w-32">
              <span class="text-xs text-slate-400">{t("set.aiCompareSymbol")}</span>
              <input
                class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm font-semibold uppercase text-slate-100 outline-none focus:border-sky-500"
                value={compareSymbol()}
                onInput={(e) => setCompareSymbol(e.currentTarget.value.toUpperCase())}
              />
            </label>
            <button
              class="rounded-lg bg-violet-600 px-4 py-2 text-sm font-semibold text-white hover:bg-violet-500 disabled:opacity-50"
              disabled={compareBusy()}
              onClick={testCompare}
            >
              {compareBusy() ? t("set.aiComparing") : t("set.aiCompare")}
            </button>
            <span class="text-sm text-slate-400">{aiMsg()}</span>
          </div>

          <Show when={compare()} keyed>
            {(r) => (
              <div class="grid gap-3 lg:grid-cols-2">
                <CompareLeg leg={r.local} />
                <CompareLeg leg={r.selected} />
              </div>
            )}
          </Show>
        </div>

        <Show when={s()!.mode === "live"}>
          <div class="rounded-lg bg-red-950 px-3 py-2 text-sm text-red-300">{t("set.liveWarn")}</div>
        </Show>

        <div class="flex items-center gap-3">
          <button class="rounded-lg bg-sky-600 px-4 py-2 text-sm font-semibold text-white hover:bg-sky-500 disabled:opacity-50" disabled={saving()} onClick={save}>
            {saving() ? t("common.saving") : t("set.saveBtn")}
          </button>
          <span class="text-sm text-slate-400">{msg()}</span>
        </div>
      </div>
    </Show>
  );
}
