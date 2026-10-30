// Move assets out of a Playtron (GameOS) wallet, step by step:
//   1. your new address (QR and text)   2. the Playtron address   3. what's in it (read-only scan)
//   4. the move plan, in signing order  5. check again until only immovable items are left.
// Used by the first-run flow and by Wallet > Move from Playtron. Rare only reads the old wallet;
// the moves are signed in the Playtron wallet itself.
//
// Fees: when the old wallet has too little SUI for the plan and rare.market covers rescue fees,
// the plan switches to sponsored gas and each step is sent from here. That needs a surface that
// signs Rare's exact bytes as the Playtron address (`signCovered`); none exists yet, so without
// one the plan stays as it is and the low-SUI note stays.

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { askText } from "../../components/Osk";
import { isTauri } from "../../lib/bridge";
import { focusFirst } from "../../nav/focus";
import { usePadHandler } from "../../nav/input";
import { Button, Empty } from "../../ui";
import { keystore, resolveAddress, type RareAccount } from "../../wallet";
import { useWallet } from "../../wallet/ui/store";
import { DoneLine, Qr, Step, Working } from "../kit";
import { formatAmount, moveBatchCovered, planMoves, rescueCovered, scanPlaytron, type MovePlan, type RescueScan } from "../rescue";
import { DEMO_PLAYTRON, demoScan } from "./demo";
import { failed } from "../../lib/fail";

type Page = "to" | "from" | "scan" | "plan";
const ORDER: Page[] = ["to", "from", "scan", "plan"];
const SAVED = "rare.playtron.address";

const short = (a: string) => `${a.slice(0, 6)}...${a.slice(-4)}`;
/** 0x and then groups of four, easier to compare by eye */
const grouped = (a: string) => `0x ${(a.slice(2).match(/.{1,4}/g) ?? []).join(" ")}`;
const sui = (mist: bigint) => `${formatAmount(mist, 9)} SUI`;
const params = () => new URLSearchParams(location.search);

function load(): string | null {
  try {
    return localStorage.getItem(SAVED);
  } catch {
    return null;
  }
}
function remember(a: string) {
  try {
    localStorage.setItem(SAVED, a);
  } catch {
    /* private mode */
  }
}

// Browser dev and screenshots: ?devpasskey stands in a watched address for a passkey account.
const DEV_ADDRESS = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const devPasskey = () => !isTauri && params().has("devpasskey");

async function currentAccount(): Promise<RareAccount | null> {
  const a = await keystore.current();
  if (!a && devPasskey()) {
    const d = await keystore.addWatch(DEV_ADDRESS, "Rare passkey");
    await keystore.select(d.address);
    void useWallet.getState().refresh();
    return d;
  }
  return a;
}

/** Accounts that can receive with a key the person holds: everything but watch-only. */
const controls = (a: RareAccount | null | undefined) => !!a && (a.kind !== "watch" || devPasskey());

export interface MoveAssetsProps {
  /** leave the whole flow (B on the first page, "I don't have one", "Done") */
  onExit: () => void;
  /** no wallet you control yet: where to set one up */
  onNeedWallet: () => void;
  /** label of the button that leaves when everything is moved or the user is finished */
  exitLabel?: string;
  /** B on the first page; without it B there is left to whatever hosts this */
  onBack?: () => void;
  /** false while another page covers this one (the wallet's page stack) */
  active?: boolean;
  /** signs exact transaction bytes as the Playtron address (base64 signature). Only with one
   * does Rare offer to cover the fees: the sponsored bytes must be signed as they are. */
  signCovered?: (bytes: Uint8Array) => Promise<string>;
}

