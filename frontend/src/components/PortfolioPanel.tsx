import { Show } from "solid-js";
import WalletPanel from "./WalletPanel";
import AccountPanel from "./AccountPanel";
import { t } from "../i18n";

/** Combined portfolio — paper mode shows the simulated wallet, live mode shows the real Bitkub balance */
export default function PortfolioPanel(props: { mode: string }) {
  const isLive = () => props.mode === "live";
  return (
    <div class="space-y-3">
      <div class="flex items-center gap-2">
        <span class="text-sm font-bold text-slate-200">{isLive() ? t("port.live") : t("port.paper")}</span>
        <span class={`rounded-full px-2 py-0.5 text-xs font-semibold ${isLive() ? "bg-red-900 text-red-300" : "bg-slate-700 text-slate-300"}`}>
          {props.mode}
        </span>
        <span class="text-xs text-slate-500">{t("port.switchHint")}</span>
      </div>
      <Show when={isLive()} fallback={<WalletPanel />}>
        <AccountPanel />
      </Show>
    </div>
  );
}
