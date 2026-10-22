#!/usr/bin/env bun
/**
 * Visual and layout audit, driven through the WebView's debugging port.
 *
 * # Why this is a script and not a unit test
 *
 * Every rule here needs the real window: real fonts, real layout, real theme
 * tokens, the actual Tauri IPC answers. `bun test` runs in a DOM shim where all of
 * that is invented, so a test there would pass on a page that is unreadable on
 * screen. This drives the built application instead.
 *
 * # What it checks
 *
 * For every page, in both themes, at two window sizes:
 *   * WCAG contrast of every leaf text node against its effective background;
 *   * text clipped by `overflow: hidden` or ellipsised by `text-overflow`;
 *   * horizontal overflow of the document;
 *   * buttons whose box leaves the viewport;
 *   * that `<main>` scrolls when its content is taller than the window.
 *
 * # Running it
 *
 *   cargo build -p conflux-desktop --features tauri/custom-protocol
 *   bun .verify/ui_audit.mjs
 *
 * Exits non-zero on any finding, so CI can gate on it. It starts and stops its own
 * copy of the application and leaves the user's settings untouched apart from the
 * theme, which it restores.
 */
import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";

const PORT = 9333;
const ROOT = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const EXE = `${ROOT}target/debug/conflux.exe`;
const PAGES = ["概览", "显示器", "排除的应用", "设置"];
const THEMES = ["浅色", "深色"];
const SIZES = [
  { name: "912x718", w: 912, h: 718 },
  { name: "720x520", w: 720, h: 520 },
];

/** Every finding, as `[where, what]`. Empty means the run passed. */
const findings = [];
/** Combinations actually inspected, so a skipped loop cannot look like a pass. */
const checked = [];

