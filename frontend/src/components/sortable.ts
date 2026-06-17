import { createSignal, createMemo, type Accessor } from "solid-js";

/** Helper for sortable tables — click a column header to toggle ascending↔descending */
export function createSortable<T>(rows: Accessor<T[]>, initialKey?: keyof T) {
  const [key, setKey] = createSignal<keyof T | undefined>(initialKey);
  const [dir, setDir] = createSignal<1 | -1>(-1); // -1 = descending (default)

  function toggle(k: keyof T) {
    if (key() === k) setDir(dir() === 1 ? -1 : 1);
    else {
      setKey(() => k); // updater form: prevents symbol/number keys from being interpreted as setter fn
      setDir(-1);
    }
  }

  const sorted = createMemo(() => {
    const k = key();
    const arr = [...rows()];
    if (!k) return arr;
    const d = dir();
    return arr.sort((a, b) => {
      const av = a[k] as unknown;
      const bv = b[k] as unknown;
      if (typeof av === "number" && typeof bv === "number") return (av - bv) * d;
      return String(av).localeCompare(String(bv)) * d;
    });
  });

  /** Arrow indicator for column headers */
  const arrow = (k: keyof T) => (key() === k ? (dir() === 1 ? " ▲" : " ▼") : "");

  return { sorted, toggle, arrow, key, dir };
}
