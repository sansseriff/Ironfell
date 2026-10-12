// Headless-Chrome driver for the Iron app. Reads one command per line from
// stdin, so a heredoc is a script. Run from this directory:
//
//   bun run drive.ts < smoke.txt
//
// Coordinates: `move/drag` take window pixels; `dmove/ddrag` take document
// 2D space (viewer-panel top-left origin, y-down), which is what the store's
// transform2d.x/y hold. Screenshots land in ./screenshots/<name>.png.
import { chromium, type Browser, type Page } from "playwright-core";
import { mkdirSync } from "node:fs";
import { createInterface } from "node:readline";

// URL=… overrides it, e.g. to run the smoke script with ?bevy=views.
const DEFAULT_URL = process.env.URL ?? "http://localhost:5173/Ironfell/?gfx=webgl2";
const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const SHOTS = new URL("./screenshots/", import.meta.url).pathname;
mkdirSync(SHOTS, { recursive: true });

const browser: Browser = await chromium.launch({
  executablePath: CHROME,
  headless: true,
  // GPU=1 uses the machine's GPU (ANGLE on Metal) instead of SwiftShader,
  // for frame-time measurements; SwiftShader caps the app near 10 fps.
  // UNTHROTTLED=1 lifts vsync and the frame-rate limit, so frames per
  // second measures throughput instead of sitting at 60.
  args: [
    ...(process.env.GPU
      ? ["--use-angle=metal", "--enable-gpu", "--ignore-gpu-blocklist", "--no-sandbox"]
      : ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist", "--no-sandbox"]),
    ...(process.env.UNTHROTTLED ? ["--disable-gpu-vsync", "--disable-frame-rate-limit"] : []),
  ],
});
// SIZE=2560x1440@2 sets the window and device scale (that one is 5K).
const [sizeSpec, dprSpec] = (process.env.SIZE ?? "1600x1000@1").split("@");
const [vw, vh] = sizeSpec.split("x").map(Number);
const page: Page = await browser.newPage({ viewport: { width: vw, height: vh }, deviceScaleFactor: Number(dprSpec ?? 1) });

// Console output. Playwright forwards the worker's console on the page
// channel too, so one listener sees both. Rust logs arrive with %c colour
// markup; strip it. `logs` prints what arrived since the last `logs` call,
// and `errors` prints page errors.
let logs: string[] = [];
const errors: string[] = [];
const clean = (t: string) => t.replace(/%c/g, "").replace(/ color:.*$/s, "").trim();
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => logs.push(clean(m.text())));

type Rect = { x: number; y: number; w: number; h: number };
let panel: Rect = { x: 0, y: 0, w: 0, h: 0 };
let found: { x: number; y: number } | null = null;
async function panelRect(name: string): Promise<Rect> {
  return page.evaluate((name) => {
    const el = document.querySelector(`[data-panel-id="${name}"]`);
    if (!el) return { x: 0, y: 0, w: 0, h: 0 };
    const r = el.getBoundingClientRect();
    return { x: r.left, y: r.top, w: r.width, h: r.height };
  }, name);
}
async function readPanel() {
  panel = await panelRect("viewer");
  return panel;
}
const doc = (x: number, y: number) => ({ x: panel.x + x, y: panel.y + y });

// Press and release must land in different frames, and the app may be
// running at ~8 fps under SwiftShader, so the gesture is paced in hundreds
// of milliseconds rather than tens.
async function drag(x0: number, y0: number, x1: number, y1: number, steps = 10) {
  await page.mouse.move(x0, y0);
  await page.waitForTimeout(200);
  await page.mouse.down();
  await page.waitForTimeout(200);
  for (let i = 1; i <= steps; i++) {
    await page.mouse.move(x0 + ((x1 - x0) * i) / steps, y0 + ((y1 - y0) * i) / steps);
    await page.waitForTimeout(40);
  }
  await page.waitForTimeout(200);
  await page.mouse.up();
  // A frame for the release to commit and one for the sync to apply.
  await page.waitForTimeout(400);
}

