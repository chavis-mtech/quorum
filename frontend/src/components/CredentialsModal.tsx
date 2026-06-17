import { createEffect, createSignal, Show } from "solid-js";
import { api, type BrokerCredentialStatus } from "../api";
import { t } from "../i18n";

const fmtWhen = (iso?: string | null) => (iso ? new Date(iso).toLocaleString() : "");

/** Modal for entering/replacing the Bitkub API key — clearly indicates whether a key is already set + masked preview */
export default function CredentialsModal(props: { open: boolean; onClose: () => void; onSaved: () => void }) {
  const [apiKey, setApiKey] = createSignal("");
  const [apiSecret, setApiSecret] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);
  const [status, setStatus] = createSignal<BrokerCredentialStatus | null>(null);
  const [loading, setLoading] = createSignal(false);

  const configured = () => !!status()?.configured;

  // Load current status + reset form every time the modal opens
  createEffect(() => {
    if (!props.open) return;
    setApiKey("");
    setApiSecret("");
    setErr(null);
    setLoading(true);
    api
      .credentialsStatus("bitkub")
      .then((s) => setStatus(s))
      .catch(() => setStatus(null))
      .finally(() => setLoading(false));
  });

  async function save() {
    setSaving(true);
    setErr(null);
    try {
      await api.setCredentials("bitkub", apiKey().trim(), apiSecret().trim());
      props.onSaved();
    } catch (e) {
      setErr(String(e));
    } finally {
      setSaving(false);
    }
  }

  const Row = (p: { label: string; value: string; mono?: boolean }) => (
    <div class="flex items-center gap-2">
      <dt class="w-20 shrink-0 text-slate-500">{p.label}</dt>
      <dd class={`truncate ${p.mono ? "font-mono text-slate-200" : "text-slate-400"}`}>{p.value}</dd>
    </div>
  );

  return (
    <Show when={props.open}>
      <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
        <div class="w-full max-w-md rounded-2xl border border-slate-700 bg-slate-900 p-6 shadow-2xl">
          <h2 class="text-lg font-bold text-slate-100">{t("cred.title")}</h2>
          <p class="mt-1 text-sm text-slate-400">{t("cred.desc")}</p>

          {/* Current status — whether a key is already set + masked preview */}
          <Show when={!loading()} fallback={<div class="mt-4 text-sm text-slate-500">{t("common.loading")}</div>}>
            <Show
              when={configured()}
              fallback={
                <div class="mt-4 flex items-center gap-2 rounded-lg border border-slate-700 bg-slate-800/50 px-3 py-2.5 text-sm">
                  <span class="h-2 w-2 rounded-full bg-slate-500" />
                  <span class="text-slate-300">{t("cred.statusOff")}</span>
                </div>
              }
            >
              <div class="mt-4 rounded-lg border border-emerald-800/60 bg-emerald-950/40 px-3 py-2.5">
                <div class="flex items-center gap-2">
                  <span class="h-2 w-2 rounded-full bg-emerald-400" />
                  <span class="text-sm font-semibold text-emerald-300">{t("cred.statusOn")}</span>
                </div>
                <dl class="mt-2 space-y-1 text-xs">
                  <Row label="API Key" value={status()?.api_key_hint || "••••"} mono />
                  <Show when={status()?.api_secret_hint}>
                    <Row label="Secret" value={status()!.api_secret_hint!} mono />
                  </Show>
                  <Show when={status()?.updated_at}>
                    <Row label={t("common.setOn")} value={fmtWhen(status()?.updated_at)} />
                  </Show>
                </dl>
                <p class="mt-2 text-[11px] leading-snug text-slate-500">{t("cred.stored")}</p>
              </div>
            </Show>
          </Show>

          {/* Form for entering new / replacing credentials */}
          <div class="mt-4">
            <Show when={configured()}>
              <div class="mb-1 text-xs font-semibold text-slate-300">{t("cred.replaceTitle")}</div>
              <p class="mb-2 text-[11px] leading-snug text-amber-300/80">{t("cred.replaceHint")}</p>
            </Show>

            <label class="block text-xs font-medium text-slate-400">API Key</label>
            <input
              class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500"
              value={apiKey()}
              onInput={(e) => setApiKey(e.currentTarget.value)}
              placeholder={configured() ? t("cred.replacePlaceholder") : "x-btk-apikey..."}
            />

            <label class="mt-3 block text-xs font-medium text-slate-400">API Secret</label>
            <input
              type="password"
              class="mt-1 w-full rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 text-sm text-slate-100 outline-none focus:border-sky-500"
              value={apiSecret()}
              onInput={(e) => setApiSecret(e.currentTarget.value)}
              placeholder="••••••••••••"
            />
          </div>

          <Show when={err()}>
            <p class="mt-3 rounded-lg bg-red-950 px-3 py-2 text-sm text-red-300">{err()}</p>
          </Show>

          <div class="mt-5 flex justify-end gap-2">
            <button
              class="rounded-lg px-4 py-2 text-sm text-slate-300 hover:bg-slate-800"
              onClick={props.onClose}
            >
              {configured() ? t("common.close") : t("cred.skip")}
            </button>
            <button
              class="rounded-lg bg-sky-600 px-4 py-2 text-sm font-semibold text-white hover:bg-sky-500 disabled:opacity-50"
              disabled={saving() || !apiKey() || !apiSecret()}
              onClick={save}
            >
              {saving() ? t("common.saving") : configured() ? t("cred.replaceBtn") : t("cred.saveConnect")}
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
}
