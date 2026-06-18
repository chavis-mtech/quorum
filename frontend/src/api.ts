// API client — communicates with Rust backend via HTTP + WebSocket
// dev: Vite proxy /api and /ws to backend (see vite.config.ts)
// uses QPACK binary for both REST (Accept header) and WS (?fmt=bin)
// → payload 40-60% smaller, less GC, lower latency

import { getToken, getAccountId, clearSession, type User, type Account } from "./session";
import { qpackDecode, QPACK_ACCEPT } from "./qpack";

export type { User, Account };
export type Action = "BUY" | "SELL" | "HOLD";

export interface AuthResult {
  token: string;
  user: User;
  accounts: Account[];
}
export interface MeResult {
  user: User;
  accounts: Account[];
  active_account_id: number;
  bitkub_configured: boolean;
}

export interface Vote {
  agent: string;
  action: Action;
  confidence: number;
  reasoning: string;
  veto: boolean;
  ok: boolean;
}

export interface Consensus {
  action: Action;
  confidence: number;
  agreement: number;
  voted: number;
  vetoed: boolean;
  passed_threshold: boolean;
  reasoning: string;
  votes: Vote[];
  regime?: string;
}

export interface Verdict {
  action: Action;
  confidence: number;
  reasoning: string;
  engine: string;
  suggested_size_pct: number;
  thinking: string;
  // trade plan
  thesis: string;
  entry_type: string;
  entry_price: number;
  target_price: number;
  stop_price: number;
  invalidation: string;
  next_step: string;
  // trend-gate scoring (delivered via the reasoning trace; optional on the parsed verdict)
  conviction?: number;       // 0..1 — likelihood the move continues in the calculated direction
  reversal_risk?: number;    // 0..1 — risk the next move turns against the action
  trend_dir?: string;        // up | down | sideways
  trend_gate?: string;       // aligned | reversal-confirmed | blocked | n/a
}

