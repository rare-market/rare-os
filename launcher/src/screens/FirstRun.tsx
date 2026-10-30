// First run, four steps and one decision each: Welcome (with the language), Wi-Fi, sign in (the username
// is suggested on that screen), Steam. Steps with nothing to do are passed over going forward (see shouldSkip).
// It ends on a full Home. Moving assets from the old SuiPlay wallet is a Home card, favourites come from
// playtime. B goes back a step, Y skips setup, Quick opens Quick Settings (Rare does nothing). The step on screen is saved in the config (onboardingStep), so a
// restart resumes there. Controller only: every step is reachable and left with the pad.
//
// Real flows plug in with registerFirstRunStep("wifi" | "passkey" | "steam",
// Component) (see src/onboarding): the component replaces that step's body, gets next/back/skip
// and handles B itself.

import { useEffect, useRef, useState, type ComponentType } from "react";
import { Check } from "lucide-react";
import { BrandMark } from "../components/Chrome";
import { native } from "../lib/bridge";
import { DEFAULT_LANGUAGE, LANGUAGES, languageName } from "../lib/locales";
import { markSetupDone } from "../lib/setup";
import { useApp } from "../lib/store";
import { focusFirst } from "../nav/focus";
import { feedback } from "../nav/feedback";
import { usePadHandler } from "../nav/input";
import { Button, Hint, LoadBar } from "../ui";
import { loadGames } from "../lib/rare";
import { useWallet } from "../wallet/ui/store";

export type FirstRunStepId = "wifi" | "passkey" | "steam";
export interface FirstRunStepProps {
  next: () => void;
  back: () => void;
  skip: () => void;
}
const slots = new Map<FirstRunStepId, ComponentType<FirstRunStepProps>>();
/** Replace a first-run step's body with a real flow (the wallet's passkey sign-in, its move guide). */
export function registerFirstRunStep(id: FirstRunStepId, component: ComponentType<FirstRunStepProps>) {
  slots.set(id, component);
}

// Welcome (with the language), Wi-Fi, sign in, Steam. Wi-Fi is skipped on the way forward when already
// online (cable, dock). B walks back through the steps that were shown.
const STEPS = ["welcome", "wifi", "passkey", "steam"] as const;
type Step = (typeof STEPS)[number];

/** True when a step has nothing to do right now, so going forward passes over it. */
async function shouldSkip(step: Step): Promise<boolean> {
  try {
    if (step === "wifi") return (await native.netStatus()).online;
  } catch {
    /* unknown: show the step */
  }
  return false;
}

export function FirstRun() {
  const [i, setI] = useState(() => {
    const q = new URLSearchParams(location.search).get("step") ?? "";
    const named = STEPS.indexOf(q as Step);
    return named >= 0 ? named : Math.min(STEPS.length - 1, Math.max(0, Number(q) || 0));
  });
  const hist = useRef<number[]>([]);
  const started = useRef(Date.now());
  const root = useRef<HTMLDivElement>(null);
  const step: Step = STEPS[i];

  // Resume where a restart left off (unless ?step= picked one).
  useEffect(() => {
    if (new URLSearchParams(location.search).has("step")) return;
    void native.config().then((c) => {
      const k = STEPS.indexOf((c.onboardingStep ?? "") as Step);
      if (k > 0) setI(k);
    });
  }, []);
  useEffect(() => {
    void native.config().then((c) => (c.onboarded || c.onboardingStep === step ? undefined : native.setConfig({ ...c, onboardingStep: step })));
  }, [step]);

  // Finishing lands on a full Home: the library is scanned and the wallet read before the overlay goes
  // (at most 4 s of a thin bar), then one toast names the Rare button.
  const [finishing, setFinishing] = useState(false);
  const finish = () => {
    if (finishing) return;
    setFinishing(true);
    console.info(`rare: first run took ${Math.round((Date.now() - started.current) / 1000)} s`);
    void native.config().then((c) => native.setConfig({ ...c, onboarded: true, onboardingStep: null }));
    markSetupDone();
    const ready = Promise.allSettled([useApp.getState().scan({ quiet: true }), useWallet.getState().refresh()]);
    void Promise.race([ready, new Promise((r) => setTimeout(r, 4000))]).then(() => {
      useApp.getState().close();
      useApp.getState().toast("Press the Rare button in any game for the Rare menu");
    });
  };
  const busy = useRef(false);
  const next = async () => {
    if (busy.current) return;
    busy.current = true;
    try {
      let k = i + 1;
      while (k < STEPS.length && (await shouldSkip(STEPS[k]))) k++;
      if (k >= STEPS.length) return finish();
      hist.current.push(i);
      setI(k);
    } finally {
      busy.current = false;
    }
  };
  const back = () => {
    const p = hist.current.pop();
    setI(p ?? Math.max(0, i - 1));
  };

  useEffect(() => {
    const t = setTimeout(() => root.current && focusFirst(root.current), 40);
    return () => clearTimeout(t);
  }, [i]);

  const Slot = step === "wifi" ? slots.get("wifi") : step === "passkey" ? slots.get("passkey") : step === "steam" ? slots.get("steam") : undefined;

  usePadHandler((b) => {
    if (b === "b") {
      // A registered step handles B itself (it may have pages of its own).
      if (Slot) return false;
      if (i > 0) {
        feedback("back");
        back();
      }
      return true;
    }
    if (b === "y") {
      feedback("select");
      finish();
      return true;
    }
    // Quick falls through to App, which opens Quick Settings over setup and comes back here on close.
    if (b === "qam") return false;
    return b === "lb" || b === "rb" || b === "start" || b === "guide";
  });

  const welcome = step === "welcome";
  return (
    <div ref={root} data-nav-scope className="fade fixed inset-0 z-40 flex flex-col overflow-hidden bg-bg">
      {welcome && <WelcomeArt />}
      <header className="relative flex h-[4.5rem] shrink-0 items-center gap-[1.5rem] px-[4rem]">
        <span className="flex items-center gap-[0.5rem]">
          <BrandMark size="1.5rem" />
          <span className="text-[1.25rem] font-semibold tracking-[-0.03em]">rare</span>
        </span>
        <span className="ml-auto flex items-center gap-[0.5rem]" role="progressbar" aria-valuemin={1} aria-valuemax={STEPS.length} aria-valuenow={i + 1} aria-label={`Step ${i + 1} of ${STEPS.length}`}>
          {STEPS.map((s, k) => (
            <span key={s} className={`h-[0.375rem] w-[0.375rem] rounded-full transition-colors duration-200 ${k === i ? "bg-fg" : k < i ? "bg-fg/40" : "bg-fg/15"}`} />
          ))}
        </span>
      </header>

      <main key={step} className="rise relative min-h-0 flex-1 px-[4rem]">
        {finishing ? (
          <div className="flex h-full items-center text-row text-muted">Getting your games ready<LoadBar on className="top-[4.5rem]" /></div>
        ) : Slot ? (
          <Slot next={() => void next()} back={back} skip={finish} />
        ) : (
          <Welcome next={() => void next()} />
        )}
      </main>

      <footer className={`relative flex h-[3rem] shrink-0 items-center justify-end gap-[1.5rem] px-[4rem] text-small text-fg/80 ${welcome ? "" : "border-t border-hair"}`}>
        <Hint b="a">Select</Hint>
        {i > 0 && <Hint b="b">Back</Hint>}
        <Hint b="y">Skip setup</Hint>
      </footer>
    </div>
  );
}

