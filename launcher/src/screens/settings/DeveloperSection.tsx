// Settings > Developer: the Sui network (mainnet or testnet), the testnet endpoints, and what the
// launcher knows about a game opened with `rare-launcher --dev-game <url>`.

import { useEffect, useState } from "react";
import { askText } from "../../components/Osk";
import { native, type DevGame } from "../../lib/bridge";
import { applyNetworkConfig, DEFAULT_RPC, rpcOk } from "../../lib/network";
import { Button, Panel } from "../../ui";
import { ChoiceRow, SettingRow } from "./rows";
import type { SectionProps } from "./config";

type Net = "mainnet" | "testnet";

/** A blank answer clears the override; anything else must be https (http only on this machine). */
export function cleanEndpoint(v: string): string | null | undefined {
  const t = v.trim().replace(/\/+$/, "");
  if (!t) return null;
  return rpcOk(t) ? t : undefined;
}

function EndpointRow({ id, title, body, value, onSet }: { id: string; title: string; body: string; value: string | null | undefined; onSet: (v: string | null) => void }) {
  return (
    <SettingRow id={id} title={title} body={value ? value : body}>
      <Button
        onClick={async () => {
          const a = await askText(title, value ?? "", { placeholder: "https://" });
          if (a === null) return;
          const c = cleanEndpoint(a);
          if (c === undefined) return;
          onSet(c);
        }}
      >
        {value ? "Change" : "Set"}
      </Button>
    </SettingRow>
  );
}

export function DeveloperSection({ cfg, patch }: SectionProps) {
  const net: Net = cfg.network === "testnet" ? "testnet" : "mainnet";
  const [dev, setDev] = useState<DevGame | null>(null);
  useEffect(() => {
    void native.devGame().then(setDev).catch(() => setDev(null));
  }, []);
  const apply = async (p: Parameters<typeof patch>[0]) => {
    await patch(p);
    applyNetworkConfig({ ...cfg, ...p });
  };
  return (
    <>
      <Panel>
        <ChoiceRow
          id="dev-network"
          title="Network"
          body={net === "testnet" ? "Testnet: Sui reads, Rare Wallet and the Rare API use testnet. Nothing here costs real SUI." : "Mainnet. Switch to testnet to try a game with test SUI."}
          value={net}
          choices={[
            { value: "mainnet", label: "Mainnet" },
            { value: "testnet", label: "Testnet" },
          ]}
          onPick={(v) => {
            void apply({ network: v }).then(() => location.reload());
          }}
        />
        <EndpointRow id="dev-testnet-rpc" title="Testnet RPC" body={`${DEFAULT_RPC.testnet} (default)`} value={cfg.testnetRpc} onSet={(v) => void apply({ testnetRpc: v })} />
        <EndpointRow id="dev-testnet-api" title="Testnet market API" body="rare.market with ?network=testnet (default)" value={cfg.testnetApi} onSet={(v) => void apply({ testnetApi: v })} />
        <EndpointRow id="dev-testnet-sponsor" title="Testnet gas sponsor" body="Same as the testnet market API (default)" value={cfg.testnetSponsor} onSet={(v) => void apply({ testnetSponsor: v })} />
      </Panel>
      <Panel>
        <SettingRow
          id="dev-game"
          title="Developer game"
          body={dev ? `${dev.url}, ${dev.slug}, ${dev.collections.length} collection${dev.collections.length === 1 ? "" : "s"}` : "None. Start the launcher with --dev-game <url> to run a local web game with the Local API."}
        />
      </Panel>
    </>
  );
}
