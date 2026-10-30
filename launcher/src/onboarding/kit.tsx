// Shared pieces for the first-run steps that plug into screens/FirstRun.tsx: a step body with a
// title, scrolling content and a button row, plus a done line, a working bar and a choice row.

import type { ReactNode } from "react";
import { IconCheck } from "../components/icons";
import { usePadHandler } from "../nav/input";

export { Qr } from "../marketplace/BuySheet";

/** B inside a step: `back` returns true when it handled the press (a page inside the step). */
export function useStepB(back: () => void) {
  usePadHandler((b) => {
    if (b !== "b") return false;
    back();
    return true;
  });
}

/** One step: title block on top, the step's content (scrolls on its own), and its buttons. */
export function Step({ title, body, children, actions, wide }: { title: ReactNode; body?: ReactNode; children?: ReactNode; actions?: ReactNode; wide?: boolean }) {
  return (
    <div className={`mx-auto flex h-full w-full flex-col pb-[1.25rem] pt-[1.5rem] ${wide ? "max-w-[66rem]" : "max-w-[48rem]"}`}>
      <div className="shrink-0">
        <h1 className="text-[2.25rem] font-semibold leading-[1.1] tracking-[-0.03em]">{title}</h1>
        {body && <p className="mt-[0.75rem] max-w-[44rem] text-body leading-relaxed text-muted">{body}</p>}
      </div>
      {/* Padding keeps the focus ring inside the scroller. */}
      <div data-scroll className="-mx-[0.75rem] mt-[1.1rem] min-h-0 flex-1 overflow-y-auto px-[0.75rem] py-[0.5rem]">
        {children}
      </div>
      {actions && (
        <div data-nav-group="step-actions" data-nav-remember className="mt-[1rem] flex shrink-0 items-center gap-[0.75rem]">
          {actions}
        </div>
      )}
    </div>
  );
}

/** A focusable radio row (phone or security key). */
export function ChoiceRow({ on, icon, title, body, onClick, def }: { on?: boolean; icon?: ReactNode; title: string; body?: string; onClick: () => void; def?: boolean }) {
  return (
    <button
      data-nav
      data-nav-default={def ? "" : undefined}
      role="radio"
      aria-checked={!!on}
      onClick={onClick}
      className={`flex w-full items-center gap-[1rem] rounded-card border px-[1.25rem] py-[1rem] text-left ${on ? "border-line-2 bg-l2" : "border-hair bg-l1"}`}
    >
      {icon && <span className="flex h-[2.75rem] w-[2.75rem] shrink-0 items-center justify-center rounded-full bg-l3 text-fg">{icon}</span>}
      <span className="min-w-0 flex-1">
        <span className="block text-body font-semibold">{title}</span>
        {body && <span className="mt-[0.15rem] block text-small text-muted">{body}</span>}
      </span>
      {on !== undefined && (
        <span className={`flex h-[1.25rem] w-[1.25rem] shrink-0 items-center justify-center rounded-full border ${on ? "border-fg" : "border-line-2"}`}>
          {on && <span className="h-[0.6rem] w-[0.6rem] rounded-full bg-fg" />}
        </span>
      )}
    </button>
  );
}

/** Something finished: the one green mark these steps use. */
export function DoneLine({ children }: { children: ReactNode }) {
  return (
    <div className="flex items-center gap-[0.7rem] text-body">
      <span className="flex h-[1.6rem] w-[1.6rem] shrink-0 items-center justify-center rounded-full bg-l3 text-fg">
        <IconCheck size="1rem" />
      </span>
      <span>{children}</span>
    </div>
  );
}

/** Thin indeterminate bar with a line of text (no skeletons). */
export function Working({ on, label }: { on: boolean; label?: string }) {
  return (
    <div className="flex flex-col gap-[0.6rem]">
      <div className="relative h-[2px] w-full overflow-hidden rounded-full bg-fg/10">{on && <div className="load-bar absolute inset-0" />}</div>
      {label && <div className="text-small text-muted">{label}</div>}
    </div>
  );
}
