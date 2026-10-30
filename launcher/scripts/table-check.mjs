// Table overflow check: loads the fixture page (src/dev/tables.tsx, worst-case values in a
// half-width column) at 1920x1200 and fails when any cell of a `.trow` row overflows its box
// without an ellipsis, or two cells of a row overlap. Also writes tables.png.
// Usage: npm run dev:ui (other shell), then: node scripts/table-check.mjs [outPng]
import puppeteer from "puppeteer-core";

const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const base = process.env.URL ?? "http://localhost:5180";
const png = process.argv[2] ?? "../docs/screenshots/tables.png";

const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ["--no-sandbox"] });
const page = await browser.newPage();
await page.setViewport({ width: 1920, height: 1200 });
await page.goto(`${base}/tables.html`, { waitUntil: "networkidle0" });
await new Promise((r) => setTimeout(r, 500));
await page.addStyleTag({ content: "html,body,#root{height:auto!important;overflow:visible!important}" });
await page.screenshot({ path: png, fullPage: true });

const problems = await page.evaluate(() => {
  const out = [];
  for (const sec of document.querySelectorAll("[data-table]")) {
    const id = sec.dataset.table;
    const rows = sec.querySelectorAll(".trow");
    if (!rows.length) out.push(`${id}: no .trow rows (not using the shared row)`);
    rows.forEach((row, ri) => {
      const cells = [...row.children];
      cells.forEach((c, ci) => {
        const cs = getComputedStyle(c);
        const text = (c.textContent ?? "").trim().slice(0, 30);
        const over = c.scrollWidth > c.clientWidth + 1;
        // An ellipsis only works on inline content: every descendant must be inline.
        const inline = [...c.querySelectorAll("*")].every((d) => ["inline", "contents", "none"].includes(getComputedStyle(d).display) || d instanceof SVGElement);
        const ellipsized = cs.textOverflow === "ellipsis" && cs.overflow === "hidden" && inline;
        if (ellipsized) return;
        if (over) out.push(`${id} row ${ri} cell ${ci} "${text}": scrollWidth ${c.scrollWidth} > clientWidth ${c.clientWidth}`);
        // descendants must stay inside the cell too
        for (const d of c.querySelectorAll("*")) {
          const r = d.getBoundingClientRect(), cr = c.getBoundingClientRect();
          if (r.width && r.right > cr.right + 1) { out.push(`${id} row ${ri} cell ${ci} "${text}": child sticks out ${Math.round(r.right - cr.right)}px`); break; }
        }
      });
      for (let i = 1; i < cells.length; i++) {
        const a = cells[i - 1].getBoundingClientRect(), b = cells[i].getBoundingClientRect();
        if (a.width && b.width && a.right > b.left + 0.5) out.push(`${id} row ${ri}: cells ${i - 1} and ${i} overlap`);
      }
    });
  }
  return out;
});
await browser.close();
if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log("tables ok: no overflow or overlap");