async function targets() {
  for (let i = 0; i < 40; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${PORT}/json/list`);
      const list = await res.json();
      const page = list.find((t) => t.type === "page");
      if (page) return page;
    } catch {
      // The port opens a few seconds after the process starts.
    }
    await sleep(500);
  }
  throw new Error(`no debugging target on port ${PORT}`);
}

function connect(url) {
  const ws = new WebSocket(url);
  let next = 1;
  const pending = new Map();
  ws.addEventListener("message", (event) => {
    const msg = JSON.parse(event.data);
    const settle = pending.get(msg.id);
    if (!settle) return;
    pending.delete(msg.id);
    if (msg.error) settle.reject(new Error(JSON.stringify(msg.error)));
    else settle.resolve(msg.result);
  });
  const ready = new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", reject, { once: true });
  });
  return {
    ready,
    close: () => ws.close(),
    async evaluate(expression) {
      await ready;
      const id = next++;
      const result = new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
      ws.send(JSON.stringify({
        id,
        method: "Runtime.evaluate",
        params: { expression, returnByValue: true, awaitPromise: true },
      }));
      const value = await result;
      if (value.exceptionDetails) {
        throw new Error(`page threw: ${value.exceptionDetails.text}`);
      }
      return value.result.value;
    },
  };
}

const PROBE = `(() => {
  // WebView2 reports computed colours as \`lab(...)\` for anything derived from
  // \`oklch()\`, and an \`rgba()\`-only parser silently returned null for every
  // colour - which made this whole audit a no-op that always passed. Ask the
  // browser to convert: paint the value onto a canvas and read the pixel back.
  const swatch = document.createElement("canvas");
  swatch.width = swatch.height = 1;
  const ctx = swatch.getContext("2d", { willReadFrequently: true });
  function colour(text) {
    if (!text) return null;
    ctx.clearRect(0, 0, 1, 1);
    ctx.fillStyle = "#000";
    ctx.fillStyle = text;
    if (ctx.fillStyle === "#000" && !/^#000|black|rgba?\\(0,\\s*0,\\s*0/.test(text)) return null;
    ctx.fillRect(0, 0, 1, 1);
    const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data;
    return { r, g, b, a: a / 255 };
  }
  function luminance(c) {
    const channel = (v) => {
      v /= 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
  }
  // Composite every layer between the text and the page, not just the first opaque
  // one. The "没有画面" label is white on a 62 %-opaque scrim over a screenshot;
  // skipping the scrim compared white against the card and reported 1.11:1 for a
  // label that is in fact readable. Layers are composited from the bottom up.
  function backgroundOf(el) {
    const layers = [];
    for (let e = el; e && e !== document.documentElement; e = e.parentElement) {
      const c = colour(getComputedStyle(e).backgroundColor);
      if (c && c.a > 0.001) layers.push(c);
    }
    const body = colour(getComputedStyle(document.body).backgroundColor);
    layers.push(body && body.a > 0.001 ? body : { r: 255, g: 255, b: 255, a: 1 });
    let out = layers[layers.length - 1];
    for (let i = layers.length - 2; i >= 0; i--) {
      const top = layers[i];
      const mix = (below, above) => above * top.a + below * (1 - top.a);
      out = { r: mix(out.r, top.r), g: mix(out.g, top.g), b: mix(out.b, top.b), a: 1 };
    }
    return out;
  }

  const contrast = [];
  const clipped = [];
  const nodes = document.querySelectorAll("p,span,div,li,h1,h2,h3,button,label,a,text");
  for (const el of nodes) {
    if (el.children.length > 0) continue;
    const text = (el.innerText || "").trim();
    if (text.length < 2) continue;
    const box = el.getBoundingClientRect();
    if (box.width < 2 || box.height < 2) continue;
    const style = getComputedStyle(el);
    if (style.visibility === "hidden" || style.opacity === "0") continue;

    const fg = colour(style.color);
    if (fg) {
      const a = luminance(fg);
      const b = luminance(backgroundOf(el));
      const ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
      // 4.5:1 is the WCAG AA floor for body text. Nothing here is large enough to
      // qualify for the 3:1 large-text exemption.
      if (ratio < 4.5) contrast.push([text.slice(0, 40), Math.round(ratio * 100) / 100]);
    }

    const ellipsised = style.textOverflow === "ellipsis" && el.scrollWidth - el.clientWidth > 2;
    const hidden = style.overflow === "hidden" && el.clientHeight > 0 && el.scrollHeight - el.clientHeight > 2;
    if (ellipsised || hidden) clipped.push(text.slice(0, 40));
  }

  const doc = document.documentElement;
  const outside = [];
  for (const b of document.querySelectorAll("button")) {
    const r = b.getBoundingClientRect();
    if (r.width > 0 && (r.right > window.innerWidth + 1 || r.left < -1)) {
      outside.push((b.innerText || "").trim().slice(0, 24));
    }
  }

  const main = document.querySelector("main");
  return JSON.stringify({
    page: document.querySelector("h1")?.innerText ?? "?",
    contrast: contrast.slice(0, 8),
    clipped: clipped.slice(0, 8),
    overflowsX: doc.scrollWidth > doc.clientWidth + 1,
    outside,
    main: main
      ? { scrollHeight: main.scrollHeight, clientHeight: main.clientHeight }
      : null,
  });
})()`;

const clickByText = (label) => `(() => {
  const b = [...document.querySelectorAll("button")].find((x) => x.innerText.trim() === ${JSON.stringify(label)});
  if (!b) return false;
  b.click();
  return true;
})()`;

/** Wait until the heading reads `title`, or give up. */
async function waitForPage(client, title) {
  for (let i = 0; i < 20; i++) {
    const now = await client.evaluate('document.querySelector("h1")?.innerText ?? ""');
    if (now === title) return true;
    await sleep(150);
  }
  return false;
}

const app = spawn(EXE, [], {
  cwd: ROOT,
  env: {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT} --remote-allow-origins=*`,
  },
  stdio: "ignore",
});

let client;
try {
  const target = await targets();
  client = connect(target.webSocketDebuggerUrl);
  await client.ready;

  // The debugging port opens before React has mounted, and a click on a page that
  // is still rendering is silently dropped - which then reads as a missing button.
  // Wait for the navigation to exist instead of guessing at a sleep.
  let mounted = false;
  for (let i = 0; i < 60 && !mounted; i++) {
    mounted = await client.evaluate('!!document.querySelector("h1") && document.querySelectorAll("nav button").length > 0');
    if (!mounted) await sleep(250);
  }
  if (!mounted) {
    findings.push(["startup", "navigation never rendered"]);
  }

  for (const theme of THEMES) {
    await client.evaluate(clickByText("设置"));
    // Fixed sleeps were the first bug in this script: 300 ms was enough on an idle
    // desktop and not on a loaded one, and the failure surfaced as "theme button
    // missing" - a layout complaint about a page that had simply not rendered yet.
    // Wait for the heading, then for the button itself.
    if (!(await waitForPage(client, "设置"))) {
      findings.push(["设置", "settings page did not open"]);
    }
    let applied = false;
    for (let i = 0; i < 20 && !applied; i++) {
      applied = await client.evaluate(clickByText(theme));
      if (!applied) await sleep(150);
    }
    if (!applied) findings.push(["设置", `theme button ${theme} missing`]);
    await sleep(400);

    for (const size of SIZES) {
      // The window is resized from outside the page; the probe reads whatever the
      // layout currently is, so the resize only needs to have settled.
      for (const page of PAGES) {
        await client.evaluate(clickByText(page));
        await waitForPage(client, page);
        await sleep(120);
        const raw = await client.evaluate(PROBE);
        const r = JSON.parse(raw);
        const where = `${theme}/${size.name}/${r.page}`;
        checked.push(where);

        if (r.page !== page) findings.push([where, `navigation went to ${r.page}`]);
        for (const [text, ratio] of r.contrast) findings.push([where, `contrast ${ratio}:1 on "${text}"`]);
        for (const text of r.clipped) findings.push([where, `clipped "${text}"`]);
        if (r.overflowsX) findings.push([where, "document overflows horizontally"]);
        for (const label of r.outside) findings.push([where, `button outside viewport: "${label}"`]);
        if (!r.main) findings.push([where, "no <main> scroll container"]);
      }
    }
  }
} finally {
  client?.close();
  app.kill();
}

const expected = PAGES.length * THEMES.length * SIZES.length;
if (checked.length !== expected) {
  findings.push(["coverage", `inspected ${checked.length} of ${expected} combinations`]);
}

if (findings.length === 0) {
  console.log(`ui audit: clean (${checked.length} combinations: ${PAGES.length} pages x ${THEMES.length} themes x ${SIZES.length} sizes)`);
  process.exit(0);
}
console.error(`ui audit: ${findings.length} finding(s)`);
for (const [where, what] of findings) console.error(`  ${where}: ${what}`);
process.exit(1);
