// Injected into every Rare web game window (before the page's own scripts).
// 1. Controller, in one of three modes picked by the game's profile (window.__rareProfile,
//    set by the launcher from src/games/profiles.json):
//    - "keys" (default): buttons become keyboard events, the right stick drives a pointer.
//    - "gamepad": the page reads the standard Gamepad API; Rare exposes one virtual pad with
//      analog sticks and triggers and sends no keys.
//    - "focus": for DOM games (word games, card games, menus): the d-pad moves a focus ring
//      over the page's own buttons, A clicks.
//    - "native" (cloud gaming): on the profile's padHosts the page reads the real controller
//      through the Gamepad API (Chromium's own, with rumble) and Rare sends nothing; on any other
//      host (a sign-in page) it is "focus".
//    Real pads are hidden from the page in the other modes, so input never arrives twice.
// 2. On-screen keyboard: types into text fields, or sends key presses when no field is focused
//    (word games that listen to the keyboard).
// 3. Rare Wallet as a Wallet Standard wallet, so Sui games connect with one tap, with the
//    "rare:platform" feature: the Rare Local API (me, inventory, sell, buy, claim, open,
//    notices) for this game. docs/LOCAL-API.md. Not in app mode (profile `wallet: false`,
//    webview apps such as X and Telegram Web).
// The page never gets Tauri IPC. Wallet requests leave through a blocked navigation to
// rare-wallet://, which the launcher intercepts, and answers come back through
// window.__rareWalletReply. Controller input arrives through window.__rarePad (presses and
// releases) and window.__rarePadState (full analog state, standard mapping).
(() => {
  if (window.__rareInjected) return;
  window.__rareInjected = true;
  // This launch's Local API token (launcher-set, before this script), kept in here, off window.
  const API = window.__rareApi || null;
  try {
    delete window.__rareApi;
  } catch {
    window.__rareApi = undefined;
  }
  // Test harnesses (scripts/localapi-e2e.mjs) stand in for the launcher's navigation hook; set
  // only by an init script that runs before this one, so a page can never install it.
  const TRANSPORT = typeof window.__rareTransport === "function" ? window.__rareTransport : null;
  try {
    delete window.__rareTransport;
  } catch {
    window.__rareTransport = undefined;
  }
  const P = Object.assign({ input: "keys", pointer: true, keys: {} }, window.__rareProfile || {});
  const onPadHost = !(P.padHosts || []).length || P.padHosts.includes(location.hostname);
  const MODE = P.input === "native" && !onPadHost ? "focus" : P.input;

  // A service that gates on the browser (cloud gaming) gets the user agent's platform everywhere
  // it looks, not only in the User-Agent header.
  if (P.userAgent) {
    const ua = P.userAgent;
    const win = /Windows/.test(ua), cros = /CrOS/.test(ua);
    const major = (ua.match(/Chrome\/(\d+)/) || [])[1] || "0";
    const brands = [{ brand: "Chromium", version: major }, { brand: /Edg\//.test(ua) ? "Microsoft Edge" : "Google Chrome", version: major }, { brand: "Not.A/Brand", version: "99" }];
    const platform = win ? "Windows" : cros ? "Chrome OS" : "Linux";
    const data = {
      brands, mobile: false, platform,
      getHighEntropyValues: async (hints) => ({ brands, mobile: false, platform, platformVersion: win ? "15.0.0" : "", architecture: "x86", bitness: "64", model: "", uaFullVersion: major + ".0.0.0", fullVersionList: brands.map((b) => ({ brand: b.brand, version: b.version + ".0.0.0" })) }),
      toJSON: () => ({ brands, mobile: false, platform }),
    };
    try {
      Object.defineProperty(Navigator.prototype, "userAgentData", { configurable: true, get: () => data });
      Object.defineProperty(Navigator.prototype, "platform", { configurable: true, get: () => (win ? "Win32" : "Linux x86_64") });
    } catch {}
  }

  // ---------- keyboard events ----------
  const NAMED = {
    Enter: ["Enter", 13], Escape: ["Escape", 27], " ": ["Space", 32], Tab: ["Tab", 9], Backspace: ["Backspace", 8],
    Shift: ["ShiftLeft", 16], Control: ["ControlLeft", 17], Alt: ["AltLeft", 18],
    ArrowUp: ["ArrowUp", 38], ArrowDown: ["ArrowDown", 40], ArrowLeft: ["ArrowLeft", 37], ArrowRight: ["ArrowRight", 39],
  };
  const keyInfo = (k) => {
    if (NAMED[k]) return [k, NAMED[k][0], NAMED[k][1]];
    if (/^[a-z]$/i.test(k)) return [k, "Key" + k.toUpperCase(), k.toUpperCase().charCodeAt(0)];
    if (/^[0-9]$/.test(k)) return [k, "Digit" + k, k.charCodeAt(0)];
    // US layout: the key each symbol is on (shifted ones share it).
    const map = {
      ".": ["Period", 190], ">": ["Period", 190], ",": ["Comma", 188], "<": ["Comma", 188], "-": ["Minus", 189], "_": ["Minus", 189], "=": ["Equal", 187], "+": ["Equal", 187],
      "/": ["Slash", 191], "?": ["Slash", 191], ";": ["Semicolon", 186], ":": ["Semicolon", 186], "'": ["Quote", 222], '"': ["Quote", 222], "[": ["BracketLeft", 219], "{": ["BracketLeft", 219],
      "]": ["BracketRight", 221], "}": ["BracketRight", 221], "\\": ["Backslash", 220], "|": ["Backslash", 220], "`": ["Backquote", 192], "~": ["Backquote", 192],
      "!": ["Digit1", 49], "@": ["Digit2", 50], "#": ["Digit3", 51], "$": ["Digit4", 52], "%": ["Digit5", 53], "^": ["Digit6", 54], "&": ["Digit7", 55], "*": ["Digit8", 56], "(": ["Digit9", 57], ")": ["Digit0", 48],
    };
    const m = map[k] || ["Unidentified", 0];
    return [k, m[0], m[1]];
  };
  const DEFAULT_KEYS = {
    up: "ArrowUp", down: "ArrowDown", left: "ArrowLeft", right: "ArrowRight",
    a: "Enter", b: "Escape", x: " ", y: "Tab", lb: "q", rb: "e", lt: "Shift", rt: "Control",
    start: "Escape", select: "m",
  };
  const KEYS = Object.assign({}, DEFAULT_KEYS, P.keys || {});
  const target = () => {
    const a = document.activeElement;
    return a && a !== document.body ? a : document.querySelector("canvas") || document.body || document;
  };
  const sendKey = (type, k, to) => {
    if (!k) return;
    const [key, code, keyCode] = keyInfo(k);
    const ev = new KeyboardEvent(type, { key, code, keyCode, which: keyCode, bubbles: true, cancelable: true, composed: true });
    (to || target()).dispatchEvent(ev);
  };
  const tapKey = (k, to) => {
    sendKey("keydown", k, to);
    sendKey("keyup", k, to);
  };

  // ---------- pointer (right stick) ----------
  const ptr = { x: innerWidth / 2, y: innerHeight / 2, el: null, shown: false, down: false, lastMove: 0 };
  const ptrDraw = () => {
    if (!ptr.el) {
      ptr.el = document.createElement("div");
      ptr.el.id = "__rare_ptr";
      ptr.el.style.cssText = "position:fixed;left:0;top:0;width:22px;height:22px;margin:-3px 0 0 -3px;z-index:2147483646;pointer-events:none;transition:opacity .2s";
      ptr.el.innerHTML = '<svg width="22" height="22" viewBox="0 0 22 22"><path d="M3 2l15 8.5-6.4 1.6L8.4 18z" fill="#f2f2f3" stroke="#100c18" stroke-width="1.6" stroke-linejoin="round"/></svg>';
      document.documentElement.appendChild(ptr.el);
    }
    ptr.el.style.transform = `translate(${ptr.x}px,${ptr.y}px)`;
    ptr.el.style.opacity = ptr.shown && !document.pointerLockElement ? "1" : "0";
  };
  const mouse = (type, x, y, extra = {}) => {
    const el = document.pointerLockElement || document.elementFromPoint(x, y) || target();
    const init = { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, screenX: x, screenY: y, button: 0, buttons: ptr.down ? 1 : 0, view: window, ...extra };
    const isPtr = type.startsWith("pointer");
    const ev = isPtr ? new PointerEvent(type, { pointerId: 1, pointerType: "mouse", isPrimary: true, ...init }) : new MouseEvent(type, init);
    el.dispatchEvent(ev);
    return el;
  };
  const ptrMove = (dx, dy) => {
    if (document.pointerLockElement) {
      mouse("pointermove", ptr.x, ptr.y, { movementX: dx, movementY: dy });
      mouse("mousemove", ptr.x, ptr.y, { movementX: dx, movementY: dy });
      return;
    }
    ptr.x = Math.max(0, Math.min(innerWidth - 1, ptr.x + dx));
    ptr.y = Math.max(0, Math.min(innerHeight - 1, ptr.y + dy));
    ptr.shown = true;
    ptr.lastMove = performance.now();
    ptrDraw();
    mouse("pointermove", ptr.x, ptr.y, { movementX: dx, movementY: dy });
    mouse("mousemove", ptr.x, ptr.y, { movementX: dx, movementY: dy });
  };
  const ptrButton = (down) => {
    if (down === ptr.down) return;
    ptr.down = down;
    if (down) {
      const el = mouse("pointerdown", ptr.x, ptr.y);
      mouse("mousedown", ptr.x, ptr.y);
      if (el && el.focus && el !== document.activeElement) try { el.focus({ preventScroll: true }); } catch {}
    } else {
      mouse("pointerup", ptr.x, ptr.y);
      mouse("mouseup", ptr.x, ptr.y);
      mouse("click", ptr.x, ptr.y, { detail: 1 });
      setTimeout(() => visibleField(document.activeElement) && oskShow(document.activeElement), 0);
    }
  };
  // Hide the pointer after a few idle seconds.
  setInterval(() => {
    if (ptr.shown && performance.now() - ptr.lastMove > 4000) {
      ptr.shown = false;
      ptrDraw();
    }
  }, 1000);

  // ---------- focus mode: spatial navigation over the page's own controls ----------
  const CLICKABLE = 'button,a[href],[role=button],[role=tab],[role=menuitem],[role=option],[role=checkbox],input:not([type=hidden]),select,textarea,[tabindex]:not([tabindex="-1"]),[data-key],[onclick]';
  const foc = { el: null, ring: null };
  const usable = (e) => {
    const r = e.getBoundingClientRect();
    if (r.width < 6 || r.height < 6 || r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth) return false;
    if (e.disabled || e.getAttribute("aria-hidden") === "true") return false;
    const st = getComputedStyle(e);
    if (st.visibility === "hidden" || Number(st.opacity) < 0.1 || st.pointerEvents === "none") return false;
    // Skip controls covered by something else (a modal above them).
    const cx = Math.min(innerWidth - 1, Math.max(0, r.left + r.width / 2));
    const cy = Math.min(innerHeight - 1, Math.max(0, r.top + r.height / 2));
    const top = document.elementFromPoint(cx, cy);
    return !top || top === e || e.contains(top) || top.contains(e) || top.id === "__rare_ring" || top.id === "__rare_ptr";
  };
  const candidates = () => {
    const all = Array.from(document.querySelectorAll(CLICKABLE)).filter((e) => !e.closest("#__rare_osk"));
    // Innermost clickables only: a button inside a link counts once.
    return all.filter((e) => usable(e) && !all.some((o) => o !== e && e.contains(o) && usable(o)));
  };
  const ringDraw = () => {
    if (!foc.ring) {
      foc.ring = document.createElement("div");
      foc.ring.id = "__rare_ring";
      foc.ring.style.cssText = "position:fixed;z-index:2147483645;pointer-events:none;border:3px solid #f2f2f3;border-radius:10px;box-shadow:0 0 0 3px #3d5afe,0 0 18px #3d5afe88;transition:all .09s ease-out";
      document.documentElement.appendChild(foc.ring);
    }
    const e = foc.el;
    if (!e || !e.isConnected) {
      foc.ring.style.opacity = "0";
      return;
    }
    const r = e.getBoundingClientRect();
    Object.assign(foc.ring.style, { opacity: "1", left: r.left - 5 + "px", top: r.top - 5 + "px", width: r.width + 4 + "px", height: r.height + 4 + "px" });
  };
  const focusOn = (e) => {
    foc.el = e;
    if (e) {
      e.scrollIntoView({ block: "nearest", inline: "nearest" });
      try { e.focus({ preventScroll: true }); } catch {}
    }
    ringDraw();
  };
  const focusStart = () => {
    const list = candidates();
    if (!list.length) return null;
    for (const sel of [].concat(P.focusStart || [])) {
      const hit = list.find((e) => e.matches(sel));
      if (hit) return hit;
    }
    // The biggest, most central control: usually the page's main "Play" button.
    const cx = innerWidth / 2, cy = innerHeight / 2;
    return list
      .map((e) => { const r = e.getBoundingClientRect(); return [e, r.width * r.height / (1 + Math.hypot(r.left + r.width / 2 - cx, r.top + r.height / 2 - cy))]; })
      .sort((a, b) => b[1] - a[1])[0][0];
  };
  /** Nearest control in a direction: the forward distance counts, the sideways offset counts double. */
  const focusMove = (dir) => {
    const list = candidates();
    if (!foc.el || !foc.el.isConnected || !list.includes(foc.el)) return focusOn(focusStart());
    const a = foc.el.getBoundingClientRect();
    const ax = a.left + a.width / 2, ay = a.top + a.height / 2;
    let best = null, bestScore = Infinity;
    for (const e of list) {
      if (e === foc.el) continue;
      const r = e.getBoundingClientRect();
      const bx = r.left + r.width / 2, by = r.top + r.height / 2;
      const dx = bx - ax, dy = by - ay;
      let fwd, side;
      if (dir === "left") { fwd = -dx; side = Math.abs(dy); if (r.right > a.left + 2) continue; }
      else if (dir === "right") { fwd = dx; side = Math.abs(dy); if (r.left < a.right - 2) continue; }
      else if (dir === "up") { fwd = -dy; side = Math.abs(dx); if (r.bottom > a.top + 2) continue; }
      else { fwd = dy; side = Math.abs(dx); if (r.top < a.bottom - 2) continue; }
      if (fwd <= 0) continue;
      const score = fwd + side * 2;
      if (score < bestScore) { bestScore = score; best = e; }
    }
    if (best) focusOn(best);
    else if (dir === "up" || dir === "down") scrollBy({ top: (dir === "down" ? 1 : -1) * innerHeight * 0.5, behavior: "smooth" });
  };
  const clickEl = (e) => {
    const r = e.getBoundingClientRect();
    const x = r.left + r.width / 2, y = r.top + r.height / 2;
    if (isText(e)) {
      try { e.focus(); } catch {}
      return oskShow(e);
    }
    const o = { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, button: 0, view: window };
    e.dispatchEvent(new PointerEvent("pointerdown", { ...o, pointerId: 1, pointerType: "mouse", isPrimary: true, buttons: 1 }));
    e.dispatchEvent(new MouseEvent("mousedown", { ...o, buttons: 1 }));
    e.dispatchEvent(new PointerEvent("pointerup", { ...o, pointerId: 1, pointerType: "mouse", isPrimary: true }));
    e.dispatchEvent(new MouseEvent("mouseup", o));
    e.dispatchEvent(new MouseEvent("click", { ...o, detail: 1 }));
    // The page may have re-rendered or opened a modal over the control: keep the ring on a
    // live, visible control. Twice, since modals animate in.
    const recheck = () => { if (!foc.el || !foc.el.isConnected || !usable(foc.el)) focusOn(focusStart()); else ringDraw(); };
    setTimeout(recheck, 120);
    setTimeout(recheck, 450);
  };
  if (MODE === "focus") {
    // The ring follows the page's own focus onto a text field: a code split over one box per digit
    // moves it a box on after each digit, and the ring stayed on the first one.
    addEventListener("focusin", (ev) => {
      const t = ev.target;
      if (t && t !== foc.el && isText(t) && usable(t)) {
        foc.el = t;
        ringDraw();
      }
    }, true);
    addEventListener("scroll", () => ringDraw(), true);
    addEventListener("resize", () => ringDraw());
    setInterval(() => foc.el && ringDraw(), 400);
  }

  // ---------- on-screen keyboard ----------
  // The launcher keyboard's layout (launcher/native features/kblayout.rs; a test there keeps this
  // equal): letters (Shift: capitals, and in plain fields the number row's symbols), ?123 and #+=
  // (every ASCII symbol between them), and in web address and email fields . - / : or @ . - _ on
  // the letters with a .com key. Pad: the d-pad moves (rows wrap), A types, X deletes, Y is Shift
  // (#+= / 123 on the symbol pages), LT switches letters and symbols, Menu finishes, B closes; held
  // X and a held A on a key that types repeat, as does a finger held on a key.
  const KB = {"lower":["1234567890","qwertyuiop","asdfghjkl@","zxcvbnm.-_"],"upper":["!?#$%&*()/","QWERTYUIOP","ASDFGHJKL:","ZXCVBNM,'+"],"sym":["1234567890","@#$_&-+()/","*\"':;!?,.=","%~<>[]{}\\|"],"more":["1234567890","[]{}#%^*+=","_\\|~<>`$&@",".,?!'\"-/:;"],"url":":.-/","com":".com"};
  const GRID = { lower: KB.lower.map((r) => Array.from(r)), upper: KB.upper.map((r) => Array.from(r)), sym: KB.sym.map((r) => Array.from(r)), more: KB.more.map((r) => Array.from(r)) };
  const osk = { el: null, input: null, r: 1, c: 0, shift: false, layer: "abc", field: "text", open: false, hold: null };
  const esc = (t) => t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  /** What a field is for: web address and email fields get their own keys. */
  const fieldOf = (i) => {
    if (!i || !i.getAttribute) return "text";
    const t = (i.getAttribute("type") || "").toLowerCase(), m = (i.getAttribute("inputmode") || "").toLowerCase(), ac = (i.getAttribute("autocomplete") || "").toLowerCase();
    if (t === "url" || m === "url" || ac === "url") return "url";
    if (t === "email" || m === "email" || ac.includes("email")) return "email";
    return "text";
  };
  const oskRows = () => {
    if (osk.layer === "sym") return GRID.sym;
    if (osk.layer === "more") return GRID.more;
    const text = osk.field === "text";
    let g = (osk.shift && text ? GRID.upper : GRID.lower).map((r) => r.slice());
    if (osk.shift && !text) g = g.map((r, ri) => (ri === 0 ? r : r.map((k) => (/^[a-z]$/.test(k) ? k.toUpperCase() : k))));
    if (osk.field === "url") [g[2][9], g[3][7], g[3][8], g[3][9]] = Array.from(KB.url);
    return g;
  };
  const oskSpecial = () => ["shift", "layer", ...(osk.field !== "text" ? ["com"] : []), "space", "del", "done"];
  const oskWeights = () => oskSpecial().map((k) => (k === "space" ? (osk.field !== "text" ? 3 : 4) : 1.4));
  const oskLabel = (k) =>
    ({ shift: { abc: "Shift", sym: "#+=", more: "123" }[osk.layer], layer: osk.layer === "abc" ? "?123" : "abc", com: KB.com, space: "Space", del: "Delete", done: osk.input ? "Done" : "Enter" })[k];
  const OSK_BUTTON = { shift: "Y", layer: "LT", del: "X", done: "Menu" };
  const oskKey = (r, c) => (r === 4 ? oskSpecial()[Math.min(c, oskSpecial().length - 1)] : { ch: oskRows()[r][c] });
  // Up and down line up with the keys' centres (Space is wide).
  const toSpecial = (c) => {
    const w = oskWeights(), total = w.reduce((a, b) => a + b, 0), x = ((c + 0.5) / 10) * total;
    let acc = 0;
    for (let i = 0; i < w.length; i++) if (x < (acc += w[i])) return i;
    return w.length - 1;
  };
  const toCol = (i) => {
    const w = oskWeights(), total = w.reduce((a, b) => a + b, 0);
    const centre = w.slice(0, i).reduce((a, b) => a + b, 0) + w[i] / 2;
    return Math.min(9, Math.floor((centre / total) * 10));
  };
  const oskRender = () => {
    if (!osk.el) return;
    const lit = (osk.layer === "abc" && osk.shift) || osk.layer === "more";
    const rows = oskRows().map((row, ri) => `<div>${row.map((k, ci) => `<b data-k="${ri},${ci}" class="${osk.r === ri && osk.c === ci ? "on" : ""}">${esc(k)}</b>`).join("")}</div>`);
    const w = oskWeights();
    rows.push(
      `<div>${oskSpecial()
        .map((k, ci) => `<b data-k="4,${ci}" style="flex:${w[ci]}" class="${osk.r === 4 && osk.c === ci ? "on" : ""} w${k === "shift" && lit ? " lit" : ""}${k === "done" ? " d" : ""}">${OSK_BUTTON[k] ? `<i>${OSK_BUTTON[k]}</i>` : ""}${esc(oskLabel(k))}</b>`)
        .join("")}</div>`,
    );
    rows.push(`<p><i>A</i> Type&emsp;<i>B</i> Close</p>`);
    osk.el.innerHTML = rows.join("");
  };
  /** Opens the keyboard. With an input it edits that field; without one it sends key presses. */
  const oskShow = (input) => {
    if (!osk.el) {
      osk.el = document.createElement("div");
      osk.el.id = "__rare_osk";
      const st = document.createElement("style");
      st.textContent =
        "#__rare_osk{position:fixed;left:0;right:0;bottom:0;z-index:2147483647;background:#191523;padding:12px 24px 14px;font:500 18px Inter,system-ui,sans-serif;color:#f2f2f3;box-shadow:0 -10px 40px #0008;user-select:none;-webkit-user-select:none;touch-action:none}#__rare_osk div{display:flex;gap:6px;margin-top:6px}#__rare_osk b{flex:1;min-width:0;height:44px;display:flex;align-items:center;justify-content:center;gap:6px;border-radius:9px;background:#231e2f;font-weight:500;white-space:nowrap}#__rare_osk b.w{background:#2a2438;font-size:16px}#__rare_osk b.lit{background:#4a4458}#__rare_osk b.d{background:#3d5afe}#__rare_osk b.on{background:#f2f2f3;color:#100c18}#__rare_osk i{font:700 10px Inter,system-ui,sans-serif;font-style:normal;padding:2px 5px;border-radius:5px;background:#f2f2f322}#__rare_osk b.on i{background:#100c1822}#__rare_osk p{margin:10px 0 0;text-align:right;font-size:13px;color:#a09cab}";
      document.documentElement.appendChild(st);
      document.documentElement.appendChild(osk.el);
      // Touch: a key types on press (the field keeps focus), and again while held.
      osk.el.addEventListener("pointerdown", (e) => {
        e.preventDefault();
        const b = e.target.closest && e.target.closest("b[data-k]");
        if (!b) return;
        const [r, c] = b.dataset.k.split(",").map(Number);
        osk.r = r;
        osk.c = c;
        oskHold("a");
      });
      for (const t of ["pointerup", "pointercancel", "pointerleave"]) osk.el.addEventListener(t, () => oskRelease("a"));
    }
    osk.input = input && isText(input) ? input : null;
    osk.field = fieldOf(osk.input);
    osk.layer = "abc";
    osk.shift = false;
    osk.r = 1;
    osk.c = 0;
    osk.open = true;
    osk.el.style.display = "block";
    oskRender();
  };
  const oskHide = () => {
    osk.open = false;
    oskRelease();
    if (osk.el) osk.el.style.display = "none";
  };
  const oskType = (t) => {
    // The page may move the focus itself between keys: a code split over one box per digit goes
    // to the next box after each one. Type where the focus is now, not where the keyboard opened.
    const a = document.activeElement;
    if (osk.input && a && a !== osk.input && isText(a)) {
      osk.input = a;
      osk.field = fieldOf(a);
    }
    const i = osk.input;
    if (!i) {
      // Key mode: the page listens to the keyboard (word games).
      const to = document.activeElement && document.activeElement !== document.body ? document.activeElement : document.body;
      if (t === null || t === "\n") return tapKey(t === null ? "Backspace" : "Enter", to);
      for (const ch of t) tapKey(ch, to);
      return;
    }
    if (t === "\n") {
      oskHide();
      tapKey("Enter", i);
      return;
    }
    if (i.isContentEditable) {
      document.execCommand(t === null ? "delete" : "insertText", false, t ?? undefined);
      return;
    }
    // Delete in an empty box: the page's Backspace handler takes the focus back a box. When the page
    // has none and leaves the focus here, a box of a split code (maxlength) goes back to the box
    // before it and clears that one, as Backspace does on the pages that handle it.
    if (t === null && i.value === "") {
      tapKey("Backspace", i);
      if (i.maxLength > 0) {
        setTimeout(() => {
          if (document.activeElement !== i || i.value !== "") return;
          const prev = box(i, -1);
          if (!prev || !(prev.maxLength > 0)) return;
          prev.focus();
          osk.input = prev;
          osk.field = fieldOf(prev);
          setValue(prev, prev.value.slice(0, -1));
        }, 0);
      }
      return;
    }
    // A full box (maxlength): what a keyboard does, a key the page can act on, no text appended.
    if (t !== null && i.maxLength > 0 && i.value.length >= i.maxLength) {
      for (const ch of t) tapKey(ch, i);
      return;
    }
    setValue(i, t === null ? i.value.slice(0, -1) : i.value + t);
    // A box that is full now (one digit of a split code): on to the next one if the page, after its
    // own handlers ran, left the focus here. Pages that move it themselves are left alone.
    if (t !== null && i.maxLength > 0 && i.value.length >= i.maxLength) {
      setTimeout(() => {
        if (document.activeElement !== i) return;
        const next = box(i, 1);
        if (next) {
          next.focus();
          osk.input = next;
          osk.field = fieldOf(next);
        }
      }, 0);
    }
  };
  /** The text field `step` places from `i` in the page's order (1 next, -1 before), when it is
   *  visible and enabled. */
  const box = (i, step) => {
    const all = Array.from(document.querySelectorAll("input, textarea")).filter((e) => isText(e) && !e.disabled && !e.readOnly && e.offsetParent !== null);
    const k = all.indexOf(i);
    return k >= 0 ? all[k + step] || null : null;
  };
  /** Sets a field's text through the element's own setter (frameworks that track the value see
   *  it) and tells the page with an input event. */
  const setValue = (i, v) => {
    const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(i), "value")?.set;
    setter ? setter.call(i, v) : (i.value = v);
    i.dispatchEvent(new Event("input", { bubbles: true }));
  };
  /** A key pressed (A on it, or a finger). `repeat`: held; only keys that type repeat. */
  const oskHit = (k, repeat) => {
    if (k && k.ch !== undefined) {
      if (osk.layer === "abc" && osk.shift && /^[A-Z]$/.test(k.ch)) osk.shift = false;
      return oskType(k.ch);
    }
    if (repeat && !["space", "del"].includes(k)) return;
    if (k === "shift") {
      if (osk.layer === "abc") osk.shift = !osk.shift;
      else osk.layer = osk.layer === "sym" ? "more" : "sym";
    } else if (k === "layer") {
      osk.layer = osk.layer === "abc" ? "sym" : "abc";
      osk.shift = false;
    } else if (k === "com") oskType(KB.com);
    else if (k === "space") oskType(" ");
    else if (k === "del") oskType(null);
    else if (k === "done") osk.input ? oskType("\n") : (oskType("\n"), oskHide());
  };
  // Any other button ends a hold first (focus may move under a held A).
  const oskOther = (b) => {
    oskRelease();
    oskPad(b, false);
  };
  const oskRepeats = (b) => {
    if (b === "x") return true;
    if (b !== "a") return false;
    const k = oskKey(osk.r, osk.c);
    return typeof k === "object" || k === "space" || k === "del";
  };
  const oskPad = (b, repeat) => {
    const last = 4, n = oskSpecial().length;
    const cols = (row) => (row === last ? n : 10);
    const at = [osk.r, osk.c];
    if (b === "up") [osk.r, osk.c] = osk.r === 0 ? [last, toSpecial(osk.c)] : osk.r === last ? [last - 1, toCol(osk.c)] : [osk.r - 1, osk.c];
    else if (b === "down") [osk.r, osk.c] = osk.r + 1 === last ? [last, toSpecial(osk.c)] : osk.r === last ? [0, toCol(osk.c)] : [osk.r + 1, osk.c];
    else if (b === "left") osk.c = (osk.c - 1 + cols(osk.r)) % cols(osk.r);
    else if (b === "right") osk.c = (osk.c + 1) % cols(osk.r);
    else if (repeat && !oskRepeats(b)) return;
    else if (b === "a") oskHit(oskKey(osk.r, osk.c), repeat);
    else if (b === "x") oskType(null);
    else if (b === "y") oskHit("shift");
    else if (b === "lt") oskHit("layer");
    else if (b === "b") oskHide();
    else if (b === "start") oskHit("done");
    if (at[0] !== osk.r || at[1] !== osk.c) osk.c = Math.min(osk.c, cols(osk.r) - 1);
    if (osk.open) oskRender();
  };
  // Held X, or A (a finger) on a key that types: again after 420 ms, then every 70 ms, for 8 s at most.
  const oskHold = (b) => {
    oskRelease();
    oskPad(b, false);
    if (!osk.open || !oskRepeats(b)) return;
    const since = performance.now();
    osk.hold = { b, t: setTimeout(function again() {
      if (!osk.open || !osk.hold || performance.now() - since > 8000) return oskRelease();
      oskPad(b, true);
      osk.hold.t = setTimeout(again, 70);
    }, 420) };
  };
  const oskRelease = (b) => {
    if (osk.hold && (!b || osk.hold.b === b)) {
      clearTimeout(osk.hold.t);
      osk.hold = null;
    }
  };
  const isText = (t) => t && ((t.tagName === "INPUT" && !["checkbox", "radio", "button", "submit", "range", "color", "file", "hidden", "image", "reset"].includes(t.type)) || t.tagName === "TEXTAREA" || t.isContentEditable);
  // Only real, visible fields: many games keep a hidden input focused to capture keys.
  const visibleField = (t) => {
    if (!isText(t)) return false;
    const r = t.getBoundingClientRect();
    const st = getComputedStyle(t);
    return r.width >= 40 && r.height >= 14 && st.visibility !== "hidden" && Number(st.opacity) > 0.1;
  };
  // Tapping a field on the touchscreen opens the keyboard too.
  document.addEventListener("pointerup", (e) => e.isTrusted && setTimeout(() => visibleField(document.activeElement) && oskShow(document.activeElement), 0), true);

  // ---------- digital presses from the launcher ----------
  const POINTER_CLICK = new Set(P.pointer ? ["rb"] : []);
  window.__rarePad = (b, down) => {
    if (osk.open) {
      if (!down) oskRelease(b);
      else if (b === "a" || b === "x") oskHold(b);
      else oskOther(b);
      return;
    }
    // The page has the real controller; A on a text field still brings the keyboard.
    if (MODE === "native") {
      if (down && b === "a" && visibleField(document.activeElement)) oskShow(document.activeElement);
      return;
    }
    // View opens the keyboard: for the focused field, or as a key sender.
    if (down && b === "select" && (MODE !== "keys" || isText(document.activeElement) || P.keyboard === "keys")) {
      return oskShow(visibleField(document.activeElement) ? document.activeElement : null);
    }
    if (MODE === "gamepad") {
      if (down && b === "a" && visibleField(document.activeElement)) oskShow(document.activeElement);
      return;
    }
    if (MODE === "focus") {
      if (!down) return;
      if (b === "up" || b === "down" || b === "left" || b === "right") return focusMove(b);
      if (b === "a") {
        if (!foc.el || !foc.el.isConnected) return focusOn(focusStart());
        return clickEl(foc.el);
      }
      // Other buttons send the profile's keys (word games: X backspace, Menu enter).
      const k = (P.keys || {})[b] ?? { b: "Escape", start: "Enter" }[b];
      if (k) tapKey(k, document.activeElement && document.activeElement !== document.body ? document.activeElement : document.body);
      if (b === "b") setTimeout(() => (!foc.el || !foc.el.isConnected || !usable(foc.el)) && focusOn(focusStart()), 150);
      return;
    }
    if (POINTER_CLICK.has(b)) return ptrButton(down);
    sendKey(down ? "keydown" : "keyup", KEYS[b]);
    if (down && b === "a" && visibleField(document.activeElement)) oskShow(document.activeElement);
  };

  // ---------- analog state from the launcher: virtual Gamepad and the pointer ----------
  // Standard mapping: 0 A, 1 B, 2 X, 3 Y, 4 LB, 5 RB, 6 LT, 7 RT, 8 View, 9 Menu, 10 L3, 11 R3,
  // 12-15 d-pad up/down/left/right, 16 Guide. Axes: left x, left y, right x, right y (down positive).
  const pad = { buttons: new Array(17).fill(0), axes: [0, 0, 0, 0], ts: 0, connected: false };
  const mkButton = (v) => ({ pressed: v > 0.5, touched: v > 0.05, value: v });
  const virtualPad = () => ({
    id: "Rare Controller (STANDARD GAMEPAD Vendor: 045e Product: 028e)",
    index: 0,
    connected: true,
    mapping: "standard",
    timestamp: pad.ts,
    axes: pad.axes.slice(),
    buttons: pad.buttons.map(mkButton),
    vibrationActuator: null,
    hapticActuators: [],
  });
  if (MODE !== "native") try {
    Object.defineProperty(Navigator.prototype, "getGamepads", {
      configurable: true,
      value: function () {
        return MODE === "gamepad" && pad.connected ? [virtualPad(), null, null, null] : [null, null, null, null];
      },
    });
  } catch {}
  // Native mode: the real pad, except the Rare button (standard button 16), which is Rare's menu
  // only. Holding View for a second is the page's button 16 instead (Xbox's and GeForce NOW's own
  // stream menus), with View let go while it counts as that.
  if (MODE === "native") try {
    const real = Navigator.prototype.getGamepads;
    const viewSince = new Map();
    const mk = (v) => ({ pressed: v > 0.5, touched: v > 0.05, value: v });
    Object.defineProperty(Navigator.prototype, "getGamepads", {
      configurable: true,
      value: function () {
        return Array.from(real.call(this), (p) => {
          if (!p || p.mapping !== "standard" || p.buttons.length < 17) return p;
          const now = performance.now();
          if (!p.buttons[8].pressed) viewSince.delete(p.index);
          else if (!viewSince.has(p.index)) viewSince.set(p.index, now);
          const menu = viewSince.has(p.index) && now - viewSince.get(p.index) > 1000;
          const buttons = Array.from(p.buttons, (b, i) => (i === 16 ? mk(menu ? 1 : 0) : i === 8 && menu ? mk(0) : b));
          return { id: p.id, index: p.index, connected: p.connected, mapping: p.mapping, timestamp: p.timestamp, axes: p.axes, buttons, vibrationActuator: p.vibrationActuator, hapticActuators: p.hapticActuators };
        });
      },
    });
  } catch {}
  let r3 = false, ptrTimer = null;
  const stickToPointer = () => {
    const [, , rx, ry] = pad.axes;
    const mag = Math.hypot(rx, ry);
    if (mag < 0.15) {
      clearInterval(ptrTimer);
      ptrTimer = null;
      return;
    }
    if (!ptrTimer) ptrTimer = setInterval(stickToPointer, 16);
    // Curved response: fine control near the centre, fast across the screen at full tilt.
    const speed = Math.pow((mag - 0.15) / 0.85, 1.8) * 22;
    ptrMove((rx / mag) * speed, (ry / mag) * speed);
  };
  window.__rarePadState = (buttons, axes) => {
    if (MODE === "native") return;
    pad.buttons = buttons;
    pad.axes = axes;
    pad.ts = performance.now();
    if (MODE === "gamepad") {
      if (!pad.connected) {
        pad.connected = true;
        const ev = new Event("gamepadconnected");
        ev.gamepad = virtualPad();
        window.dispatchEvent(ev);
      }
      return;
    }
    if (osk.open) return;
    if (P.pointer) {
      if (!ptrTimer) stickToPointer();
      const down = buttons[11] > 0.5;
      if (down !== r3) {
        r3 = down;
        ptrButton(down);
      }
    }
  };

  // ---------- Rare Wallet (Wallet Standard) ----------
  // App mode (X, Telegram Web in Rare's window): the controller and keyboard only, no wallet.
  if (P.wallet === false) return;
  const CHAINS = ["sui:mainnet"];
  const ICON =
    "data:image/svg+xml;base64," +
    btoa('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff4fd8"/><stop offset=".5" stop-color="#ffd23f"/><stop offset="1" stop-color="#3d5afe"/></linearGradient></defs><rect width="64" height="64" rx="14" fill="#100c18"/><path d="M32 8 52 24 44 52H20L12 24z" fill="url(#g)"/></svg>');
  const pending = new Map();
  let seq = 0;
  const queue = [];
  let flushing = false;
  const flush = () => {
    if (flushing || !queue.length) return;
    flushing = true;
    const msg = queue.shift();
    if (TRANSPORT) TRANSPORT(msg);
    else location.href = "rare-wallet://request/" + encodeURIComponent(btoa(unescape(encodeURIComponent(JSON.stringify(msg)))));
    setTimeout(() => {
      flushing = false;
      flush();
    }, 30);
  };
  const request = (method, params) =>
    new Promise((resolve, reject) => {
      const id = ++seq;
      pending.set(id, { resolve, reject });
      queue.push({ id, method, params, origin: location.origin });
      flush();
    });
  window.__rareWalletReply = (id, ok, value) => {
    const p = pending.get(id);
    if (!p) return;
    pending.delete(id);
    ok ? p.resolve(value) : p.reject(new Error(value));
  };

  const listeners = new Set();
  let accounts = [];
  const toAccount = (a) => ({
    address: a.address,
    publicKey: new Uint8Array(a.publicKey ? Uint8Array.from(atob(a.publicKey), (c) => c.charCodeAt(0)) : []),
    chains: CHAINS,
    features: ["sui:signTransaction", "sui:signAndExecuteTransaction", "sui:signPersonalMessage", "sui:signTransactionBlock", "sui:signAndExecuteTransactionBlock"],
    label: a.label,
  });
  const txJson = async (t) => (typeof t?.toJSON === "function" ? await t.toJSON() : t);

  // rare:platform: every call runs in the launcher for this game only, and every sheet it opens
  // waits for the player (and Rare Wallet's review and PIN for anything signed).
  const platform = (call, args) => request("platform", { call, args: args || {}, token: API ? API.token : "" });
  const noticeListeners = new Set();
  let noticesOn = false;
  window.__rareNotice = (n) => noticeListeners.forEach((f) => {
    try {
      f(n);
    } catch (e) {
      console.warn("rare:platform notice listener failed", e);
    }
  });
  const rarePlatform = {
    version: "1.0.0",
    /** The game's Rare slug (or library id), and this launch's Local API over loopback HTTP. */
    game: API ? API.game : null,
    api: API && API.url ? Object.freeze({ url: API.url, token: API.token }) : null,
    me: () => platform("me"),
    inventory: () => platform("inventory"),
    sell: (input) => platform("sell", input),
    buy: (input) => platform("buy", input),
    claim: (voucher) => platform("claim", typeof voucher === "string" ? { voucher } : voucher),
    open: (view, id) => platform("open", typeof view === "string" ? { view, id } : view),
    /** "notice": offer_received, sold, bought for this game's items. Returns an unsubscribe. */
    on: (event, f) => {
      if (event !== "notice" || typeof f !== "function") return () => {};
      noticeListeners.add(f);
      if (!noticesOn) {
        noticesOn = true;
        platform("notices").catch(() => {
          noticesOn = false;
        });
      }
      return () => noticeListeners.delete(f);
    },
  };
  const wallet = {
    version: "1.0.0",
    name: "Rare Wallet",
    icon: ICON,
    chains: CHAINS,
    get accounts() {
      return accounts;
    },
    features: {
      "standard:connect": {
        version: "1.0.0",
        connect: async () => {
          const r = await request("connect", {});
          accounts = r.accounts.map(toAccount);
          listeners.forEach((f) => f({ accounts }));
          return { accounts };
        },
      },
      "standard:disconnect": {
        version: "1.0.0",
        disconnect: async () => {
          accounts = [];
          listeners.forEach((f) => f({ accounts }));
        },
      },
      "standard:events": {
        version: "1.0.0",
        on: (event, f) => {
          if (event !== "change") return () => {};
          listeners.add(f);
          return () => listeners.delete(f);
        },
      },
      "sui:signTransaction": {
        version: "2.0.0",
        signTransaction: async (input) => request("signTransaction", { transaction: await txJson(input.transaction), chain: input.chain }),
      },
      "sui:signAndExecuteTransaction": {
        version: "2.0.0",
        signAndExecuteTransaction: async (input) => request("signAndExecuteTransaction", { transaction: await txJson(input.transaction), chain: input.chain }),
      },
      // The older names, which older dapp-kit pages (SuiFrens) require before listing a wallet.
      "sui:signTransactionBlock": {
        version: "1.0.0",
        signTransactionBlock: async (input) => request("signTransaction", { transaction: await txJson(input.transactionBlock), chain: input.chain }),
      },
      "sui:signAndExecuteTransactionBlock": {
        version: "1.0.0",
        signAndExecuteTransactionBlock: async (input) => request("signAndExecuteTransaction", { transaction: await txJson(input.transactionBlock), chain: input.chain }),
      },
      "sui:signPersonalMessage": {
        version: "1.1.0",
        signPersonalMessage: async (input) => request("signPersonalMessage", { message: btoa(String.fromCharCode(...input.message)) }),
      },
      "rare:platform": rarePlatform,
    },
  };
  const register = (api) => {
    try {
      api.register(wallet);
    } catch (e) {
      console.warn("Rare Wallet register failed", e);
    }
  };
  try {
    window.dispatchEvent(new CustomEvent("wallet-standard:register-wallet", { detail: register }));
  } catch {}
  window.addEventListener("wallet-standard:app-ready", (e) => register(e.detail));
})();
