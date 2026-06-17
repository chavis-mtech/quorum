import { createSignal, Show } from "solid-js";
import { api } from "../api";
import { user, updateUser, clearSession } from "../session";
import { t } from "../i18n";

/** Profile page — edit display name, change password, log out */
export default function ProfilePanel() {
  const [name, setName] = createSignal(user()?.display_name ?? "");
  const [cur, setCur] = createSignal("");
  const [next, setNext] = createSignal("");
  const [msg, setMsg] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  async function saveName() {
    setBusy(true);
    setMsg("");
    try {
      await api.updateProfile(name().trim());
      const u = user();
      if (u) updateUser({ ...u, display_name: name().trim() });
      setMsg(t("set.saved"));
    } catch (e) {
      setMsg("" + e);
    } finally {
      setBusy(false);
    }
  }

  async function savePassword() {
    setBusy(true);
    setMsg("");
    try {
      await api.updatePassword(cur(), next());
      setCur("");
      setNext("");
      setMsg(t("profile.pwChanged"));
    } catch (e) {
      setMsg("" + e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div class="mx-auto max-w-xl space-y-4">
      <div class="rounded-2xl border border-slate-800 bg-slate-900 p-5">
        <h3 class="text-sm font-bold text-slate-200">{t("profile.title")}</h3>
        <div class="mt-3 grid gap-1 text-sm text-slate-400">
          <div>{t("profile.email")}: <span class="text-slate-200">{user()?.email}</span></div>
          <div>{t("profile.role")}: <span class="text-slate-200">{user()?.role}</span></div>
        </div>
        <label class="mt-4 block">
          <span class="text-xs text-slate-400">{t("auth.name")}</span>
          <input
            class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500"
            value={name()}
            onInput={(e) => setName(e.currentTarget.value)}
          />
        </label>
        <button
          class="mt-3 rounded-lg bg-sky-600 px-4 py-2 text-sm font-semibold text-white hover:bg-sky-500 disabled:opacity-50"
          disabled={busy()}
          onClick={saveName}
        >
          {t("common.save")}
        </button>
      </div>

      <div class="rounded-2xl border border-slate-800 bg-slate-900 p-5">
        <h3 class="text-sm font-bold text-slate-200">{t("profile.changePw")}</h3>
        <label class="mt-3 block">
          <span class="text-xs text-slate-400">{t("profile.curPw")}</span>
          <input type="password" class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm" value={cur()} onInput={(e) => setCur(e.currentTarget.value)} />
        </label>
        <label class="mt-2 block">
          <span class="text-xs text-slate-400">{t("profile.newPw")}</span>
          <input type="password" class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm" value={next()} onInput={(e) => setNext(e.currentTarget.value)} />
        </label>
        <button
          class="mt-3 rounded-lg border border-slate-600 px-4 py-2 text-sm font-semibold text-slate-200 hover:bg-slate-800 disabled:opacity-50"
          disabled={busy() || !cur() || !next()}
          onClick={savePassword}
        >
          {t("profile.changePw")}
        </button>
      </div>

      <Show when={msg()}>
        <div class="text-sm text-slate-400">{msg()}</div>
      </Show>

      <button
        class="rounded-lg bg-red-900/60 px-4 py-2 text-sm font-semibold text-red-200 hover:bg-red-900"
        onClick={clearSession}
      >
        {t("profile.logout")}
      </button>
    </div>
  );
}
