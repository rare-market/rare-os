// Boot: the same picture Plymouth draws (image/.../plymouth/themes/rare), so the hand-off from the
// boot splash to the launcher cannot be seen. The gem is the very file Plymouth uses (147x160, no
// wordmark, no text, no progress), at Plymouth's place: centred horizontally, top at 45 % of the
// free height, on --background (0x100c18 in rare.plymouth). index.html paints the same thing before
// any script runs (#splash); React replaces it in place. One specular sweep crosses the gem after
// 280 ms; the first frame is the plain gem. It cross-fades into Home as soon as the first library
// scan lands (never before 900 ms). ?boot holds it on screen (screenshots).

import { useEffect, useState } from "react";
import { useApp } from "../lib/store";
import { feedback } from "../nav/feedback";

const MIN_MS = 900;
const GIVE_UP_MS = 8000;

export function Boot() {
  const q = new URLSearchParams(location.search);
  const hold = q.has("boot");
  const [phase, setPhase] = useState<"on" | "out" | "gone">("on");
  const [min, setMin] = useState(false);
  const ready = useApp((s) => s.library !== null);

  useEffect(() => {
    const a = setTimeout(() => setMin(true), MIN_MS);
    const c = setTimeout(() => setPhase((p) => (p === "on" && !hold ? "out" : p)), GIVE_UP_MS);
    return () => [a, c].forEach(clearTimeout);
  }, [hold]);
  useEffect(() => {
    if (!hold && phase === "on" && ready && min) setPhase("out");
  }, [ready, min, phase, hold]);
  useEffect(() => {
    if (phase !== "out") return;
    feedback("boot");
    const t = setTimeout(() => setPhase("gone"), 300);
    return () => clearTimeout(t);
  }, [phase]);

  if (phase === "gone") return null;
  return (
    <div aria-hidden className={`splash transition-opacity duration-300 ease-[cubic-bezier(0.2,0.8,0.2,1)] ${phase === "out" ? "opacity-0" : "opacity-100"}`}>
      <span className="splash-gem">
        <img src="/brand/boot-gem.png" alt="" width={147} height={160} draggable={false} />
        {/* The specular sweep: a light band masked to the gem's own shape, moved with transform once. */}
        <span className="absolute inset-0 overflow-hidden [mask-image:url(/brand/boot-gem.png)] [mask-position:center] [mask-repeat:no-repeat] [mask-size:100%_100%]">
          <span className="boot-sweep absolute inset-y-0 left-0 w-full bg-[linear-gradient(105deg,transparent_30%,rgb(255_255_255/0.55)_50%,transparent_70%)]" />
        </span>
      </span>
    </div>
  );
}
