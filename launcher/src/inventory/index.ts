export { InventoryView } from "./InventoryView";
export { InventorySheetHost } from "./ItemSheet";
export { acceptFromNotice, isOfferNotice, showItem } from "./notice";
export { noteSentItems, startInventorySync } from "./store";

import { mock, useMock } from "./mock";
import { setInventorySource, startInventorySync as start } from "./store";

if (useMock) setInventorySource(mock.get);
start();