export interface TradePlan {
  id: number;
  symbol: string;
  quote: string;
  state: "pending" | "open" | "closed" | "cancelled";
  action: Action;
  entry_type: string;
  entry_price: number;
  target_price: number;
  stop_price: number;
  confidence: number;
  thesis: string;
  invalidation: string;
  next_step: string;
  decision_id: number | null;
  last_price: number;
  high_water_mark: number;
  initial_stop: number;
  trail_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface TraceStep {
  seq: number;
  stage: "data" | "news" | "web" | "agent" | "consensus" | "judge";
  title: string;
  detail: string;
  status: "done" | "warn" | "error" | "thinking";
  data: Record<string, any>;
  elapsed_ms: number;
}

export interface SymbolTicker {
  symbol: string;
  quote: string;
  last: number;
  change_24h_pct: number;
  high_24h: number;
  low_24h: number;
  volume_24h: number;
}

export interface Analysis {
  symbol: string;
  quote: string;
  last_price: number | null;
  mode: string;
  regime?: string;
  data_source: string;
  synthetic: boolean;
  news_source: string;
  news_count: number;
  web_source: string;
  web_count: number;
  consensus: Consensus;
  verdict: Verdict;
  trace: TraceStep[];
}

export interface DecisionRecord {
  id: number;
  symbol: string;
  quote: string;
  mode: string;
  final_action: Action;
  consensus_action: Action;
  consensus_confidence: number;
  agreement: number;
  voted: number;
  vetoed: boolean;
  judge_engine: string;
  judge_reasoning: string;
  last_price: number | null;
  executed: boolean;
  note: string;
  created_at: string;
}

export interface ReportSummary {
  total_decisions: number;
  executed: number;
  buy: number;
  sell: number;
  hold: number;
  vetoed: number;
  avg_confidence: number;
  symbols_tracked: number;
}

export interface TradingSettings {
  mode: "paper" | "live" | "signal-only";
  auto_trade: boolean;
  trade_amount_quote: number;
  max_position_pct: number;
  min_confidence: number;
  daily_loss_limit: number;
  max_open_positions: number;
  allow_sell: boolean;
  take_profit_pct: number;
  stop_loss_pct: number;
  discovery_enabled: boolean;
  discovery_top_n: number;
  paused: boolean;
  ai_judge_enabled: boolean;
  ai_judge_provider: AiProvider;
  ai_judge_model: string;
  ai_judge_ollama_url: string;
  ai_judge_base_url: string;
  ai_judge_thinking: boolean;
  broker: BrokerId;
  manage_style: ManageStyle;
  let_winners_run: boolean;
}

export type ManageStyle = "off" | "conservative" | "balanced" | "aggressive";

export type BrokerId = "bitkub" | "binance";

export type AiProvider = "ollama" | "openai" | "anthropic" | "groq" | "openrouter" | "custom" | "none";

export interface AiCredentialStatus {
  provider: AiProvider;
  needs_key: boolean;
  configured: boolean;
  api_key_hint?: string;
  updated_at?: string | null;
}

export interface BrokerCredentialStatus {
  broker: string;
  configured: boolean;
  api_key_hint?: string;
  api_secret_hint?: string;
  updated_at?: string | null;
}

export interface AiModelList {
  provider: AiProvider;
  source: "ollama" | "catalog";
  ok: boolean;
  models: string[];
}

export interface AiCompareLeg {
  label: "local" | "selected";
  provider: string;
  ok: boolean;
  elapsed_ms: number;
  engine?: string;
  action?: Action;
  confidence?: number;
  reasoning?: string;
  thesis?: string;
  next_step?: string;
  synthetic?: boolean;
  error?: string;
}

export interface AiCompareResult {
  symbol: string;
  provider_configured: boolean;
  local: AiCompareLeg;
  selected: AiCompareLeg;
}

export interface PaperPosition {
  symbol: string;
  amount_base: number;
  avg_price: number;
  last_price: number;
}
export interface WalletView {
  cash_quote: number;
  starting_cash: number;
  positions: PaperPosition[];
  positions_value: number;
  equity: number;
  pnl: number;
  pnl_pct: number;
  simulated: boolean;
}

export interface TradeRecord {
  id: number;
  decision_id: number | null;
  symbol: string;
  quote: string;
  side: Action;
  mode: string;
  simulated: boolean;
  amount_base: number;
  amount_quote: number;
  price: number;
  status: string;
  external_order_id: string;
  note: string;
  realized_pnl: number;
  created_at: string;
}

export interface TradeStats {
  session_start: string;
  total_trades: number;
  buys: number;
  closed: number;
  wins: number;
  losses: number;
  win_rate: number;
  gross_pnl: number;
  avg_win: number;
  avg_loss: number;
  best: number;
  worst: number;
  profit_factor: number;
}

export interface MarketScanItem {
  symbol: string;
  score: number;
  reason: string;
  last_price: number;
  change_24h: number;
}

export interface TargetStatus {
  symbol: string;
  state: "holding" | "plan_pending" | "candidate" | "waiting" | "skipped" | "queued";
  reason: string;
  last_price: number;
  entry_price: number;
  target_price: number;
  stop_price: number;
  confidence: number;
  action: string;
  decision_id: number | null;
  updated_at: string | null;
}

export interface GovernorState {
  account_id: number;
  state: "trading" | "scanning" | "full" | "halted" | "paused" | "manual" | "signal";
  reason: string;
  cash: number;
  equity: number;
  daily_pnl_pct: number;
  loss_limit: number;
  loss_used: number;
  open_positions: number;
  max_open_positions: number;
  open_slots: number;
  buys_remaining: number;
  trade_amount: number;
  auto_trade: boolean;
  paused: boolean;
  watch_capacity: number;
}

export interface Balance {
  asset: string;
  available: number;
}

export interface AlertRecord {
  id: number;
  account_id: number;
  level: "info" | "warn" | "error";
  code: string;
  message: string;
  created_at: string;
}

export type LiveEvent =
  | { type: "analyzing"; account_id: number; symbol: string }
  | { type: "decision"; record: DecisionRecord; analysis: Analysis }
  | { type: "trade"; trade: TradeRecord }
  | { type: "discovery"; items: MarketScanItem[] }
  | { type: "governor"; governor: GovernorState }
  | { type: "progress"; account_id: number; symbol: string; pct: number; stage: string; title: string; thinking?: string }
  | { type: "status"; message: string; healthy: boolean }
  | { type: "alert"; alert: AlertRecord };

async function jsonFetch<T>(url: string, init?: RequestInit): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    // always request binary QPACK — server falls back to JSON if unsupported (backward compat)
    "Accept": QPACK_ACCEPT + ", application/json",
    ...(init?.headers as Record<string, string> | undefined),
  };
  const tok = getToken();
  if (tok) headers["Authorization"] = `Bearer ${tok}`;
  const acct = getAccountId();
  if (acct) headers["X-Account-Id"] = String(acct);

  const res = await fetch(url, { ...init, headers });
  if (res.status === 401) {
    clearSession(); // token expired → redirect to login
    throw new Error("Session expired, please log in again");
  }
  if (!res.ok) {
    // error body may be JSON or QPACK — try parsing both
    const ct = res.headers.get("Content-Type") ?? "";
    let errMsg = res.statusText;
    try {
      if (ct.includes("application/x-qpack")) {
        const buf = await res.arrayBuffer();
        const obj = qpackDecode(buf) as Record<string, unknown>;
        errMsg = String(obj?.error ?? res.statusText);
      } else {
        const obj = await res.json();
        errMsg = String(obj?.error ?? res.statusText);
      }
    } catch { /* ignore */ }
    throw new Error(errMsg);
  }

  // Response decoding: QPACK binary or JSON
  const ct = res.headers.get("Content-Type") ?? "";
  if (ct.includes("application/x-qpack")) {
    const buf = await res.arrayBuffer();
    return qpackDecode(buf) as T;
  }
  return res.json();
}

