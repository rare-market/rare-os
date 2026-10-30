// Send coins: pick the coin, type the amount on the pad, pick or type the recipient, Review.
// Review builds the transfer and hands it to the signing queue; the global review sheet takes
// over from there (simulation, PIN, device or phone), and a sent coin lands back on home.

import { ChevronDown } from "lucide-react";
import { useMemo, useState } from "react";
import { SignRejected, signAndExecute, useSignRequests } from "../../sign";
import { baseToUsd, buildCoinSend, chipAmount, formatUnits, parseUnits, SUI_GAS_RESERVE, toInput, usdToBase, usd } from "../../tx";
import { normType, SUI_TYPE } from "../../review";
import { useWallet as useW } from "../store";
import { canSignAccount } from "../dev";
import { isSuiType, useHints, usePortfolio } from "../home/lib";
import { Avatar, Btn, Field, ListButton, Page, Panel, Sheet, cn } from "../kit";
import { useUnlock } from "../unlock";
import { useWalletNav, type WalletView } from "../store";
import { AmountPad } from "../sign/AmountPad";
import { noteSent } from "../sign/recents";
import { RecipientLine, RecipientPicks, useRecipient } from "../sign/Recipient";

type Mode = "token" | "usd";

export function SendPage({ view, active }: { view: Extract<WalletView, { kind: "send" }>; active: boolean }) {
  const account = useW((s) => s.current);
  const accounts = useW((s) => s.accounts) ?? [];
  const epoch = useW((s) => s.epoch);
  const pf = usePortfolio(account?.address, epoch);
  const [coinType, setCoinType] = useState(normType(view.coinType ?? SUI_TYPE));
  const [picker, setPicker] = useState(false);
  const [mode, setMode] = useState<Mode>("token");
  const [text, setText] = useState("");
  const [isMax, setIsMax] = useState(false);
  const [exact, setExact] = useState<bigint | null>(null);
  const [to, setTo] = useState(view.to ?? "");
  const rs = useRecipient(to);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // a sheet is open (review, PIN, picker): the pad stops listening
  const queued = useSignRequests((s) => s.requests.length > 0);
  const unlocking = useUnlock((s) => !!s.pending);
  const sheet = queued || unlocking;
  const live = active && !picker && !busy && !sheet;

  const tokens = pf.data?.tokens ?? [];
  const held = useMemo(() => tokens.filter((t) => t.raw > 0n).sort((a, b) => (b.valueUsd ?? 0) - (a.valueUsd ?? 0)), [tokens]);
  const token = tokens.find((t) => normType(t.coinType) === coinType);
  const isSui = isSuiType(coinType) || coinType === SUI_TYPE;
  const decimals = token?.decimals ?? 9;
  const symbol = token?.symbol ?? (isSui ? "SUI" : coinType.split("::").pop() ?? "?");
  const avail = token?.raw ?? 0n;
  const price = token?.priceUsd ?? null;

  const base: string | null = isMax ? avail.toString() : exact !== null ? exact.toString() : mode === "token" ? parseUnits(text, decimals) : usdToBase(text, price, decimals);
  const amt = base ? BigInt(base) : 0n;
  const hasAmount = amt > 0n;
  const over = amt > avail;
  const noGas = !over && isSui && !isMax && amt > avail - SUI_GAS_RESERVE;
  const usdValue = hasAmount ? baseToUsd(amt, decimals, price) : null;
  const ready = rs.s === "ready" ? rs : null;
  const self = !!ready && account && ready.address === account.address.toLowerCase();
  const canSign = canSignAccount(account);

  let cta = "Review";
  if (!hasAmount) cta = "Enter an amount";
  else if (over) cta = `Not enough ${symbol}`;
  else if (noGas) cta = "Leave some SUI for gas";
  else if (!ready) cta = rs.s === "empty" ? "Enter a recipient" : rs.s === "loading" ? "Looking up name" : "Check the recipient";
  else if (self) cta = "Pick another address";
  const canReview = canSign && hasAmount && !over && !noGas && !!ready && !self && !busy;

  const typed = (v: string) => {
    setIsMax(false);
    setExact(null);
    setText(v);
    setError(null);
  };
  const chip = (pct: 25 | 50 | 100) => {
    const b = chipAmount(avail, pct, isSui);
    setError(null);
    if (pct === 100) setIsMax(true);
    else setIsMax(false);
    setExact(b);
    setText(mode === "token" ? toInput(b, decimals) : (baseToUsd(b, decimals, price) ?? 0).toFixed(2));
  };
  const toggleMode = () => {
    if (mode === "token") {
      setText(hasAmount && price ? (usdValue ?? 0).toFixed(2).replace(/\.00$/, "") : "");
      setMode("usd");
    } else {
      setText(hasAmount ? toInput(amt, decimals) : "");
      setMode("token");
    }
    setExact(null);
  };
  const pickCoin = (t: string) => {
    setCoinType(normType(t));
    setPicker(false);
    setText("");
    setExact(null);
    setIsMax(false);
    setMode("token");
  };

  const submit = async () => {
    if (!canReview || !account || !ready) return;
    setBusy(true);
    setError(null);
    try {
      const tx = await buildCoinSend({ sender: account.address, to: ready.address, coinType, amount: amt, max: isMax, balance: avail });
      await signAndExecute(tx, { account: account.address, title: `Send ${formatUnits(amt, decimals, 4)} ${symbol}` });
      noteSent(ready.address, ready.suins);
      useW.getState().bump();
      useWalletNav.getState().reset();
    } catch (e) {
      if (!(e instanceof SignRejected)) setError(e instanceof Error ? e.message : "Could not send");
    } finally {
      setBusy(false);
    }
  };

  useHints(active, [
    { b: "a", label: "Select" },
    { b: "x", label: "Delete" },
    ...(canReview ? [{ b: "start" as const, label: "Review" }] : []),
    { b: "b", label: "Back" },
  ]);

  const shown = mode === "usd" ? `$${text || "0"}` : text || "0";
  const converse = mode === "usd" ? `≈ ${formatUnits(amt, decimals, 6)} ${symbol}` : price ? `≈ ${usd(usdValue ?? 0)}` : "No price";

  if (!account) return null;

  return (
    <Page active={active} ready={account.address} title="Send" subtitle={`From ${account.label}`}>
      {!canSign && (
        <Panel className="mx-auto mb-6 max-w-[64rem] px-5 py-4">
          <p className="text-base font-semibold">This account is watch only</p>
          <p className="mt-1 text-sm text-secondary">Add an account that can sign to send from this wallet.</p>
          <Btn variant="primary" className="mt-3" onClick={() => useWalletNav.getState().push({ kind: "add" })}>
            Add an account
          </Btn>
        </Panel>
      )}
      <div className="mx-auto grid max-w-[64rem] grid-cols-[minmax(0,22rem)_minmax(0,1fr)] gap-10">
        <div className="flex flex-col gap-4">
          <button data-nav onClick={() => setPicker(true)} className="flex h-16 w-full items-center gap-3 rounded border border-border-light px-4 text-left">
            <Avatar src={token?.iconUrl} label={symbol} size="2.25rem" />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-base font-semibold">{symbol}</span>
              <span className="block truncate text-xs text-secondary tabular-nums">Balance {formatUnits(avail, decimals, 4)}</span>
            </span>
            <ChevronDown size="1.25rem" className="text-icon-secondary" />
          </button>

          <div className="flex min-h-[7rem] flex-col items-center justify-center text-center">
            <p className={cn("max-w-full truncate font-bold tabular-nums", shown.length > 12 ? "text-3xl" : "text-5xl", !text && !isMax && "text-secondary", over && "text-negative")} data-testid="amount">
              {shown}
            </p>
            {price ? (
              <button data-nav onClick={toggleMode} aria-label={mode === "usd" ? "Type in coins" : "Type in USD"} className="mt-1 rounded px-2 py-1 text-base text-secondary tabular-nums">
                {converse} <span className="ml-1 text-xs">Switch</span>
              </button>
            ) : (
              <p className="mt-1 py-1 text-base text-secondary">{converse}</p>
            )}
          </div>

          <div className="grid grid-cols-3 gap-2">
            <Btn size="md" onClick={() => chip(25)}>25%</Btn>
            <Btn size="md" onClick={() => chip(50)}>50%</Btn>
            <Btn size="md" onClick={() => chip(100)}>Max</Btn>
          </div>
          <AmountPad value={text} onChange={typed} decimals={mode === "usd" ? 2 : decimals} active={live} onSubmit={canReview ? () => void submit() : undefined} />
        </div>

        <div className="flex min-w-0 flex-col gap-4">
          <div>
            <Field label="To" value={to} onChange={(v) => setTo(v.replace(/\s+/g, ""))} placeholder="0x address or name.sui" prompt="Send to" />
            <RecipientLine rs={rs} self={!!self} />
          </div>

          {!ready && <RecipientPicks accounts={accounts} current={account.address} filter={to} onPick={setTo} />}

          <div className="mt-auto">
            {isMax && isSui && <p className="mb-2 text-sm text-secondary">Max sends everything. The network fee comes out of it.</p>}
            {error && (
              <p className="mb-2 text-sm text-negative" role="alert">
                {error}
              </p>
            )}
            <Btn variant={canReview ? "primary" : "dimmed"} className="w-full" onClick={canReview ? () => void submit() : undefined}>
              {busy ? "Opening review" : cta}
            </Btn>
          </div>
        </div>
      </div>

      {picker && (
        <Sheet title="Send which coin" onClose={() => setPicker(false)} width="36rem">
          <div className="-mx-4 flex flex-col">
            {held.length === 0 && <p className="px-4 py-6 text-center text-sm text-secondary">No coins in this account yet.</p>}
            {held.map((t, i) => (
              <ListButton
                key={t.coinType}
                def={normType(t.coinType) === coinType || (i === 0 && !held.some((x) => normType(x.coinType) === coinType))}
                icon={<Avatar src={t.iconUrl} label={t.symbol} size="2.25rem" />}
                title={t.symbol}
                body={`${formatUnits(t.raw, t.decimals, 4)}${t.valueUsd != null ? ` · ${usd(t.valueUsd)}` : ""}`}
                onClick={() => pickCoin(t.coinType)}
              />
            ))}
          </div>
        </Sheet>
      )}
    </Page>
  );
}
