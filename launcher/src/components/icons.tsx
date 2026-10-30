// Icons: lucide-react, the set rare.market uses (same version, 1.75 stroke like its header
// and search). Sizes are passed in rem so they scale with the UI.

import type { SVGProps } from "react";
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowRight,
  ArrowUpRight,
  Battery,
  BatteryCharging,
  BatteryFull,
  BatteryLow,
  BatteryMedium,
  Check,
  Dices,
  Eye,
  Folder,
  LayoutGrid,
  Monitor,
  Moon,
  Play,
  Plus,
  Power,
  RotateCcw,
  Rocket,
  Search,
  Store,
  Sun,
  Swords,
  Volume2,
  Wallet,
  Wifi,
  Zap,
  type LucideIcon,
} from "lucide-react";

type P = Omit<SVGProps<SVGSVGElement>, "ref"> & { size?: number | string };

const wrap = (Icon: LucideIcon, fill = false) =>
  function RareIcon({ size = "1.25rem", ...p }: P) {
    return <Icon size={size} strokeWidth={1.75} aria-hidden fill={fill ? "currentColor" : "none"} {...p} />;
  };

export const IconPlay = wrap(Play, true);
export const IconWifi = wrap(Wifi);
export const IconSearch = wrap(Search);
export const IconBolt = wrap(Zap);
export const IconSun = wrap(Sun);
export const IconVolume = wrap(Volume2);
export const IconMoon = wrap(Moon);
export const IconPower = wrap(Power);
export const IconRestart = wrap(RotateCcw);
export const IconDesktop = wrap(Monitor);
export const IconWallet = wrap(Wallet);
export const IconPlus = wrap(Plus);
export const IconEye = wrap(Eye);
export const IconArrowRight = wrap(ArrowRight);
export const IconFolder = wrap(Folder);
export const IconCheck = wrap(Check);
export const IconGrid = wrap(LayoutGrid);
export const IconSwap = wrap(ArrowLeftRight);
export const IconSend = wrap(ArrowUpRight);
export const IconReceive = wrap(ArrowDownLeft);
export const IconDice = wrap(Dices);
export const IconRocket = wrap(Rocket);
export const IconSword = wrap(Swords);
export const IconStore = wrap(Store);

export function IconBattery({ percent = 100, charging = false, ...p }: P & { percent?: number; charging?: boolean }) {
  const Icon = charging ? BatteryCharging : percent >= 80 ? BatteryFull : percent >= 45 ? BatteryMedium : percent >= 15 ? BatteryLow : Battery;
  return wrap(Icon)(p);
}

/** Steam's own mark (simple-icons, CC0): a store's logo, like rare.market's venue logos. */
export function IconSteam({ size = "1.25rem", ...p }: P) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden {...p}>
      <path d="M11.979 0C5.678 0 .511 4.86.022 11.037l6.432 2.658c.545-.371 1.203-.59 1.912-.59.063 0 .125.004.188.006l2.861-4.142V8.91c0-2.495 2.028-4.524 4.524-4.524 2.494 0 4.524 2.031 4.524 4.527s-2.03 4.525-4.524 4.525h-.105l-4.076 2.911c0 .052.004.105.004.159 0 1.875-1.515 3.396-3.39 3.396-1.635 0-3.016-1.173-3.331-2.727L.436 15.27C1.862 20.307 6.486 24 11.979 24c6.627 0 11.999-5.373 11.999-12S18.605 0 11.979 0zM7.54 18.21l-1.473-.61c.262.543.714.999 1.314 1.25 1.297.539 2.793-.076 3.332-1.375.263-.63.264-1.319.005-1.949s-.75-1.121-1.377-1.383c-.624-.26-1.29-.249-1.878-.03l1.523.63c.956.4 1.409 1.5 1.009 2.455-.397.957-1.497 1.41-2.454 1.012H7.54zm11.415-9.303c0-1.662-1.353-3.015-3.015-3.015-1.665 0-3.015 1.353-3.015 3.015 0 1.665 1.35 3.015 3.015 3.015 1.663 0 3.015-1.35 3.015-3.015zm-5.273-.005c0-1.252 1.013-2.266 2.265-2.266 1.249 0 2.266 1.014 2.266 2.266 0 1.251-1.017 2.265-2.266 2.265-1.253 0-2.265-1.014-2.265-2.265z" />
    </svg>
  );
}