export const api = {
  health: () => jsonFetch<{ ai_engine: boolean; broker: string }>("/api/health"),
  // ---- auth ----
  register: (email: string, password: string, display_name: string) =>
    jsonFetch<AuthResult>("/api/auth/register", {
      method: "POST",
      body: JSON.stringify({ email, password, display_name }),
    }),
  login: (email: string, password: string) =>
    jsonFetch<AuthResult>("/api/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
    }),
  me: () => jsonFetch<MeResult>("/api/auth/me"),
  updateProfile: (display_name: string) =>
    jsonFetch<{ ok: boolean }>("/api/auth/profile", {
      method: "PUT",
      body: JSON.stringify({ display_name }),
    }),
  updatePassword: (current_password: string, new_password: string) =>
    jsonFetch<{ ok: boolean }>("/api/auth/password", {
      method: "PUT",
      body: JSON.stringify({ current_password, new_password }),
    }),
  listAccounts: () => jsonFetch<Account[]>("/api/accounts"),
  createAccount: (kind: "paper" | "live", name: string) =>
    jsonFetch<Account>("/api/accounts", {
      method: "POST",
      body: JSON.stringify({ kind, name }),
    }),
  deleteAccount: (id: number) =>
    jsonFetch<{ ok: boolean }>(`/api/accounts/${id}`, { method: "DELETE" }),
  setPause: (paused: boolean) =>
    jsonFetch<{ ok: boolean; paused: boolean }>("/api/account/pause", {
      method: "POST",
      body: JSON.stringify({ paused }),
    }),
  credentialsStatus: (broker = "bitkub") =>
    jsonFetch<BrokerCredentialStatus>(`/api/credentials/status?broker=${broker}`),
  setCredentials: (broker: string, api_key: string, api_secret: string) =>
    jsonFetch<{ ok: boolean }>("/api/credentials", {
      method: "POST",
      body: JSON.stringify({ broker, api_key, api_secret }),
    }),
  getWatch: () => jsonFetch<{ symbols: string[] }>("/api/watch"),
  setWatch: (symbols: string[]) =>
    jsonFetch<{ symbols: string[] }>("/api/watch", {
      method: "POST",
      body: JSON.stringify({ symbols }),
    }),
  analyze: (symbol: string, judgeOverride?: Record<string, unknown>) =>
    jsonFetch<DecisionRecord>("/api/analyze", {
      method: "POST",
      body: JSON.stringify(judgeOverride ? { symbol, judge_override: judgeOverride } : { symbol }),
    }),
  searchSymbols: (q: string, limit = 12) =>
    jsonFetch<SymbolTicker[]>(`/api/symbols/search?q=${encodeURIComponent(q)}&limit=${limit}`),
  ticker: (symbol: string) => jsonFetch<SymbolTicker>(`/api/symbols/ticker/${symbol}`),
  recentDecisions: (limit = 50) => jsonFetch<DecisionRecord[]>(`/api/decisions?limit=${limit}`),
  decisionsForSymbol: (symbol: string, limit = 50) =>
    jsonFetch<DecisionRecord[]>(`/api/decisions/${symbol}?limit=${limit}`),
  decisionAnalysis: (id: number) => jsonFetch<Analysis>(`/api/decision/${id}/analysis`),
  report: () => jsonFetch<ReportSummary>("/api/report"),
  // settings
  getSettings: () => jsonFetch<TradingSettings>("/api/settings"),
  putSettings: (s: TradingSettings) =>
    jsonFetch<{ ok: boolean }>("/api/settings", { method: "PUT", body: JSON.stringify(s) }),
  aiCredentialsStatus: (provider: AiProvider) =>
    jsonFetch<AiCredentialStatus>(`/api/ai/credentials/status?provider=${encodeURIComponent(provider)}`),
  setAiCredentials: (provider: AiProvider, api_key: string) =>
    jsonFetch<{ ok: boolean; provider: AiProvider; configured: boolean }>("/api/ai/credentials", {
      method: "POST",
      body: JSON.stringify({ provider, api_key }),
    }),
  aiModels: (provider: AiProvider) =>
    jsonFetch<AiModelList>(`/api/ai/models?provider=${encodeURIComponent(provider)}`),
  compareAi: (symbol: string) =>
    jsonFetch<AiCompareResult>("/api/ai/compare", {
      method: "POST",
      body: JSON.stringify({ symbol }),
    }),
  // wallet (paper)
  getWallet: () => jsonFetch<WalletView>("/api/wallet"),
  resetWallet: (starting_cash: number) =>
    jsonFetch<{ ok: boolean }>("/api/wallet/reset", { method: "POST", body: JSON.stringify({ starting_cash }) }),
  // account (live)
  accountBalance: () => jsonFetch<{ broker: string; balances: Balance[] }>("/api/account/balance"),
  // trades
  recentTrades: (limit = 100) => jsonFetch<TradeRecord[]>(`/api/trades?limit=${limit}`),
  clearTrades: () => jsonFetch<{ ok: boolean; deleted: number }>("/api/trades", { method: "DELETE" }),
  trade: (symbol: string, side: Action, amount_quote: number, mode?: string) =>
    jsonFetch<TradeRecord>("/api/trade", {
      method: "POST",
      body: JSON.stringify({ symbol, side, amount_quote, mode }),
    }),
  // discovery
  marketScan: (top_n = 8) => jsonFetch<MarketScanItem[]>(`/api/market/scan?top_n=${top_n}`),
  // stats (win/loss)
  stats: () => jsonFetch<TradeStats>("/api/stats"),
  resetStats: () => jsonFetch<{ ok: boolean }>("/api/stats/reset", { method: "POST" }),
  // trade plans
  plans: () => jsonFetch<TradePlan[]>("/api/plans"),

  // alerts — events the user should know about
  alerts: () => jsonFetch<AlertRecord[]>("/api/alerts"),
  // governor (capital / risk state)
  governor: () => jsonFetch<GovernorState>("/api/governor"),
  // targets pipeline (what AI is tracking + why)
  targets: () => jsonFetch<TargetStatus[]>("/api/targets"),
};

