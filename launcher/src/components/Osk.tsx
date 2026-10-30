// On-screen keyboard, driven by the controller: the d-pad moves, A types, X deletes,
// Y toggles shift, LB/RB move the cursor, Start finishes, B cancels.

import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { usePadHandler } from "../nav/input";
import { Glyph, Hint } from "../ui/controls";
import type { PadButton } from "../lib/types";
import { isTauri } from "../lib/bridge";

interface OskRequest {
  title: string;
  value: string;
  placeholder?: string;
  secret?: boolean;
  /** A live line under the text (debounced). `ok: false` keeps Done from finishing. */
  status?: (value: string) => Promise<OskStatus | null> | OskStatus | null;
  resolve: (v: string | null) => void;
}

const useOsk = create<{ req: OskRequest | null }>(() => ({ req: null }));

export interface OskStatus {
  text: string;
  ok: boolean;
}

/** Opens the keyboard and resolves with the text, or null when cancelled. */
export function askText(title: string, value = "", opts: { placeholder?: string; secret?: boolean; status?: OskRequest["status"] } = {}): Promise<string | null> {
  return new Promise((resolve) => useOsk.setState({ req: { title, value, resolve, ...opts } }));
}

/**
 * RC (sysbuttons.rs) with the launcher in front: the keyboard for the focused text field, or
 * closed when it is already up. The text goes back into the field as typing would.
 */
export function toggleOsk() {
  const open = useOsk.getState().req;
  if (open) {
    open.resolve(null);
    useOsk.setState({ req: null });
    return;
  }
  const el = document.activeElement;
  if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) || el.readOnly || el.disabled) return;
  const label = el.getAttribute("aria-label") || el.placeholder || "Type";
  void askText(label, el.value, { placeholder: el.placeholder || undefined, secret: el instanceof HTMLInputElement && el.type === "password" }).then((v) => {
    if (v == null) return;
    // React-controlled fields: set through the native setter so onChange sees an input event.
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")?.set?.call(el, v);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.focus();
  });
}

let oskListening = false;
/** Listens for RC from the native side (once). */
export function listenOskToggle() {
  if (oskListening || !isTauri) return;
  oskListening = true;
  void import("@tauri-apps/api/event").then(({ listen }) => listen("osk-toggle", () => toggleOsk())).catch(() => {});
}

const ROWS_LOWER = [
  ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
  ["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"],
  ["a", "s", "d", "f", "g", "h", "j", "k", "l", "@"],
  ["z", "x", "c", "v", "b", "n", "m", ".", "-", "_"],
];
const ROWS_UPPER = [
  ["!", "?", "#", "$", "%", "&", "*", "(", ")", "/"],
  ["Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P"],
  ["A", "S", "D", "F", "G", "H", "J", "K", "L", ":"],
  ["Z", "X", "C", "V", "B", "N", "M", ",", "'", "+"],
];
const SPECIAL = ["shift", "space", "paste", "del", "done"] as const;

