// session.ts — central auth state (token, user, accounts, selected account)
// separated from api.ts to avoid circular imports — api.ts reads token/accountId from here

import { createSignal } from "solid-js";

export interface User {
  id: number;
  email: string;
  display_name: string;
  role: string;
}
export interface Account {
  id: number;
  user_id: number;
  kind: "paper" | "live";
  name: string;
  base_quote: string;
  created_at: string;
}

const TOKEN_KEY = "quorum.token";
const ACCT_KEY = "quorum.accountId";

const [token, setTokenSig] = createSignal<string | null>(localStorage.getItem(TOKEN_KEY));
const [user, setUser] = createSignal<User | null>(null);
const [accounts, setAccounts] = createSignal<Account[]>([]);
const [activeAccountId, setActiveAccountIdSig] = createSignal<number | null>(
  Number(localStorage.getItem(ACCT_KEY)) || null,
);

export { token, user, accounts, activeAccountId };

/** non-reactive read for headers in fetch */
export function getToken(): string | null {
  return token();
}
export function getAccountId(): number | null {
  return activeAccountId();
}

export function setActiveAccountId(id: number) {
  localStorage.setItem(ACCT_KEY, String(id));
  setActiveAccountIdSig(id);
}

export function activeAccount(): Account | undefined {
  return accounts().find((a) => a.id === activeAccountId());
}

/** set session after successful login/register */
export function setSession(tok: string, u: User, accs: Account[]) {
  localStorage.setItem(TOKEN_KEY, tok);
  setTokenSig(tok);
  setUser(u);
  setAccounts(accs);
  // select paper account as default if none selected or previous account no longer exists
  const cur = activeAccountId();
  if (!cur || !accs.some((a) => a.id === cur)) {
    const paper = accs.find((a) => a.kind === "paper") ?? accs[0];
    if (paper) setActiveAccountId(paper.id);
  }
}

export function updateUser(u: User) {
  setUser(u);
}
export function setAccounts2(accs: Account[]) {
  setAccounts(accs);
}

export function clearSession() {
  localStorage.removeItem(TOKEN_KEY);
  setTokenSig(null);
  setUser(null);
  setAccounts([]);
}
