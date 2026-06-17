import { For, Show, createSignal } from "solid-js";
import { api } from "../api";
import { accounts, activeAccountId, setActiveAccountId, setAccounts2 } from "../session";
import { t } from "../i18n";

/** Switch account (paper/live) — each account's transactions are fully isolated */
export default function AccountSwitcher(props: { onSwitch: (id: number) => void }) {
  const [open, setOpen] = createSignal(false);
  const [adding, setAdding] = createSignal(false);
  const [busy, setBusy] = createSignal<number | null>(null);

  const cur = () => accounts().find((a) => a.id === activeAccountId());

  function pick(id: number) {
    setActiveAccountId(id);
    setOpen(false);
    props.onSwitch(id);
  }

  async function addAccount(kind: "paper" | "live") {
    setAdding(true);
    try {
      const existing = accounts().filter((a) => a.kind === kind).length;
      const name = `${kind === "paper" ? "Paper" : "Live"} ${existing + 1}`;
      const acc = await api.createAccount(kind, name);
      setAccounts2([...accounts(), acc]);
      pick(acc.id);
    } catch (e) {
      alert("" + e);
    } finally {
      setAdding(false);
    }
  }

  async function removeAccount(e: MouseEvent, id: number, name: string) {
    e.stopPropagation(); // prevent triggering pick() on the row
    if (accounts().length <= 1) {
      alert(t("acctsw.lastWarn"));
      return;
    }
    if (!confirm(t("acctsw.confirmDel").replace("{name}", name))) return;
    setBusy(id);
    try {
      await api.deleteAccount(id);
      const remaining = accounts().filter((a) => a.id !== id);
      setAccounts2(remaining);
      // if the active account was deleted → switch to another account (keyed Show will remount itself)
      if (activeAccountId() === id && remaining[0]) {
        pick(remaining[0].id);
      }
    } catch (err) {
      alert("" + err);
    } finally {
      setBusy(null);
    }
  }

  return (
    <div class="relative">
      <button
        class="flex items-center gap-2 rounded-lg border border-slate-700 bg-slate-800 px-3 py-1 text-xs font-semibold hover:bg-slate-700"
        onClick={() => setOpen(!open())}
      >
        <span class={`h-2 w-2 rounded-full ${cur()?.kind === "live" ? "bg-red-500" : "bg-emerald-500"}`} />
        <span>{cur()?.name ?? "—"}</span>
        <span class="text-slate-500">▾</span>
      </button>

      <Show when={open()}>
        <div class="absolute right-0 z-50 mt-1 w-56 rounded-xl border border-slate-700 bg-slate-900 p-1 shadow-2xl">
          <div class="px-2 py-1 text-[11px] uppercase tracking-wide text-slate-500">{t("acctsw.title")}</div>
          <For each={accounts()}>
            {(a) => (
              <div
                class="flex w-full items-center gap-1 rounded-lg pr-1 text-sm hover:bg-slate-800"
                classList={{ "bg-slate-800": a.id === activeAccountId() }}
              >
                <button
                  class="flex min-w-0 flex-1 items-center gap-2 px-2 py-1.5 text-left"
                  onClick={() => pick(a.id)}
                >
                  <span class={`h-2 w-2 shrink-0 rounded-full ${a.kind === "live" ? "bg-red-500" : "bg-emerald-500"}`} />
                  <span class="flex-1 truncate">{a.name}</span>
                  <span class="text-[10px] uppercase text-slate-500">{a.kind}</span>
                </button>
                <Show when={accounts().length > 1}>
                  <button
                    class="shrink-0 rounded-md px-1.5 py-1 text-slate-500 hover:bg-red-950/60 hover:text-red-400 disabled:opacity-40"
                    title={t("acctsw.delete")}
                    disabled={busy() === a.id}
                    onClick={(e) => removeAccount(e, a.id, a.name)}
                  >
                    {busy() === a.id ? "…" : "🗑"}
                  </button>
                </Show>
              </div>
            )}
          </For>
          <div class="my-1 border-t border-slate-800" />
          <div class="flex gap-1 px-1 pb-1">
            <button
              class="flex-1 rounded-lg border border-slate-700 px-2 py-1 text-xs hover:bg-slate-800 disabled:opacity-50"
              disabled={adding()}
              onClick={() => addAccount("paper")}
            >
              + Paper
            </button>
            <button
              class="flex-1 rounded-lg border border-slate-700 px-2 py-1 text-xs hover:bg-slate-800 disabled:opacity-50"
              disabled={adding()}
              onClick={() => addAccount("live")}
            >
              + Live
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
}
