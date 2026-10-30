// New recovery phrase: choose 12 or 24 words, write them down, prove it by picking three words
// (no typing, so a controller is enough), then protect it with a PIN if there is no vault yet.
// The words live only in this page's state and are cleared when it closes.

import { TriangleAlert } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { keystore } from "../../index";
import { newMnemonic, WORDLIST } from "../../core/keys/derive";
import type { PinOptions } from "../../core/keys/keystore";
import { Btn, Page, Panel, Segmented } from "../kit";
import { H, finishAdd, plainError, useHints, useStepBack, vaultGate } from "../onboard/flow";
import { PinSetup } from "../onboard/PinSetup";
import { WordGrid } from "../onboard/WordGrid";
import type { WalletView } from "../store";

type Step = "length" | "show" | "verify" | "pin";

const rnd = (n: number) => {
  const a = new Uint32Array(1);
  crypto.getRandomValues(a);
  return a[0] % n;
};
const shuffle = <T,>(xs: T[]) => {
  const a = xs.slice();
  for (let i = a.length - 1; i > 0; i--) {
    const j = rnd(i + 1);
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
};

/** Three distinct positions, each with the right word among five others. */
function makeQuiz(words: string[]) {
  const pos = shuffle(words.map((_, i) => i)).slice(0, 3).sort((a, b) => a - b);
  return pos.map((i) => {
    const others = shuffle(WORDLIST.filter((w) => !words.includes(w))).slice(0, 5);
    return { index: i, choices: shuffle([words[i], ...others]) };
  });
}

export function CreatePhrasePage({ active }: { view: Extract<WalletView, { kind: "create-phrase" }>; active: boolean }) {
  const [step, setStep] = useState<Step>("length");
  const [count, setCount] = useState<12 | 24>(12);
  const [words, setWords] = useState<string[]>([]);
  const [quiz, setQuiz] = useState<Array<{ index: number; choices: string[] }>>([]);
  const [q, setQ] = useState(0);
  const [wrong, setWrong] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const alive = useRef(true);

  // The phrase never outlives the page.
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      setWords([]);
      setQuiz([]);
    };
  }, []);

  const order: Step[] = ["length", "show", "verify", "pin"];
  const back = () => {
    if (step === "verify" && q > 0) setQ(q - 1);
    else setStep(order[Math.max(0, order.indexOf(step) - 1)]);
    setWrong(false);
  };
  useStepBack(active, step !== "length", back);
  useHints(active, step === "pin" ? [H.select, H.del, H.ok, H.back] : [H.select, H.back]);

  const start = () => {
    setWords(newMnemonic(count === 24 ? 256 : 128).split(" "));
    setStep("show");
  };
  const toVerify = () => {
    setQuiz(makeQuiz(words));
    setQ(0);
    setWrong(false);
    setStep("verify");
  };
  const answer = async (w: string) => {
    if (w !== words[quiz[q].index]) {
      setWrong(true);
      return;
    }
    setWrong(false);
    if (q < quiz.length - 1) return setQ(q + 1);
    const gate = await vaultGate("Enter your PIN to add this phrase");
    if (gate === "pin") setStep("pin");
    else if (gate === "ready") await add();
  };
  const add = async (pin?: PinOptions) => {
    setBusy(true);
    setError("");
    try {
      const a = await keystore.addPhrase(words.join(" "), { pin });
      setWords([]);
      await finishAdd(a.address);
    } catch (e) {
      setError(plainError(e, "Could not save the phrase"));
      if (alive.current) setBusy(false);
    }
  };

  const quizNow = quiz[q];
  const title = { length: "Create a recovery phrase", show: "Your recovery phrase", verify: "Check your backup", pin: "" }[step];
  const wide = step === "show" && words.length > 12;
  const shown = useMemo(() => words, [words]);

  return (
    <Page active={active} narrow={!wide} className={wide ? "mx-auto max-w-[64rem]" : undefined} title={title || undefined} ready={step + q}>
      {step === "length" && (
        <div className="flex flex-col gap-6">
          <p className="text-secondary">You get a list of secret words. They are the only way to get your wallet back if you lose this device.</p>
          <div>
            <div className="mb-1.5 text-sm text-secondary">Length</div>
            <Segmented
              items={[
                { value: "12", label: "12 words" },
                { value: "24", label: "24 words" },
              ]}
              value={String(count)}
              onChange={(v) => setCount(v === "24" ? 24 : 12)}
            />
          </div>
          <Btn variant="primary" def className="w-full" onClick={start}>
            Show my words
          </Btn>
        </div>
      )}

      {step === "show" && (
        <div className="flex flex-col gap-5">
          <Panel className="flex items-start gap-3 px-5 py-4">
            <TriangleAlert size="1.25rem" className="mt-0.5 shrink-0 text-secondary" />
            <p className="text-sm">Write these words down in order and keep them private. Anyone with them can take your money. Rare cannot recover them for you.</p>
          </Panel>
          <WordGrid words={shown} />
          <Btn variant="primary" def className="w-full" onClick={toVerify}>
            I wrote it down
          </Btn>
        </div>
      )}

      {step === "verify" && quizNow && (
        <div className="flex flex-col gap-5">
          <p className="text-secondary">
            Pick word <span className="font-semibold text-primary">#{quizNow.index + 1}</span> of your phrase. Question {q + 1} of {quiz.length}.
          </p>
          <div className="grid grid-cols-3 gap-2">
            {quizNow.choices.map((w, i) => (
              <Btn key={w} def={i === 0} className="w-full font-mono" onClick={() => void answer(w)}>
                {w}
              </Btn>
            ))}
          </div>
          {wrong && <p className="text-sm text-negative">That is not word #{quizNow.index + 1}. Look at your backup and try again.</p>}
          {error && <p className="text-sm text-negative">{error}</p>}
        </div>
      )}

      {step === "pin" && (
        <div className="pt-4">
          <PinSetup busy={busy} onDone={(pin) => void add(pin)} body="It unlocks your recovery phrase on this device. It never leaves it." />
          {error && <p className="mt-4 text-center text-sm text-negative">{error}</p>}
        </div>
      )}
    </Page>
  );
}
