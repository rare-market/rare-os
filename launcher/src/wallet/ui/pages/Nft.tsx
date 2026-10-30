// One collectible (market/web/components/item/item-view.tsx): art, name, collection, traits and
// where it is held, with Send and View in Marketplace.

import { ArrowUpRight, ExternalLink, UserPlus } from "lucide-react";
import { MImg } from "../../../marketplace/MImg";
import { openInMarket } from "../../../marketplace/nav";
import { RarityBadge, Verified } from "../../../marketplace/parts";
import { fmtPct, fmtSui } from "../../../marketplace/format";
import { nftDetail, type NftDetail } from "../../data/nfts";
import { canSignAccount } from "../dev";
import { Btn, Empty, Page, Panel, Row } from "../kit";
import { useWallet, useWalletNav, type WalletView } from "../store";
import { CopyValue } from "../home/bits";
import { LoadBar, shortDigest, useHints, useLoad } from "../home/lib";

function held(d: NftDetail): string {
  if (d.owner.kind === "address") return d.inKiosk ? "In your kiosk" : "In your wallet";
  if (d.owner.kind === "object") return d.inKiosk ? "In a kiosk" : "Held inside another object";
  if (d.owner.kind === "shared") return "Shared object";
  if (d.owner.kind === "immutable") return "Immutable";
  return "Unknown";
}

export function NftPage({ view, active }: { view: Extract<WalletView, { kind: "nft" }>; active: boolean }) {
  const account = useWallet((s) => s.current);
  const epoch = useWallet((s) => s.epoch);
  const push = useWalletNav((s) => s.push);
  const q = useLoad<NftDetail>(`nftd:${view.id}:${account?.address ?? ""}`, () => nftDetail(view.id, account?.address), epoch);
  const d = q.data;
  const canSign = canSignAccount(account);
  useHints(active, [{ b: "a", label: "Select" }, { b: "b", label: "Back" }]);

  if (!d) {
    return (
      <Page active={active} ready={q.error}>
        <LoadBar on={q.loading} />
        {q.error && <Empty title="Could not load this item" />}
      </Page>
    );
  }

  return (
    <Page active={active} ready={d.id}>
      <div className="grid grid-cols-[minmax(0,26rem)_minmax(0,1fr)] gap-10">
        <div className="self-start">
          <div className="relative aspect-square overflow-hidden rounded-xl bg-layer-01">
            <MImg src={d.imageUrl} label={d.collection?.name ?? d.name} width={1024} opts={{ fit: "inside" }} contain className="absolute inset-0 size-full bg-layer-02" />
          </div>
        </div>
        <div className="min-w-0">
          <h1 className="text-3xl font-bold leading-tight">{d.name}</h1>
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2 text-base">
            {d.collection && (
              <span className="flex items-center gap-1.5">
                {d.collection.name} {d.collection.verified && <Verified size="1rem" />}
              </span>
            )}
            <RarityBadge rarity={d.rarity} />
            {d.listing && <span className="text-secondary">Listed for {fmtSui(d.listing.allInMist ?? d.listing.priceMist, { dp: 3 })} SUI</span>}
          </div>
          {d.description && <p className="mt-3 line-clamp-3 text-sm leading-relaxed text-secondary">{d.description}</p>}

          <div className="mt-5 flex flex-wrap gap-2">
            {!canSign ? (
              <Btn variant="primary" def onClick={() => push({ kind: "add" })}>
                <UserPlus size="1.1rem" /> Add a signing account
              </Btn>
            ) : d.canSend ? (
              <Btn variant="primary" def onClick={() => push({ kind: "send-nfts", ids: [d.id] })}>
                <ArrowUpRight size="1.1rem" /> Send
              </Btn>
            ) : (
              <Btn variant="dimmed" def onClick={() => {}}>
                Can't send this one
              </Btn>
            )}
            <Btn onClick={() => openInMarket({ kind: "item", id: d.id })}>
              <ExternalLink size="1.05rem" /> View in Marketplace
            </Btn>
          </div>
          {canSign && !d.canSend && d.cantSend && <p className="mt-2 text-sm text-secondary">{d.cantSend}.</p>}

          {d.attributes.length > 0 && (
            <>
              <h2 className="mb-2 mt-6 text-lg font-bold">Traits</h2>
              <div className="grid grid-cols-3 gap-2">
                {d.attributes.slice(0, 9).map((a) => (
                  <div key={`${a.type}:${a.value}`} className="min-w-0 rounded-lg border border-border-primary bg-layer-01 px-3 py-2">
                    <div className="truncate text-xs text-secondary">{a.type}</div>
                    <div className="truncate text-sm font-semibold">{a.value}</div>
                    {a.pct != null && <div className="text-xs tabular-nums text-secondary">{fmtPct(a.pct, 1)} have this</div>}
                  </div>
                ))}
              </div>
            </>
          )}

          <Panel className="mt-6 flex flex-col gap-2 px-5 py-4">
            <Row label="Held">{held(d)}</Row>
            {d.kioskId && (
              <Row label="Kiosk">
                <CopyValue value={d.kioskId} shown={shortDigest(d.kioskId)} label="Copy kiosk id" />
              </Row>
            )}
            <Row label="Object">
              <CopyValue value={d.id} shown={shortDigest(d.id)} label="Copy object id" />
            </Row>
          </Panel>
        </div>
      </div>
    </Page>
  );
}
