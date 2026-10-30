// Posting a share link to other apps. The Apps integration (Telegram app) sets telegramPoster when it lands.
export let telegramPoster: ((url: string) => Promise<void>) | null = null;

export function setTelegramPoster(p: ((url: string) => Promise<void>) | null) {
  telegramPoster = p;
}
