import { createEffect, createSignal, onCleanup, onMount, Show, For } from "solid-js";
import { api, connectLive, type Analysis, type BrokerCredentialStatus, type GovernorState, type LiveEvent, type MarketScanItem, type TradeRecord } from "./api";
import { token, activeAccount, setSession } from "./session";
import { t, lang, toggleLang } from "./i18n";
import LoginView from "./components/LoginView";
import AccountSwitcher from "./components/AccountSwitcher";
import GovernorBar from "./components/GovernorBar";
import ProfilePanel from "./components/ProfilePanel";
import CredentialsModal from "./components/CredentialsModal";
import SymbolSearch from "./components/SymbolSearch";
import WatchList from "./components/WatchList";
import ConsensusView from "./components/ConsensusView";
import AgentVotesPanel from "./components/AgentVotesPanel";
import ReasoningTrace from "./components/ReasoningTrace";
import ReportView from "./components/ReportView";
import SettingsPanel from "./components/SettingsPanel";
import PortfolioPanel from "./components/PortfolioPanel";
import DashboardPanel from "./components/DashboardPanel";
import TradesView from "./components/TradesView";
import HistoryView from "./components/HistoryView";
import DiscoveryPanel from "./components/DiscoveryPanel";
import TrackingPanel from "./components/TrackingPanel";

/** Root — auth gate; loads AppShell once authenticated (remounts on account switch) */
export default function App() {
  const [booted, setBooted] = createSignal(false);

  onMount(async () => {
    if (token()) {
      try {
        const m = await api.me();
        setSession(token()!, m.user, m.accounts);
      } catch {
        /* 401 → session already cleared */
      }
    }
    setBooted(true);
  });

  return (
    <Show when={booted()} fallback={<div class="flex min-h-screen items-center justify-center bg-slate-950 text-slate-500">…</div>}>
      <Show when={token()} fallback={<LoginView />}>
        {/* remount everything on account switch → no stale cross-account data */}
        <Show when={activeAccount()?.id} keyed>
          {(aid) => <AppShell accountId={aid} />}
        </Show>
      </Show>
    </Show>
  );
}

type Tab = "live" | "targets" | "dashboard" | "history" | "trades" | "portfolio" | "discovery" | "report" | "settings" | "profile";
const TAB_IDS: Tab[] = ["live", "targets", "dashboard", "portfolio", "trades", "history", "discovery", "report", "settings"];

function initialTab(): Tab {
  const saved = localStorage.getItem("quorum.activeTab") as Tab | null;
  return saved && TAB_IDS.includes(saved) ? saved : "live";
}

