// Friends and parties by SuiNS name: the friend graph, presence and party chat on rare.market
// (/v1/friends, /v1/presence; market api/src/friends), signed by a device key Rare Wallet links
// once, live over the notify stream. App calls startFriends() once.

export { startFriends } from "./live";
export { Friends } from "./ui/FriendsScreen";
export { HomeFriends } from "./ui/HomeFriends";
export { openFriends } from "./nav";
export { useFriends } from "./store";
