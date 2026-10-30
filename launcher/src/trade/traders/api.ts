// The trader ledger behind Top traders and the trader page. Shapes follow the Trade API
// (/leaderboard, /users/:address/*), which reads the ClickHouse trader ledger: realized
// P/L per wallet and position, the 30 day and all time boards, funding links between wallets.
// Every path here is GET and goes through the trade_api allow-list.

export type BoardWindow = "7d" | "30d";

export interface UserChip {
  address: string;
  username: string | null;
  displayName: string | null;
  avatarUrl: string | null;
}

export interface TokenChip {
  coinType: string;
  symbol: string;
  name: string;
  logo: string | null;
  priceUsd: number | null;
  change24h?: number | null;
  verified?: boolean;
}

export interface LeaderRow {
  rank: number;
  user: UserChip;
  /** realized P/L in the window */
  pnlUsd: number;
  volumeUsd: number;
  trades: number;
  /** profitable share of sells with a known cost */
  winRate?: number | null;
  topTokens: TokenChip[];
  tokenCount: number;
  /** team board rows: wallets linked by funding */
  team?: { members: number; entity: string };
}

export interface BoardTag {
  key: string;
  label: string;
  reason: string;
}

export interface BoardExtras {
  available: boolean;
  rows: Record<string, { tags: BoardTag[]; curve30d: number[]; copyPnlUsd: number | null }>;
}

export interface PnlPoint {
  time: number;
  pnlUsd: number;
}

export interface TraderHolding {
  coin: string;
  amount: number;
  priceUsd: number | null;
  valueUsd: number | null;
  costUsd?: number | null;
  unrealized: number | null;
  token: TokenChip | null;
}

export interface TraderStats {
  address: string;
  window: string;
  realized: number;
  unrealized: number;
  pnl: number;
  trades: number;
  buys: number;
  sells: number;
  volume: number;
  wins: number;
  losses: number;
  winRate: number | null;
  holdingsUsd: number | null;
  label?: { smart?: boolean; whale?: boolean; kol?: boolean };
  /** the indexer reads this wallet as a bot */
  bot: boolean;
  firstTradeMs: number | null;
  holdings: TraderHolding[];
}

export interface TraderTrade {
  id: string;
  digest: string;
  timeMs: number;
  buy: boolean;
  usd: number;
  /** coin units */
  amount: number;
  priceUsd: number;
  trader: string;
  coin: string;
}

export interface TradesPage {
  items: TraderTrade[];
  nextBefore: number | null;
  tokens: Record<string, TokenChip>;
}

export interface LinkedPage {
  total: number;
  wallets: Array<{ address: string; username: string | null }>;
}

export interface PublicProfile {
  address: string;
  username: string | null;
  displayName: string | null;
  avatarUrl: string | null;
  followers?: number;
  trades?: number;
}

const enc = encodeURIComponent;

export const traderPaths = {
  board: (w: BoardWindow) => `/leaderboard?window=${w}`,
  extras: (w: BoardWindow) => `/leaderboard/extras?window=${w}`,
  /** funding-linked wallet groups (30 days is the shortest window the team board has) */
  teams: () => `/leaderboard?window=30d&view=teams`,
  profile: (a: string) => `/users/${enc(a)}`,
  stats: (a: string, w: BoardWindow) => `/users/${enc(a)}/stats?window=${w}`,
  pnl: (a: string, w: BoardWindow) => `/users/${enc(a)}/pnl?window=${w}`,
  trades: (a: string, limit = 25) => `/users/${enc(a)}/trades?limit=${limit}`,
  linked: (a: string) => `/users/${enc(a)}/linked?limit=5`,
};

/** The name a trader goes by, Rare username first (names.ts), else as below. */
export function traderNameWith(rare: { primary: string } | null, u: Pick<UserChip, "address" | "username" | "displayName">): string {
  return rare ? rare.primary : traderName(u);
}

/** The name a trader goes by: their username, a display name, else a short address. */
export function traderName(u: Pick<UserChip, "address" | "username" | "displayName">): string {
  if (u.username) return `@${u.username}`;
  if (u.displayName) return u.displayName;
  const a = u.address;
  return `${a.slice(0, 6)}...${a.slice(-4)}`;
}