const out = (s: string) => console.log(s);
async function run(line: string) {
  const [cmd, ...rest] = line.trim().split(/\s+/);
  const n = rest.map(Number);
  switch (cmd) {
    case "":
    case "#":
      return;
    case "nav":
      await page.goto(rest[0] ?? DEFAULT_URL);
      return out(`nav ${rest[0] ?? DEFAULT_URL}`);
    case "ready": {
      // The engine exists when the loading overlay leaves the DOM, but under
      // SwiftShader the first frames take seconds to compile shaders, and a
      // gesture delivered inside one long frame collapses into nothing. So
      // also wait until the frame loop is cycling: poll the cadence probe
      // and require the frame count to be advancing between polls. (Not
      // "fast": SwiftShader manages ~8 fps here, which is fine for input.)
      const t0 = Date.now();
      await page.waitForSelector(".loading", { state: "detached", timeout: 180_000 });
      let last = -1;
      let frames = 0;
      for (let i = 0; i < 120; i++) {
        await page.evaluate(() => (window as any).__probe?.());
        await page.waitForTimeout(500);
        const line = [...logs].reverse().find((l) => l.includes("[cadence probe]"));
        frames = line ? (JSON.parse(line.slice(line.indexOf("{"))).frames ?? 0) : 0;
        if (last >= 0 && frames >= 20 && frames - last >= 2) break;
        last = frames;
      }
      logs = logs.filter((l) => !l.includes("[cadence probe]"));
      await readPanel();
      return out(`ready in ${Date.now() - t0} ms (${frames} frames); viewer panel ${JSON.stringify(panel)}`);
    }
    case "sleep":
      return page.waitForTimeout(n[0] ?? 500);
    case "panel":
      return out(JSON.stringify(rest[0] ? await panelRect(rest[0]) : await readPanel()));
    case "shot": {
      const file = `${SHOTS}${rest[0] ?? "shot"}.png`;
      await page.screenshot({ path: file });
      return out(`screenshot ${file}`);
    }
    case "move":
      return page.mouse.move(n[0], n[1]);
    case "dmove": {
      const p = doc(n[0], n[1]);
      return page.mouse.move(p.x, p.y);
    }
    case "down":
      return page.mouse.down();
    case "up":
      return page.mouse.up();
    case "drag":
      return drag(n[0], n[1], n[2], n[3], n[4] || 10);
    case "ddrag": {
      const a = doc(n[0], n[1]);
      const b = doc(n[2], n[3]);
      return drag(a.x, a.y, b.x, b.y, n[4] || 10);
    }
    case "pdrag": {
      // Drag inside a named panel (viewer, timeline, sidebar), panel-relative.
      const r = await panelRect(rest[0]);
      const [x0, y0, x1, y1, steps] = rest.slice(1).map(Number);
      return drag(r.x + x0, r.y + y0, r.x + x1, r.y + y1, steps || 10);
    }
    case "hunt": {
      // hunt x0 y0 x1 y1 step <js>: sweep the pointer over a grid in
      // document space until <js> (evaluated in the page) is truthy; prints
      // and remembers where (for `dragfound`). For targets whose screen
      // position is hard to predict, such as a shape on a plane in 3D.
      const [x0, y0, x1, y1, st] = rest.slice(0, 5).map(Number);
      const expr = rest.slice(5).join(" ");
      for (let y = y0; y <= y1; y += st) {
        for (let x = x0; x <= x1; x += st) {
          const p = doc(x, y);
          await page.mouse.move(p.x, p.y);
          await page.waitForTimeout(120);
          if (await page.evaluate(expr)) {
            found = { x, y };
            return out(`found at ${x} ${y}`);
          }
        }
      }
      found = null;
      return out("not found");
    }
    case "dragfound": {
      // dragfound dx dy: drag from the last `hunt` hit by (dx, dy).
      if (!found) return out("nothing found to drag");
      const [dx, dy] = rest.map(Number);
      const a = doc(found.x, found.y);
      const b = doc(found.x + dx, found.y + dy);
      return drag(a.x, a.y, b.x, b.y, 10);
    }
    case "keydown":
      await page.keyboard.down(rest[0]);
      return;
    case "keyup":
      await page.keyboard.up(rest[0]);
      return;
    case "click": {
      // Click a DOM element by its visible text (a button in the shell).
      await page.getByRole("button", { name: rest.join(" ") }).first().click();
      return out(`clicked ${rest.join(" ")}`);
    }
    case "pclick": {
      // Click inside a named panel, panel-relative.
      const r = await panelRect(rest[0]);
      const [x, y] = rest.slice(1).map(Number);
      await page.mouse.move(r.x + x, r.y + y);
      await page.waitForTimeout(200);
      await page.mouse.down();
      await page.waitForTimeout(200);
      await page.mouse.up();
      await page.waitForTimeout(200);
      return;
    }
    case "tab": {
      // Activate the dock tab with this title (the workspace, src-ui/views).
      const title = rest.join(" ");
      const tab = page.locator(".dv-tab", { hasText: new RegExp(`^\\s*${title}\\s*$`) }).first();
      await tab.click();
      await page.waitForTimeout(400);
      return out(`tab ${title}`);
    }
    case "dragtab": {
      // dragtab <title> <x> <y>: drag a dock tab to a window point (a
      // group's edge splits, its centre or tab bar stacks).
      const title = rest.slice(0, -2).join(" ");
      const [x, y] = rest.slice(-2).map(Number);
      const tab = page.locator(".dv-tab", { hasText: new RegExp(`^\\s*${title}\\s*$`) }).first();
      const box = await tab.boundingBox();
      if (!box) return out(`no tab ${title}`);
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await page.mouse.down();
      for (let i = 1; i <= 12; i++) {
        await page.mouse.move(box.x + box.width / 2 + ((x - box.x - box.width / 2) * i) / 12, box.y + box.height / 2 + ((y - box.y - box.height / 2) * i) / 12);
        await page.waitForTimeout(40);
      }
      await page.waitForTimeout(200);
      await page.mouse.up();
      await page.waitForTimeout(500);
      return out(`dragged tab ${title}`);
    }
    case "dclick":
    case "ddblclick": {
      // Click (or double-click) at a document point in the viewer, paced
      // like a drag so press and release land in different frames.
      const p = doc(n[0], n[1]);
      await page.mouse.move(p.x, p.y);
      await page.waitForTimeout(200);
      for (let i = 0; i < (cmd === "ddblclick" ? 2 : 1); i++) {
        await page.mouse.down();
        await page.waitForTimeout(80);
        await page.mouse.up();
        await page.waitForTimeout(120);
      }
      await page.waitForTimeout(200);
      return;
    }
    case "key":
      return page.keyboard.press(rest[0]);
    case "eval":
      return out(JSON.stringify(await page.evaluate(rest.join(" "))));
    case "logs": {
      // Console messages from the worker reach Playwright a few hundred
      // milliseconds after the frame that produced them; let them land.
      await page.waitForTimeout(500);
      const re = rest.length ? new RegExp(rest.join(" ")) : null;
      const shown = logs.filter((l) => !re || re.test(l));
      logs = [];
      return out(shown.join("\n") || "(no new console output)");
    }
    case "peek": {
      // Like `logs`, but leaves the buffer alone, so a later `logs` still
      // sees everything.
      await page.waitForTimeout(500);
      const re = rest.length ? new RegExp(rest.join(" ")) : null;
      return out(logs.filter((l) => !re || re.test(l)).join("\n") || "(no matching console output)");
    }
    case "errors":
      return out(errors.join("\n") || "(no page errors)");
    case "quit":
      await browser.close();
      process.exit(0);
    default:
      return out(`unknown command: ${cmd}`);
  }
}

const rl = createInterface({ input: process.stdin });
for await (const line of rl) {
  try {
    await run(line);
  } catch (e) {
    out(`error in "${line}": ${e instanceof Error ? e.message.split("\n")[0] : e}`);
  }
}
await browser.close();