export function Osk() {
  const req = useOsk((s) => s.req);
  useEffect(() => listenOskToggle(), []);
  const [text, setText] = useState("");
  const [shift, setShift] = useState(false);
  const [pos, setPos] = useState<[number, number]>([1, 0]);
  const restore = useRef<HTMLElement | null>(null);
  const [note, setNote] = useState<OskStatus | null>(null);

  useEffect(() => {
    if (req) {
      setText(req.value);
      setPos([1, 0]);
      restore.current = document.activeElement as HTMLElement;
    }
  }, [req]);

  // The live line: wait for a pause in typing, and drop answers for text that has moved on.
  useEffect(() => {
    if (!req?.status) return;
    setNote(null);
    let gone = false;
    const t = setTimeout(() => {
      void Promise.resolve(req.status!(text))
        .then((n) => !gone && setNote(n))
        .catch(() => undefined);
    }, 300);
    return () => {
      gone = true;
      clearTimeout(t);
    };
  }, [req, text]);

  const rows = shift ? ROWS_UPPER : ROWS_LOWER;
  const finish = (v: string | null) => {
    req?.resolve(v);
    useOsk.setState({ req: null });
    setTimeout(() => restore.current?.focus({ preventScroll: true }), 0);
  };
  const done = () => {
    // With a live check, Done waits for a name that passes (empty text may always finish).
    if (req?.status && text.trim() && !note?.ok) return;
    finish(text);
  };
  const press = (k: string) => {
    if (k === "shift") setShift((s) => !s);
    else if (k === "space") setText((t) => t + " ");
    else if (k === "del") setText((t) => t.slice(0, -1));
    else if (k === "done") done();
    else if (k === "paste") void navigator.clipboard?.readText().then((c) => c && setText((t) => t + c.trim())).catch(() => {});
    else {
      setText((t) => t + k);
      if (shift && /[A-Z]/.test(k)) setShift(false);
    }
  };

  usePadHandler(
    (b) => {
      const [r, c] = pos;
      const lastRow = rows.length;
      const cols = (row: number) => (row === lastRow ? SPECIAL.length : 10);
      switch (b) {
        case "up":
          setPos([Math.max(0, r - 1), r - 1 === lastRow ? Math.min(c, SPECIAL.length - 1) : r === lastRow ? Math.min(9, Math.round((c / (SPECIAL.length - 1)) * 9)) : c]);
          return true;
        case "down":
          setPos(r + 1 === lastRow ? [lastRow, Math.min(SPECIAL.length - 1, Math.round((c / 9) * (SPECIAL.length - 1)))] : [Math.min(lastRow, r + 1), c]);
          return true;
        case "left":
          setPos([r, (c - 1 + cols(r)) % cols(r)]);
          return true;
        case "right":
          setPos([r, (c + 1) % cols(r)]);
          return true;
        case "a":
          press(r === lastRow ? SPECIAL[c] : rows[r][c]);
          return true;
        case "x":
          press("del");
          return true;
        case "y":
          press("shift");
          return true;
        case "start":
          done();
          return true;
        case "b":
          finish(null);
          return true;
        default:
          return true;
      }
    },
    !!req,
  );

  useEffect(() => {
    if (!req) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key.length === 1 && !e.ctrlKey && !e.metaKey) {
        e.stopPropagation();
        setText((t) => t + e.key);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [req]);

  if (!req) return null;
  const shown = req.secret ? "•".repeat(text.length) : text;
  // Keys that have a button of their own show its glyph.
  const keyGlyph: Partial<Record<(typeof SPECIAL)[number], PadButton>> = { shift: "y", del: "x", done: "start" };
  const label = (k: (typeof SPECIAL)[number]) => (k === "shift" ? (shift ? "abc" : "Shift") : k === "del" ? "Delete" : k === "done" ? "Done" : k === "paste" ? "Paste" : "Space");
  return (
    <div className="fade fixed inset-0 z-[60] flex flex-col justify-end bg-black/55" data-nav-scope>
      <div className="slide-up rounded-t-sheet border-t border-border-light bg-l1 px-[2.5rem] pb-[1.1rem] pt-[1.2rem] shadow-[0_-1rem_3rem_rgb(0_0_0/0.5)]">
        <div className="mx-auto w-full max-w-[64rem]">
          <div className="mb-[0.5rem] text-small text-muted">{req.title}</div>
          <div className="mb-[1rem] flex h-[3.25rem] items-center rounded-card border border-border-light bg-bg px-[1rem] text-row">
            {shown ? <span className="num truncate">{shown}</span> : <span className="text-muted/70">{req.placeholder}</span>}
            <span className="ml-[1px] h-[1.5rem] w-[2px] animate-pulse bg-fg" />
          </div>
          {req.status && <div className={`-mt-[0.5rem] mb-[0.75rem] h-[1.25rem] text-small ${note?.ok ? "text-fg/85" : "text-muted"}`}>{note?.text ?? (text.trim() ? "Checking" : "")}</div>}
          <div className="flex flex-col gap-[0.45rem]">
            {rows.map((row, ri) => (
              <div key={ri} className="flex gap-[0.45rem]">
                {row.map((k, ci) => {
                  const on = pos[0] === ri && pos[1] === ci;
                  return (
                    <button
                      key={k}
                      onClick={() => press(k)}
                      className={`h-[2.9rem] flex-1 rounded-card text-row font-medium transition-[transform,background-color,color] duration-100 ${on ? "relative z-[1] scale-[1.06] bg-fg text-bg shadow-[0_0_0_0.125rem_rgb(var(--background)),0_0_0_0.3125rem_rgb(var(--text-primary))]" : "bg-l3 text-fg"}`}
                    >
                      {k}
                    </button>
                  );
                })}
              </div>
            ))}
            <div className="flex gap-[0.45rem]">
              {SPECIAL.map((k, ci) => {
                const on = pos[0] === rows.length && pos[1] === ci;
                const g = keyGlyph[k];
                return (
                  <button
                    key={k}
                    onClick={() => press(k)}
                    className={`flex h-[2.9rem] items-center justify-center gap-[0.5rem] rounded-card text-body font-medium transition-[transform,background-color,color] duration-100 ${k === "space" ? "flex-[4]" : "flex-[1.4]"} ${on ? "relative z-[1] scale-[1.04] bg-fg text-bg shadow-[0_0_0_0.125rem_rgb(var(--background)),0_0_0_0.3125rem_rgb(var(--text-primary))]" : k === "done" ? "bg-brand text-white" : "bg-l4 text-fg"}`}
                  >
                    {g && <Glyph b={g} size="sm" />}
                    {label(k)}
                  </button>
                );
              })}
            </div>
          </div>
          <div className="mt-[0.9rem] flex justify-end gap-[1.3rem] text-small text-fg/75">
            <Hint b="a">Type</Hint>
            <Hint b="b">Cancel</Hint>
          </div>
        </div>
      </div>
    </div>
  );
}
