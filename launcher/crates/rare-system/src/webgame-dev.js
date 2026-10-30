// Developer mode banner for a game opened with --dev-game. Page scripts can't remove it by
// accident: it lives in a closed shadow root and ignores the pointer.
(function () {
  var d = window.__rareDev;
  if (!d || window.__rareDevBanner) return;
  window.__rareDevBanner = true;
  function mount() {
    var host = document.createElement("div");
    host.style.cssText = "position:fixed;top:0;left:0;right:0;z-index:2147483647;pointer-events:none;height:0";
    var root = host.attachShadow({ mode: "closed" });
    var bar = document.createElement("div");
    bar.style.cssText = "margin:0 auto;width:max-content;max-width:96vw;padding:3px 12px;border-radius:0 0 8px 8px;background:#f5b301;color:#111;font:600 12px/1.5 system-ui,sans-serif;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;opacity:.92";
    bar.textContent = "Rare developer mode  |  " + d.slug + "  |  " + d.network + "  |  Local API on" + (d.collections.length ? "  |  " + d.collections.length + " collection" + (d.collections.length === 1 ? "" : "s") : "  |  no collections (--dev-collection)");
    root.appendChild(bar);
    (document.documentElement || document.body).appendChild(host);
  }
  if (document.documentElement) mount();
  else document.addEventListener("DOMContentLoaded", mount, { once: true });
})();
