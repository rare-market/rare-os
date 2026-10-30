// Import a recovery phrase or a private key. The phrase is entered a word at a time on the
// on-screen keyboard: type the first letters, pick the BIP39 word, and the next box opens.
// "Paste all words" takes the whole phrase in one go. Nothing here is logged or kept after leaving.

import { useEffect, useMemo, useRef, useState } from "react";
import { keystore } from "../../index";
import { askText } from "../../../components/Osk";
import { isValidMnemonic, keypairFromSecret, WORDLIST } from "../../core/keys/derive";
import type { PinOptions } from "../../core/keys/keystore";
import { Btn, Field, Page, Segmented, Sheet } from "../kit";
import { H, finishAdd, plainError, useHints, useStepBack, vaultGate } from "../onboard/flow";
import { PinSetup } from "../onboard/PinSetup";
import { WordGrid } from "../onboard/WordGrid";
import type { WalletView } from "../store";

type Step = "enter" | "pin";

const lookup = (prefix: string) => WORDLIST.filter((w) => w.startsWith(prefix));

export function ImportPage({ view, active }: { view: Extract<WalletView, { kind: "import" }>; active: boolean }) {
  const isKey = view.what === "key";
  const [step, setStep] = useState<Step>("enter");
  const [count, setCount] = useState<12 | 24>(12);
  const [words, setWords] = useState<string[]>(() => Array(12).fill(""));
  const [key, setKey] = useState("");
  const [pick, setPick] = useState<{ index: number; options: string[] } | null>(null);
  const [flash, setFlash] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    // Strict mode mounts twice: cleanup empties the state, so setup refills the empty boxes.
    setWords((w) => (w.length ? w : Array(12).fill("")));
    return () => {
      alive.current = false;
      setWords([]);
      setKey("");
    };
  }, []);

  useStepBack(active, step === "pin", () => setStep("enter"));
  useHints(active, step === "pin" ? [H.select, H.del, H.ok, H.back] : [H.type, H.back]);

  const changeCount = (n: 12 | 24) => {
    setCount(n);
    setWords((w) => Array.from({ length: n }, (_, i) => w[i] ?? ""));
  };

  const filled = words.every(Boolean);
  const phraseOk = filled && isValidMnemonic(words.join(" "));
  const phraseError = filled && !phraseOk ? "That phrase is not valid. Check each word and the order." : "";
  const keyOk = useMemo(() => {
    if (!key.trim()) return false;
    try {
      keypairFromSecret(key.trim());
      return true;
    } catch {
      return false;
    }
  }, [key]);
  const keyError = key.trim() && !keyOk ? "That is not a valid private key. It starts with suiprivkey1." : "";

  /** Opens the keyboard for box `i`; after a word is accepted, moves to the next empty box. */
  const enterWord = async (i: number, chain = true): Promise<void> => {
    setFlash("");
    const raw = await askText(`Word ${i + 1}`, "", { placeholder: "First letters" });
    if (raw === null || !alive.current) return;
    const t = raw.trim().toLowerCase();
    if (!t) return;
    // Pasting a whole phrase into one box fills them all.
    const many = t.split(/\s+/);
    if (many.length > 1) return fillAll(many);
    const exact = WORDLIST.includes(t) ? [t] : [];
    const options = exact.length ? exact : lookup(t);
    if (options.length === 0) {
      setFlash(`"${t}" is not a recovery word. Check the spelling.`);
      return;
    }
    if (options.length === 1) return accept(i, options[0], chain);
    setPick({ index: i, options: options.slice(0, 12) });
  };
  const accept = (i: number, w: string, chain: boolean) => {
    setWords((cur) => {
      const next = cur.slice();
      next[i] = w;
      return next;
    });
    setPick(null);
    if (!chain) return;
    const nextEmpty = words.findIndex((x, j) => j > i && !x);
    if (nextEmpty >= 0) setTimeout(() => alive.current && void enterWord(nextEmpty), 120);
  };
  const fillAll = (list: string[]) => {
    const n = list.length === 24 ? 24 : 12;
    if (list.length !== 12 && list.length !== 24) {
      setFlash(`That is ${list.length} words. A recovery phrase has 12 or 24.`);
      return;
    }
    setCount(n);
    setWords(list.map((w) => w.toLowerCase()));
  };
  const pasteAll = async () => {
    const v = await askText("Paste your recovery phrase", "", { placeholder: "word1 word2 word3 ...", secret: false });
    if (v) fillAll(v.trim().split(/\s+/));
  };

  const go = async () => {
    setError("");
    const gate = await vaultGate("Enter your PIN to add this wallet");
    if (gate === "pin") setStep("pin");
    else if (gate === "ready") await add();
  };
  const add = async (pin?: PinOptions) => {
    setBusy(true);
    setError("");
    try {
      const a = isKey ? await keystore.addPrivateKey(key.trim(), { pin }) : await keystore.addPhrase(words.join(" "), { pin });
      setWords([]);
      setKey("");
      await finishAdd(a.address);
    } catch (e) {
      setError(plainError(e, "Could not add the wallet"));
      if (alive.current) {
        setBusy(false);
        setStep("enter");
      }
    }
  };

  const ready = isKey ? keyOk : phraseOk;
  return (
    <Page
      active={active}
      narrow={isKey || count === 12}
      className={!isKey && count === 24 ? "mx-auto max-w-[64rem]" : undefined}
      title={step === "pin" ? undefined : isKey ? "Import a private key" : "Import a recovery phrase"}
      subtitle={step === "pin" ? undefined : isKey ? "Paste a key that starts with suiprivkey1. It is stored encrypted on this device." : "Type the first letters of each word and pick it from the list."}
      ready={step}
    >
      {step === "pin" ? (
        <div className="pt-4">
          <PinSetup busy={busy} onDone={(pin) => void add(pin)} body="It unlocks your keys on this device. It never leaves it." />
        </div>
      ) : isKey ? (
        <div className="flex flex-col gap-5">
          <Field def label="Private key" secret value={key} onChange={setKey} placeholder="suiprivkey1..." error={keyError || error} />
          <Btn variant="primary" className={`w-full ${!ready || busy ? "pointer-events-none opacity-40" : ""}`} onClick={() => void go()}>
            {busy ? "Adding" : "Import"}
          </Btn>
        </div>
      ) : (
        <div className="flex flex-col gap-5">
          <div className="flex items-center justify-between gap-4">
            <Segmented
              items={[
                { value: "12", label: "12 words" },
                { value: "24", label: "24 words" },
              ]}
              value={String(count)}
              onChange={(v) => changeCount(v === "24" ? 24 : 12)}
            />
            <Btn size="md" onClick={() => void pasteAll()}>
              Paste all words
            </Btn>
          </div>
          <WordGrid words={words} onPick={(i) => void enterWord(i)} defIndex={Math.max(0, words.findIndex((w) => !w))} bad={phraseError ? new Set(words.map((_, i) => i)) : undefined} />
          {(flash || phraseError || error) && <p className="text-sm text-negative">{flash || phraseError || error}</p>}
          <Btn variant="primary" className={`w-full ${!ready || busy ? "pointer-events-none opacity-40" : ""}`} onClick={() => void go()}>
            {busy ? "Adding" : "Import"}
          </Btn>
        </div>
      )}
      {pick && (
        <Sheet title={`Word ${pick.index + 1}`} subtitle="Pick the word" onClose={() => setPick(null)} width="34rem">
          <div className="grid grid-cols-2 gap-2">
            {pick.options.map((w, i) => (
              <Btn key={w} def={i === 0} className="w-full font-mono" onClick={() => accept(pick.index, w, true)}>
                {w}
              </Btn>
            ))}
          </div>
        </Sheet>
      )}
    </Page>
  );
}