/** Connect WebSocket to receive LiveEvent — auto-reconnect, binary QPACK by default
 *
 *  ?fmt=bin → server sends Message::Binary(QPACK bytes) instead of JSON text
 *  saves 40-60% bandwidth on the hot path (events every 5-30 seconds)
 *  ?fmt=json → debug mode (add &fmt=json to URL and inspect in DevTools)
 */
export function connectLive(onEvent: (e: LiveEvent) => void): () => void {
  let ws: WebSocket | null = null;
  let closed = false;
  let retry: number | undefined;

  const open = () => {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const tok = getToken() ?? "";
    const acct = getAccountId() ?? "";
    // send fmt=bin to receive binary QPACK; add &fmt=json for debug
    ws = new WebSocket(
      `${proto}://${location.host}/ws?token=${encodeURIComponent(tok)}&account_id=${acct}&fmt=bin`
    );
    ws.binaryType = "arraybuffer"; // receive binary as ArrayBuffer, not Blob
    ws.onmessage = (ev) => {
      try {
        let event: LiveEvent;
        if (ev.data instanceof ArrayBuffer) {
          // QPACK binary — decode immediately
          event = qpackDecode(ev.data) as LiveEvent;
        } else {
          // JSON text (fallback or debug mode)
          event = JSON.parse(ev.data as string);
        }
        onEvent(event);
      } catch {
        /* ignore parse errors */
      }
    };
    ws.onclose = () => {
      if (!closed) retry = window.setTimeout(open, 2000);
    };
  };
  open();

  return () => {
    closed = true;
    if (retry) clearTimeout(retry);
    ws?.close();
  };
}
