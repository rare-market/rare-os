// Share: screenshots and 30 second clips, shared as a rare.market link stored on Walrus.
//
// For the in-game overlay: drop in <ShareEntry /> (a quick-settings style button that opens the
// Share screen and shows how many captures are new) or call openShare() from any control, and
// takeScreenshot() / saveClip() for the capture buttons. startShare() runs once from App and turns
// "share-captured" events into "Screenshot saved" / "Clip saved" toasts when the player is back.

export { ShareScreen, clipsNote } from "./ShareScreen";
export { ShareEntry, openShare, takeScreenshot, saveClip, shareDevState } from "./entry";
export { startShare, useShare, newCount } from "./store";
export { shareNative, type Capture, type ShareStatus } from "./native";
export { getSponsor, setShareSponsor, type ShareSponsor } from "./sponsor";
export { telegramPoster, setTelegramPoster } from "./post";
export { shareLink, buildShareJson, SHARE_FORMAT } from "./format";