/** ACT's key art, text free, full bleed on the right; a plain linear scrim keeps the words on black. */
const WELCOME_ART = "/games/act/hero.jpg";
function WelcomeArt() {
  // Only a live or beta game may greet a new owner; if ACT ever is not, the page is typographic.
  const [ok, setOk] = useState(true);
  useEffect(() => {
    void loadGames().then((r) => {
      const act = r.games.find((g) => g.slug === "act");
      setOk(!!act && (act.status === "live" || act.status === "beta"));
    });
  }, []);
  if (!ok) return null;
  return (
    <div aria-hidden className="absolute inset-y-0 left-[34%] right-0">
      <img src={WELCOME_ART} alt="" draggable={false} className="fade absolute inset-0 h-full w-full object-cover object-[78%_center] mix-blend-lighten" />
      <div className="absolute inset-y-0 left-0 w-[45%] bg-gradient-to-r from-bg to-transparent" />
      <div className="absolute inset-x-0 bottom-0 h-[25%] bg-gradient-to-t from-bg to-transparent" />
    </div>
  );
}

function Welcome({ next }: { next: () => void }) {
  const [lang, setLang] = useState<string>(DEFAULT_LANGUAGE);
  const [pick, setPick] = useState(false);
  useEffect(() => {
    void native.config().then((c) => setLang(c.language ?? DEFAULT_LANGUAGE));
  }, []);
  const choose = (code: string) => {
    setLang(code);
    setPick(false);
    void native.config().then((c) => native.setConfig({ ...c, language: code }));
  };
  return (
    <div className="flex h-full max-w-[40rem] flex-col justify-center pb-[2rem]">
      <h1 className="text-[3.75rem] font-semibold leading-[1.02] tracking-[-0.04em]">Welcome to Rare OS</h1>
      <p className="mt-[1.25rem] max-w-[34rem] text-row leading-relaxed text-fg/65">Your games, your wallet and the Sui market, in one place.</p>
      <div data-nav-group="firstrun-actions" data-nav-remember className="mt-[2.5rem] flex flex-wrap gap-[0.75rem]">
        <Button variant="primary" size="lg" def onClick={next} className="min-w-[11rem]">
          Get started
        </Button>
        <Button size="lg" variant="ghost" onClick={() => setPick((v) => !v)} hint="Language">
          {languageName(lang)}
        </Button>
      </div>
      {pick && (
        <div data-nav-group="firstrun-language" role="radiogroup" className="fade mt-[1rem] flex w-[22rem] flex-col gap-[0.4rem]">
          {LANGUAGES.map((l) => (
            <button
              key={l.code}
              data-nav
              data-nav-default={l.code === lang ? "" : undefined}
              role="radio"
              aria-checked={l.code === lang}
              onClick={() => choose(l.code)}
              className={`flex items-center justify-between rounded-card border px-[1.2rem] py-[0.8rem] text-left text-body ${l.code === lang ? "border-line-2 bg-l2" : "border-hair bg-l1"}`}
            >
              {l.name}
              {l.code === lang && <Check size="1rem" strokeWidth={2.5} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
