import { createSignal, Show } from "solid-js";
import { api } from "../api";
import { setSession } from "../session";
import { t, lang, toggleLang } from "../i18n";

/** Login / registration page — entry gate before the app */
export default function LoginView() {
  const [mode, setMode] = createSignal<"login" | "register">("login");
  const [email, setEmail] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [name, setName] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [err, setErr] = createSignal("");

  async function submit(e: Event) {
    e.preventDefault();
    setErr("");
    setBusy(true);
    try {
      const r =
        mode() === "login"
          ? await api.login(email().trim(), password())
          : await api.register(email().trim(), password(), name().trim());
      setSession(r.token, r.user, r.accounts);
    } catch (e) {
      setErr("" + (e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div class="flex min-h-screen items-center justify-center bg-slate-950 px-4 text-slate-100">
      <div class="w-full max-w-md">
        <div class="mb-6 text-center">
          <div class="text-4xl font-extrabold">⚖️ Quorum</div>
          <div class="mt-1 text-sm text-slate-400">{t("auth.tagline")}</div>
        </div>

        <div class="rounded-2xl border border-slate-800 bg-slate-900 p-6 shadow-2xl">
          <div class="mb-4 flex overflow-hidden rounded-xl border border-slate-700">
            <button
              class="flex-1 py-2 text-sm font-semibold"
              classList={{ "bg-sky-600 text-white": mode() === "login", "text-slate-400": mode() !== "login" }}
              onClick={() => setMode("login")}
            >
              {t("auth.login")}
            </button>
            <button
              class="flex-1 py-2 text-sm font-semibold"
              classList={{ "bg-sky-600 text-white": mode() === "register", "text-slate-400": mode() !== "register" }}
              onClick={() => setMode("register")}
            >
              {t("auth.register")}
            </button>
          </div>

          <form class="space-y-3" onSubmit={submit}>
            <Show when={mode() === "register"}>
              <Field label={t("auth.name")} value={name()} onIn={setName} placeholder="e.g. Mark" />
            </Show>
            <Field label={t("auth.email")} value={email()} onIn={setEmail} type="email" placeholder="you@email.com" />
            <Field label={t("auth.password")} value={password()} onIn={setPassword} type="password" placeholder="••••••" />

            <Show when={err()}>
              <div class="rounded-lg bg-red-950 px-3 py-2 text-sm text-red-300">{err()}</div>
            </Show>

            <button
              type="submit"
              class="w-full rounded-xl bg-sky-600 py-2.5 text-sm font-bold text-white hover:bg-sky-500 disabled:opacity-50"
              disabled={busy()}
            >
              {busy() ? t("common.loading") : mode() === "login" ? t("auth.loginBtn") : t("auth.registerBtn")}
            </button>
          </form>

          <Show when={mode() === "register"}>
            <div class="mt-3 text-center text-xs text-slate-600">{t("auth.registerHint")}</div>
          </Show>
        </div>

        <div class="mt-4 flex items-center justify-center gap-3 text-xs text-slate-500">
          <button class="rounded-lg border border-slate-700 px-2 py-1 hover:bg-slate-800" onClick={toggleLang}>
            {lang() === "th" ? "EN" : "TH"}
          </button>
          <span>{t("auth.demo")}</span>
        </div>
      </div>
    </div>
  );
}

function Field(p: {
  label: string;
  value: string;
  onIn: (v: string) => void;
  type?: string;
  placeholder?: string;
}) {
  return (
    <label class="block">
      <span class="text-xs text-slate-400">{p.label}</span>
      <input
        class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500"
        type={p.type ?? "text"}
        value={p.value}
        placeholder={p.placeholder}
        onInput={(e) => p.onIn(e.currentTarget.value)}
      />
    </label>
  );
}