function AppShell(_props: { accountId: number }) {
  const [tab, setTab] = createSignal<Tab>(initialTab());
  const [symbols, setSymbols] = createSignal<string[]>([]);
  const [current, setCurrent] = createSignal<Analysis | null>(null);
  const [analyzing, setAnalyzing] = createSignal<string | null>(null);
  const [connected, setConnected] = createSignal(false);
  const [aiOk, setAiOk] = createSignal(false);
  const [showModal, setShowModal] = createSignal(false);
  const [bitkubStatus, setBitkubStatus] = createSignal<BrokerCredentialStatus | null>(null);
  const [discovery, setDiscovery] = createSignal<MarketScanItem[]>([]);
  const [toast, setToast] = createSignal<string | null>(null);
  const [governor, setGovernor] = createSignal<GovernorState | null>(null);
  const [pauseBusy, setPauseBusy] = createSignal(false);
  const [progress, setProgress] = createSignal<{ pct: number; label: string; stage: string } | null>(null);
  const [thinking, setThinking] = createSignal("");
  let thinkEl: HTMLDivElement | undefined;
  // auto-scroll the thinking box to the bottom as new text arrives
  createEffect(() => {
    thinking();
    if (thinkEl) thinkEl.scrollTop = thinkEl.scrollHeight;
  });

  const acct = () => activeAccount();
  const isLive = () => acct()?.kind === "live";

  async function refreshGovernor() { setGovernor(await api.governor().catch(() => null)); }
  async function togglePause() {
    const g = governor();
    if (!g) return;
    setPauseBusy(true);
    try { await api.setPause(!g.paused); await refreshGovernor(); }
    finally { setPauseBusy(false); }
  }

  async function refreshWatch() { setSymbols((await api.getWatch()).symbols); }
  async function loadLatestAnalysis() {
    const rows = await api.recentDecisions(1).catch(() => []);
    if (!rows.length) return;
    const latest = await api.decisionAnalysis(rows[0].id).catch(() => null);
    if (latest) setCurrent(latest);
  }
  async function pushWatch(next: string[]) {
    const uniq = [...new Set(next.map((s) => s.toUpperCase()))];
    setSymbols(uniq);
    await api.setWatch(uniq);
  }
  function flash(msg: string) {
    setToast(msg);
    setTimeout(() => setToast(null), 4000);
  }

  function onLive(e: LiveEvent) {
    if (e.type === "status") {
      setConnected(e.healthy);
      if (e.message && e.message !== "connected") flash(e.message);
    } else if (e.type === "analyzing") {
      setAnalyzing(e.symbol);
      setThinking("");
      setProgress({ pct: 0, label: "Starting…", stage: "data" });
    } else if (e.type === "progress") {
      setProgress({ pct: e.pct, label: e.title, stage: e.stage });
      if (e.thinking) setThinking((prev) => prev + e.thinking);
    } else if (e.type === "decision") {
      setAnalyzing(null);
      setProgress(null);
      setCurrent(e.analysis);
    } else if (e.type === "trade") {
      const tr = e.trade as TradeRecord;
      flash(`💱 ${tr.side} ${tr.symbol} ${tr.status === "filled" ? "✓" : "✗"} (${tr.mode})`);
      refreshGovernor();
    } else if (e.type === "governor") {
      setGovernor(e.governor);
    } else if (e.type === "discovery") {
      setDiscovery(e.items);
    } else if (e.type === "alert") {
      const icon = e.alert.level === "error" ? "🚨" : e.alert.level === "warn" ? "⚠️" : "ℹ️";
      flash(`${icon} ${e.alert.message}`);
    }
  }

  createEffect(() => localStorage.setItem("quorum.activeTab", tab()));

  let stopLive: (() => void) | undefined;
  onCleanup(() => stopLive?.());

  onMount(async () => {
    try { const h = await api.health(); setAiOk(h.ai_engine); } catch {}
    try { const c = await api.credentialsStatus("bitkub"); setBitkubStatus(c); if (!c.configured && isLive()) setShowModal(true); } catch {}
    await refreshWatch().catch(() => {});
    await loadLatestAnalysis().catch(() => {});
    await refreshGovernor().catch(() => {});
    stopLive = connectLive(onLive);
  });

  async function analyzeNow(sym: string) {
    setTab("live");
    setAnalyzing(sym);
    try {
      const rec = await api.analyze(sym);
      const analysis = await api.decisionAnalysis(rec.id).catch(() => null);
      if (analysis) setCurrent(analysis);
    } finally { setAnalyzing(null); }
  }

  async function credentialsSaved() {
    setShowModal(false);
    flash(t("cred.saved"));
    try { setBitkubStatus(await api.credentialsStatus("bitkub")); } catch {}
  }

  async function manualTrade(sym: string, side: "BUY" | "SELL", amount: number) {
    try {
      const tr = await api.trade(sym, side, amount);
      flash(`💱 ${side} ${sym} ${tr.status} @ ${tr.price}`);
    } catch (e) {
      flash("Trade failed: " + e);
    }
  }

  return (
    <div class="min-h-screen overflow-x-hidden bg-slate-950 text-slate-100">
      <CredentialsModal open={showModal()} onClose={() => setShowModal(false)} onSaved={credentialsSaved} />

      <Show when={toast()}>
        <div class="fixed bottom-4 right-4 z-50 rounded-xl border border-slate-700 bg-slate-800 px-4 py-3 text-sm shadow-2xl">{toast()}</div>
      </Show>

      <header class="border-b border-slate-800 bg-slate-900/60 backdrop-blur">
        <div class="mx-auto flex max-w-6xl flex-wrap items-center gap-x-3 gap-y-2 px-3 py-3 sm:px-6">
          <h1 class="text-xl font-extrabold">⚖️ Quorum</h1>
          <span class={`rounded-full px-2 py-0.5 text-xs font-semibold ${isLive() ? "bg-red-900 text-red-300" : "bg-slate-700 text-slate-300"}`}>
            {acct()?.kind ?? "paper"}
          </span>
          <AccountSwitcher onSwitch={() => { /* keyed Show remounts shell */ }} />
          <div class="ml-auto flex flex-wrap items-center gap-2 text-xs sm:gap-3">
            <span class="flex items-center gap-1"><span class={`h-2 w-2 rounded-full ${connected() ? "bg-emerald-500" : "bg-slate-600"}`} />live</span>
            <span class="flex items-center gap-1"><span class={`h-2 w-2 rounded-full ${aiOk() ? "bg-emerald-500" : "bg-red-500"}`} />AI</span>
            <button class="rounded-lg border border-slate-700 px-2 py-1 font-semibold hover:bg-slate-800" onClick={toggleLang}>
              {lang() === "th" ? "EN" : "TH"}
            </button>
            <button class="rounded-lg border border-slate-700 px-3 py-1 hover:bg-slate-800" onClick={() => setShowModal(true)}>{t("hdr.apiSettings")}</button>
            <button class="rounded-lg border border-slate-700 px-3 py-1 hover:bg-slate-800" classList={{ "bg-sky-600 text-white": tab() === "profile" }} onClick={() => setTab("profile")}>👤</button>
          </div>
        </div>
        <nav class="mx-auto flex max-w-6xl gap-1 overflow-x-auto px-3 pb-2 sm:px-4">
          <For each={TAB_IDS}>
            {(id) => (
              <button
                class="whitespace-nowrap rounded-lg px-3 py-1.5 text-sm font-medium"
                classList={{ "bg-sky-600 text-white": tab() === id, "text-slate-400 hover:bg-slate-800": tab() !== id }}
                onClick={() => setTab(id)}
              >
                {t(`tab.${id}`)}
              </button>
            )}
          </For>
        </nav>
      </header>

      <GovernorBar gov={governor()} onTogglePause={togglePause} busy={pauseBusy()} />

      <Show when={progress()} fallback={
        <Show when={analyzing()}>
          <div class="mx-auto mt-2 max-w-6xl bg-sky-950/60 px-3 py-2 sm:px-6 text-center text-sm text-sky-300">
            🔎 {t("live.analyzing")} <b>{analyzing()}</b> {t("live.analyzingTail")}
          </div>
        </Show>
      }>
        {(p) => (
          <div class="mx-auto mt-2 max-w-6xl px-3 sm:px-6">
            <div class="rounded-xl border border-sky-900/60 bg-sky-950/30 p-3">
              <div class="flex items-center gap-2 text-sm">
                <span class="animate-pulse">🔎</span>
                <b class="text-sky-200">{analyzing()}</b>
                <span class="text-sky-400/80">· {p().label}</span>
                <span class="ml-auto font-mono text-xs font-semibold text-sky-300">{p().pct}%</span>
              </div>
              <div class="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-slate-800">
                <div
                  class="h-full rounded-full bg-gradient-to-r from-sky-500 to-violet-500 transition-all duration-300 ease-out"
                  style={{ width: `${p().pct}%` }}
                />
              </div>
              <Show when={thinking()}>
                <div class="mt-2">
                  <div class="mb-1 text-[11px] font-semibold uppercase tracking-wide text-slate-500">💭 AI thinking…</div>
                  <div
                    ref={(el) => (thinkEl = el)}
                    class="max-h-44 overflow-y-auto whitespace-pre-wrap rounded-lg border border-slate-800 bg-slate-950/70 p-2.5 text-xs leading-relaxed text-slate-400"
                  >
                    {thinking()}
                  </div>
                </div>
              </Show>
            </div>
          </div>
        )}
      </Show>

      <main class="mx-auto max-w-6xl px-3 py-6 sm:px-6">
        <Show when={tab() === "live"}>
          <div class="space-y-4">
            <SymbolSearch onPick={analyzeNow} onAdd={(s) => pushWatch([...symbols(), s])} />
            <WatchList symbols={symbols()} current={current()?.symbol ?? analyzing()} onSetSymbols={pushWatch} onAnalyze={analyzeNow} />
            <Show when={current()} fallback={<Empty />} keyed>
              {(a) => (
                <>
                  <div class="flex flex-wrap items-center gap-3 text-sm text-slate-400">
                    <span>{a.symbol}/{a.quote} · price {a.last_price ?? "—"} · {a.data_source}{a.synthetic ? " (simulated)" : ""} · news {a.news_count} · web {a.web_count}</span>
                    <span class="ml-auto flex items-center gap-2">
                      <button class="rounded-lg bg-emerald-600 px-3 py-1 text-xs font-semibold text-white hover:bg-emerald-500" onClick={() => manualTrade(a.symbol, "BUY", 1000)}>{t("live.buy")} 1,000</button>
                      <button class="rounded-lg bg-red-600 px-3 py-1 text-xs font-semibold text-white hover:bg-red-500" onClick={() => manualTrade(a.symbol, "SELL", 1000)}>{t("live.sell")} 1,000</button>
                    </span>
                  </div>
                  <ConsensusView a={a} />
                  <div class="grid gap-4 lg:grid-cols-2">
                    <AgentVotesPanel votes={a.consensus.votes} />
                    <ReasoningTrace steps={a.trace} thinking={a.verdict.thinking} />
                  </div>
                </>
              )}
            </Show>
          </div>
        </Show>

        <Show when={tab() === "targets"}><TrackingPanel onAnalyze={analyzeNow} /></Show>
        <Show when={tab() === "dashboard"}><DashboardPanel /></Show>
        <Show when={tab() === "history"}><HistoryView /></Show>
        <Show when={tab() === "trades"}><TradesView /></Show>
        <Show when={tab() === "portfolio"}><PortfolioPanel mode={acct()?.kind ?? "paper"} /></Show>
        <Show when={tab() === "discovery"}>
          <DiscoveryPanel items={discovery()} onAdd={(s) => pushWatch([...symbols(), s])} onAnalyze={analyzeNow} />
        </Show>
        <Show when={tab() === "report"}><ReportView /></Show>
        <Show when={tab() === "settings"}><SettingsPanel onSaved={() => {}} bitkubStatus={bitkubStatus()} onManageBroker={() => setShowModal(true)} /></Show>
        <Show when={tab() === "profile"}><ProfilePanel /></Show>
      </main>

      {/* ─── Branding footer ────────────────────────────────────────── */}
      <footer class="mt-16 border-t border-slate-800/60 pb-8 pt-6 text-center">
        <p class="text-xs tracking-wide text-slate-600">
          AI consensus trading system &nbsp;·&nbsp; analyze together, decide together
        </p>
        <p class="mt-1 text-xs text-slate-700">
          Crafted with{" "}
          <span class="bg-gradient-to-r from-violet-500 to-sky-400 bg-clip-text text-transparent font-semibold">
            Claude
          </span>
          {" "}·{" "}
          <span class="font-semibold text-slate-500">Quorum</span>
          {" "}·{" "}
          <a
            href="/api/about"
            target="_blank"
            rel="noopener"
            class="text-slate-700 underline underline-offset-2 hover:text-slate-400 transition-colors"
          >
            about
          </a>
        </p>
      </footer>
    </div>
  );
}

function Empty() {
  return (
    <div class="rounded-2xl border border-dashed border-slate-800 p-10 text-center text-slate-500">
      {t("live.empty")}
    </div>
  );
}