export function MoveAssets({ onExit, onNeedWallet, exitLabel = "Done", onBack, active = true, signCovered }: MoveAssetsProps) {
  const [page, setPage] = useState<Page>(() => (params().get("move") as Page) || "to");
  const [account, setAccount] = useState<RareAccount | null | undefined>(undefined);
  const [from, setFrom] = useState<string | null>(() => ((!isTauri && params().has("demoscan")) ? DEMO_PLAYTRON : load()));
  const [scan, setScan] = useState<RescueScan | null>(null);
  const [stage, setStage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checks, setChecks] = useState(0);
  const [cover, setCover] = useState(false);
  const [sent, setSent] = useState<Record<number, "sending" | "sent" | string>>({});
  const root = useRef<HTMLDivElement>(null);
  const abort = useRef<AbortController | null>(null);

  useEffect(() => void currentAccount().then(setAccount), []);
  useEffect(() => () => abort.current?.abort(), []);
  useEffect(() => {
    const t = setTimeout(() => root.current && focusFirst(root.current), 40);
    return () => clearTimeout(t);
  }, [page, scan === null, account === undefined]); // eslint-disable-line react-hooks/exhaustive-deps

  const to = controls(account) ? account!.address : null;

  const run = useCallback(async (address: string, again = false) => {
    abort.current?.abort();
    const ac = new AbortController();
    abort.current = ac;
    setError(null);
    setStage("Starting");
    if (!again) setScan(null);
    try {
      const demo = (!isTauri && params().has("demoscan"));
      const s = demo ? await demoScan(address, setStage, again) : await scanPlaytron(address, { onProgress: setStage, signal: ac.signal });
      if (ac.signal.aborted) return;
      setScan(s);
      if (again) setChecks((n) => n + 1);
    } catch (e) {
      if (!ac.signal.aborted) setError(failed("Couldn't read that wallet.", e, "Check the address and try again."));
    } finally {
      if (!ac.signal.aborted) setStage(null);
    }
  }, []);

  // Dev and screenshots: ?move=scan or ?move=plan opens that page with the demo wallet.
  useEffect(() => {
    if ((page === "scan" || page === "plan") && from && !scan && !stage) void run(from);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const back = () => {
    const i = ORDER.indexOf(page);
    if (i <= 0) return false;
    abort.current?.abort();
    setStage(null);
    setPage(ORDER[i - 1]);
    return true;
  };
  usePadHandler((b) => {
    if (b !== "b") return false;
    if (back()) return true;
    if (!onBack) return false;
    onBack();
    return true;
  }, active && (page !== "to" || !!onBack));

  const enterFrom = async () => {
    const v = await askText("Your old SuiPlay wallet address", from ?? "", { placeholder: "0x... or name.sui" });
    if (!v) return;
    try {
      const a = await resolveAddress(v);
      if (to && a === to) {
        setError("That's your new wallet. Enter the address of the wallet your SuiPlay came with.");
        return;
      }
      setFrom(a);
      remember(a);
      setPage("scan");
      void run(a);
    } catch (e) {
      setError(failed("Couldn't open that wallet.", e));
    }
  };

  useEffect(() => {
    setCover(false);
    if (!signCovered || !from) return;
    let live = true;
    void rescueCovered(from).then((v) => live && setCover(v));
    return () => {
      live = false;
    };
  }, [from, signCovered]);

  const ownPlan: MovePlan | null = scan && to ? planMoves(scan, to) : null;
  // Too little SUI for the fees and Rare covers them: sponsored gas, sent from here
  const covered = !!signCovered && cover && !!ownPlan?.feeNote;
  const plan: MovePlan | null = covered && scan && to ? planMoves(scan, to, "sponsored") : ownPlan;

  const sendCovered = async (index: number) => {
    if (!scan || !plan || !signCovered) return;
    setSent((m) => ({ ...m, [index]: "sending" }));
    try {
      const r = await moveBatchCovered(scan, plan, index, signCovered);
      setSent((m) => ({ ...m, [index]: r.ok ? "sent" : r.message }));
    } catch (e) {
      setSent((m) => ({ ...m, [index]: failed("Couldn't move these.", e) }));
    }
  };

  return (
    <div ref={root} className="h-full">
      {page === "to" && (
        <Step
          title="Move your assets to a wallet you control"
          body="The wallet your SuiPlay came with signs through its maker's sign-in. If that service stops, what's in it can't move anymore. Moving it now keeps it yours."
          actions={
            account === undefined ? null : to ? (
              <>
                <Button variant="primary" size="lg" def onClick={() => setPage("from")}>Continue</Button>
                <Button variant="ghost" size="lg" onClick={onExit}>I don't have an old wallet</Button>
              </>
            ) : (
              <>
                <Button variant="primary" size="lg" def onClick={onNeedWallet}>Set up your wallet</Button>
                <Button size="lg" onClick={() => setPage("from")}>Just look at my old wallet</Button>
                <Button variant="ghost" size="lg" onClick={onExit}>I don't have one</Button>
              </>
            )
          }
          wide
        >
          {to ? (
            <div className="flex items-start gap-[2.5rem]">
              <Qr text={to} className="h-[13rem] w-[13rem] shrink-0" />
              <div className="min-w-0 flex-1">
                <div className="text-[0.8rem] font-medium uppercase tracking-[0.1em] text-muted">Your new address</div>
                <div className="mt-[0.3rem] text-[1.05rem] font-semibold">{account!.label}</div>
                <div className="mt-[0.6rem] max-w-[30rem] break-words font-mono [font-variant-ligatures:none] text-[1.02rem] leading-[1.7] text-fg/90">{grouped(to)}</div>
                <p className="mt-[1rem] max-w-[34rem] text-[0.9rem] leading-relaxed text-muted">Everything from your old SuiPlay wallet goes here. Next, Rare reads what's in the old wallet and lists exactly what to send, in order.</p>
              </div>
            </div>
          ) : account !== undefined ? (
            <Empty
              title="First, a wallet you control"
              body={account ? `${account.label} is watch-only, so it can't receive with a key you hold. Create a passkey wallet to move your assets into.` : "Your assets need somewhere to go: a Rare Wallet signed in with your passkey. You can still look at what's in your old wallet first."}
            />
          ) : null}
        </Step>
      )}

      {page === "from" && (
        <Step
          title="Which wallet came with your SuiPlay?"
          body="Enter its address. On the SuiPlay's original system it's under Wallet, at the top. Rare only reads it: nothing moves until you approve it in that wallet."
          actions={
            <>
              <Button variant="primary" size="lg" def onClick={() => void enterFrom()}>{from ? "Change address" : "Enter address"}</Button>
              {from && <Button size="lg" onClick={() => { setPage("scan"); void run(from); }}>Scan {short(from)}</Button>}
            </>
          }
          wide
        >
          {from ? (
            <div className="max-w-[34rem] break-words font-mono [font-variant-ligatures:none] text-[1.02rem] leading-[1.7] text-fg/90">{grouped(from)}</div>
          ) : (
            <div className="text-[0.95rem] text-muted">A 0x address, or a name ending in .sui if you set one.</div>
          )}
          {error && <p className="mt-[1rem] text-small text-neg">{error}</p>}
        </Step>
      )}

      {page === "scan" && (
        <Step
          title={stage ? "Reading your old wallet" : "What's in your old wallet"}
          body={from ? <>Wallet <span className="num text-fg/80">{short(from)}</span>{scan?.defaultName ? <> ({scan.defaultName})</> : null}. Read from the Sui network; nothing has moved.</> : undefined}
          actions={
            stage ? null : scan ? (
              <>
                {to ? (
                  <Button variant="primary" size="lg" def onClick={() => setPage("plan")} disabled={!plan || plan.empty}>
                    See the move plan
                  </Button>
                ) : (
                  <Button variant="primary" size="lg" def onClick={onNeedWallet}>Set up your wallet to move these</Button>
                )}
                <Button size="lg" onClick={() => from && void run(from)}>Scan again</Button>
                <Button variant="ghost" size="lg" onClick={() => setPage("from")}>Different wallet</Button>
              </>
            ) : (
              <>
                <Button variant="primary" size="lg" def onClick={() => from && void run(from)}>Try again</Button>
                <Button variant="ghost" size="lg" onClick={() => setPage("from")}>Different wallet</Button>
              </>
            )
          }
          wide
        >
          {stage && <Working on label={stage} />}
          {error && <p className="text-small text-neg">{error}</p>}
          {scan && !stage && <ScanResults scan={scan} plan={plan} />}
        </Step>
      )}

      {page === "plan" && plan && scan && (
        <Step
          title={plan.empty ? (checks > 0 ? "Everything that can move has moved" : "Nothing to move") : "Send these, in this order"}
          body={
            plan.empty
              ? plan.stuck.length
                ? "What's left can't be sent by anyone but its own app. Rare keeps an eye on the old wallet in case something new arrives."
                : "The old wallet is empty. Rare keeps an eye on it in case something new arrives."
              : "Sign these in your old SuiPlay wallet while it still works. Each one sends to your new address. Come back here and check again when you're done."
          }
          actions={
            <>
              {plan.empty ? (
                <Button variant="primary" size="lg" def onClick={onExit}>{exitLabel}</Button>
              ) : (
                <Button variant="primary" size="lg" def disabled={!!stage} onClick={() => from && void run(from, true)}>
                  Check again
                </Button>
              )}
              {!plan.empty && <Button variant="ghost" size="lg" onClick={onExit}>Finish later</Button>}
            </>
          }
          wide
        >
          <div className="flex flex-col gap-[1rem]">
            {stage && <Working on label={stage} />}
            {plan.empty && checks > 0 && <DoneLine>Moved to {short(plan.to)}</DoneLine>}
            {plan.feeNote && !plan.empty && <p className="text-small text-warn">{plan.feeNote}</p>}
            {covered && !plan.empty && <p className="text-small text-muted">Rare covers the fees for these moves.</p>}
            {!plan.empty && (
              <div className="grid grid-cols-[minmax(0,1fr)_15rem] items-start gap-[1.5rem]">
                <ol data-nav-group="move-batches" className="flex flex-col gap-[0.6rem]">
                  {plan.batches.map((b) => (
                    <li key={b.index}>
                      <div data-nav tabIndex={0} className="flex gap-[1rem] rounded-card border border-hair bg-l1 px-[1.2rem] py-[1rem]">
                        <span className="num flex h-[2rem] w-[2rem] shrink-0 items-center justify-center rounded-full bg-l4 text-[0.9rem] font-semibold">{b.index}</span>
                        <span className="min-w-0 flex-1">
                          <span className="block text-[0.98rem] font-semibold">{b.title}</span>
                          <span className="mt-[0.3rem] flex flex-col gap-[0.15rem]">
                            {b.lines.slice(0, BATCH_ROWS).map((l) => (
                              <span key={l.kind + l.id} className="flex gap-[0.6rem] text-[0.86rem]">
                                <span className="min-w-0 truncate text-fg/90">{l.label}</span>
                                {l.detail && <span className="min-w-0 truncate text-muted">{l.detail}</span>}
                              </span>
                            ))}
                            {b.lines.length > BATCH_ROWS && <span className="text-[0.82rem] text-muted">and {b.lines.length - BATCH_ROWS} more</span>}
                          </span>
                          {covered && sent[b.index] && sent[b.index] !== "sending" && sent[b.index] !== "sent" && <span className="mt-[0.3rem] block text-[0.82rem] text-neg">{sent[b.index]}</span>}
                        </span>
                        {covered && (
                          <span className="shrink-0 self-center">
                            {sent[b.index] === "sent" ? (
                              <span className="text-[0.86rem] text-muted">Sent</span>
                            ) : (
                              <Button size="sm" disabled={sent[b.index] === "sending"} onClick={() => void sendCovered(b.index)}>
                                {sent[b.index] === "sending" ? "Sending" : "Send"}
                              </Button>
                            )}
                          </span>
                        )}
                      </div>
                    </li>
                  ))}
                </ol>
                <div className="flex flex-col items-center rounded-card border border-hair bg-l1 p-[1rem] text-center">
                  <Qr text={plan.to} className="h-[11rem] w-[11rem]" />
                  <div className="mt-[0.7rem] text-[0.85rem] font-semibold">Send to</div>
                  <div className="mt-[0.2rem] break-all font-mono [font-variant-ligatures:none] text-[0.72rem] leading-relaxed text-muted">{plan.to}</div>
                </div>
              </div>
            )}
            {!plan.empty && (
              <p className="max-w-[44rem] text-[0.86rem] leading-relaxed text-muted">
                Already on Rare OS? The original system stays on this SuiPlay in the boot menu: hold Shift while it powers on (with a USB-C keyboard), pick it, send, then restart into Rare OS.
              </p>
            )}
            {plan.stuck.length > 0 && <Stuck lines={plan.stuck} />}
          </div>
        </Step>
      )}
    </div>
  );
}

function Section({ title, right, children }: { title: string; right?: string; children: ReactNode }) {
  return (
    <section data-nav tabIndex={0} className="flex flex-col rounded-card border border-hair bg-l1 px-[1.2rem] py-[1rem]">
      <div className="mb-[0.5rem] flex items-baseline gap-[0.6rem]">
        <h2 className="text-[0.98rem] font-semibold">{title}</h2>
        {right && <span className="text-[0.82rem] text-muted">{right}</span>}
      </div>
      <div className="flex flex-col gap-[0.25rem] text-[0.88rem]">{children}</div>
    </section>
  );
}

function Row({ left, right }: { left: ReactNode; right?: ReactNode }) {
  return (
    <div className="flex items-baseline gap-[0.8rem]">
      <span className="min-w-0 flex-1 truncate text-fg/90">{left}</span>
      {right != null && <span className="num shrink-0 text-muted">{right}</span>}
    </div>
  );
}

const MAX_ROWS = 5;
const BATCH_ROWS = 8;
function more(n: number) {
  return n > MAX_ROWS ? <div className="text-[0.82rem] text-muted">and {n - MAX_ROWS} more</div> : null;
}

function ScanResults({ scan, plan }: { scan: RescueScan; plan: MovePlan | null }) {
  const kioskItems = scan.kiosks.reduce((n, k) => n + k.items.length, 0);
  const staked = scan.staked.reduce((n, s) => n + s.principal, 0n);
  const nothing = !scan.coins.length && !scan.nfts.length && !scan.kiosks.length && !scan.staked.length && !scan.names.length;
  return (
    <div className="flex flex-col gap-[1rem]">
      {nothing && <Empty title="Nothing to move" body={scan.soulbound.length ? "Only items that can't be sent are in this wallet." : "This wallet is empty."} />}
      <div data-nav-group="scan-sections" className="grid grid-cols-2 items-start gap-[0.9rem]">
        {scan.coins.length > 0 && (
          <Section title="Coins" right={`${scan.coins.length}`}>
            {scan.coins.slice(0, MAX_ROWS).map((c) => (
              <Row key={c.coinType} left={c.symbol} right={formatAmount(c.raw, c.decimals)} />
            ))}
            {more(scan.coins.length)}
          </Section>
        )}
        {scan.nfts.length > 0 && (
          <Section title="Collectibles" right={`${scan.nfts.length}`}>
            {scan.nfts.slice(0, MAX_ROWS).map((n) => (
              <Row key={n.objectId} left={n.name} right={n.collection} />
            ))}
            {more(scan.nfts.length)}
          </Section>
        )}
        {scan.kiosks.length > 0 && (
          <Section title="Kiosks" right={`${scan.kiosks.length}, ${kioskItems} items`}>
            {scan.kiosks.map((k) => (
              <div key={k.kioskId} className="flex flex-col gap-[0.2rem]">
                <Row left={`${k.personal ? "Personal kiosk" : "Kiosk"} ${short(k.kioskId)}`} right={k.profits > 0n ? `${sui(k.profits)} in sales` : undefined} />
                {k.items.slice(0, MAX_ROWS).map((it) => (
                  <Row key={it.objectId} left={<span className="pl-[0.9rem] text-fg/80">{it.name}</span>} right={[it.listed != null ? `Listed ${sui(it.listed)}` : null, it.locked ? "Locked" : null].filter(Boolean).join(", ") || undefined} />
                ))}
                {more(k.items.length)}
              </div>
            ))}
          </Section>
        )}
        {scan.staked.length > 0 && (
          <Section title="Staked SUI" right={`${scan.staked.length}`}>
            <Row left="Total staked" right={sui(staked)} />
            <div className="text-[0.82rem] text-muted">Moves as it is and keeps earning.</div>
          </Section>
        )}
        {scan.names.length > 0 && (
          <Section title="SuiNS names" right={`${scan.names.length}`}>
            {scan.names.slice(0, MAX_ROWS).map((n) => (
              <Row key={n.objectId} left={n.domain} right={`Until ${new Date(n.expiresMs).toLocaleDateString([], { month: "short", year: "numeric" })}`} />
            ))}
            {more(scan.names.length)}
          </Section>
        )}
        {scan.soulbound.length > 0 && (
          <Section title="Can't move" right={`${scan.soulbound.length}`}>
            {(plan?.stuck ?? scan.soulbound.map((s) => ({ id: s.objectId, label: s.name, reason: "" }))).slice(0, MAX_ROWS).map((s) => (
              <div key={s.id}>
                <div className="truncate text-fg/90">{s.label}</div>
                {s.reason && <div className="text-[0.8rem] leading-relaxed text-muted">{s.reason}</div>}
              </div>
            ))}
            {more(scan.soulbound.length)}
          </Section>
        )}
      </div>
      {scan.truncated && <p className="text-[0.86rem] text-muted">This wallet holds more objects than Rare reads at once. Move these first, then check again for the rest.</p>}
    </div>
  );
}

function Stuck({ lines }: { lines: MovePlan["stuck"] }) {
  return (
    <div className="flex flex-col gap-[0.4rem]">
      <h2 className="text-[0.92rem] font-semibold">Stays behind</h2>
      {lines.slice(0, BATCH_ROWS).map((s) => (
        <div key={s.id} className="text-[0.86rem]">
          <span className="text-fg/90">{s.label}</span>
          <span className="text-muted">: {s.reason}</span>
        </div>
      ))}
      {lines.length > BATCH_ROWS && <div className="text-[0.82rem] text-muted">and {lines.length - BATCH_ROWS} more</div>}
    </div>
  );
}
