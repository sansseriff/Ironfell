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
// Clipboard permissions, so Cmd+C reaches the system clipboard as it would
// for a person.
const page: Page = await browser.newPage({ viewport: { width: vw, height: vh }, deviceScaleFactor: Number(dprSpec ?? 1), permissions: ["clipboard-read", "clipboard-write"] });

// Console output. Playwright forwards the worker's console on the page
// channel too, so one listener sees both. Rust logs arrive with %c colour
// markup; strip it. `logs` prints what arrived since the last `logs` call,
// and `errors` prints page errors, and any interaction work the engine
// reported drawing a frame late (`report_late_work`), kept apart from `logs`
// so a filtered `logs` cannot clear it.
let logs: string[] = [];
const errors: string[] = [];
const late: string[] = [];
let commandFailed = false;
const clean = (t: string) => t.replace(/%c/g, "").replace(/ color:.*$/s, "").trim();
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => {
  const text = clean(m.text());
  logs.push(text);
  if (text.includes("drawn a frame late")) late.push(text);
  // Rust panics can be caught by the frame wrapper rather than surfacing as
  // pageerror. They must still fail a smoke/benchmark run.
  if (text.includes("panicked at")) commandFailed = true;
});

// Default panel offsets are CSS px. Benchmarks address physical panel px,
// matching the stress fixture's document coordinates at zoom 1.
const panelUnit = process.env.PANEL_PIXELS === "physical" ? 1 / Number(dprSpec ?? 1) : 1;
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
    case "up": {
      // down|up [left|right|middle]: right-drag orbits a 3D panel's camera.
      const button = (rest[0] ?? "left") as "left" | "right" | "middle";
      return cmd === "down" ? page.mouse.down({ button }) : page.mouse.up({ button });
    }
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
      return drag(r.x + x0 * panelUnit, r.y + y0 * panelUnit, r.x + x1 * panelUnit, r.y + y1 * panelUnit, steps || 10);
    }
    case "scheme": {
      // scheme light|dark: the system appearance the page sees (the app's
      // theme follows it unless Settings forces one).
      await page.emulateMedia({ colorScheme: rest[0] as "light" | "dark" });
      return out(`scheme ${rest[0]}`);
    }
    case "dpr": {
      // dpr <ratio>: change the page's devicePixelRatio live, as moving the
      // window to a screen of another density does.
      const vp = page.viewportSize()!;
      const cdp = await page.context().newCDPSession(page);
      await cdp.send("Emulation.setDeviceMetricsOverride", { width: vp.width, height: vp.height, deviceScaleFactor: Number(rest[0]), mobile: false });
      return out(`dpr ${rest[0]}`);
    }
    case "mdrag": {
      // mdrag <panel> x0 y0 x1 y1 [steps]: drag with the middle button
      // (pans a 2D view), panel-relative.
      const r = await panelRect(rest[0]);
      const [x0, y0, x1, y1, steps] = rest.slice(1).map(Number);
      const n = steps || 10;
      await page.mouse.move(r.x + x0 * panelUnit, r.y + y0 * panelUnit);
      await page.mouse.down({ button: "middle" });
      for (let i = 1; i <= n; i++) {
        await page.mouse.move(r.x + (x0 + ((x1 - x0) * i) / n) * panelUnit, r.y + (y0 + ((y1 - y0) * i) / n) * panelUnit);
        await page.waitForTimeout(16);
      }
      await page.mouse.up({ button: "middle" });
      return;
    }
    case "pmove": {
      // pmove <panel> x y: move the pointer to a panel-relative point.
      const r = await panelRect(rest[0]);
      const [x, y] = rest.slice(1).map(Number);
      return page.mouse.move(r.x + x * panelUnit, r.y + y * panelUnit);
    }
    case "psweep": {
      // psweep <panel> x0 y0 x1 y1 steps ms: move (not press) from one
      // panel point to another in `steps` moves, `ms` apart. With `down`
      // before and `up` after, a gesture held for as long as you like.
      const r = await panelRect(rest[0]);
      const [x0, y0, x1, y1, steps, ms] = rest.slice(1).map(Number);
      for (let i = 1; i <= steps; i++) {
        await page.mouse.move(r.x + (x0 + ((x1 - x0) * i) / steps) * panelUnit, r.y + (y0 + ((y1 - y0) * i) / steps) * panelUnit);
        await page.waitForTimeout(ms);
      }
      return;
    }
    case "praf": {
      // Frame-paced synthetic pointer input through the normal DOM bridge.
      // praf panel x0 y0 x1 y1 ms. Used to remove WebDriver pacing overhead.
      const r = await panelRect(rest[0]);
      const [x0,y0,x1,y1,ms] = rest.slice(1).map(Number);
      const result = await page.evaluate(({r,x0,y0,x1,y1,ms,unit}) => new Promise(resolve => {
        const canvas = document.querySelector('canvas')!;
        let start: number | undefined; let frames = 0;
        const frame = (time: number) => {
          start ??= time;
          const t = Math.min((time-start)/ms,1);
          const q = t < .5 ? t*2 : (1-t)*2;
          const props = {clientX:r.x+(x0+(x1-x0)*q)*unit,clientY:r.y+(y0+(y1-y0)*q)*unit,pointerType:'mouse',pointerId:1,buttons:1,bubbles:true};
          // The app binds one of these two channels according to raw-input policy.
          canvas.dispatchEvent(new PointerEvent('pointerrawupdate',props));
          canvas.dispatchEvent(new PointerEvent('pointermove',props));
          frames++;
          if(t<1)requestAnimationFrame(frame);else resolve({producer_frames:frames,elapsed_ms:time-start});
        };
        requestAnimationFrame(frame);
      }), {r,x0,y0,x1,y1,ms,unit:panelUnit});
      return out(`praf ${JSON.stringify(result)}`);
    }
    case "pwheel": {
      // pwheel <panel> x y dx dy count ms: `count` wheel events at a panel
      // point, `ms` apart (hold Control first for a pinch-zoom).
      const r = await panelRect(rest[0]);
      const [x, y, dx, dy, count, ms] = rest.slice(1).map(Number);
      await page.mouse.move(r.x + x * panelUnit, r.y + y * panelUnit);
      for (let i = 0; i < count; i++) {
        await page.mouse.wheel(dx, dy);
        await page.waitForTimeout(ms);
      }
      return;
    }
    case "measure-reset": {
      // Start a measured stretch: clear the cadence probe and the phase timings.
      await page.evaluate(async () => {
        (window as any).__probeReset();
        await (window as any).__iron.timings(true);
      });
      return;
    }
    case "measure": {
      // measure <label>: one JSON line of what happened since `measure-reset`:
      // frames per second and worker tick time (cadence probe), and each
      // render phase's CPU time (src/perf.rs).
      const label = rest.join(" ");
      const m = await page.evaluate(async () => {
        const w = window as any;
        w.__lastProbeStats = null;
        w.__probe();
        for (let i = 0; i < 40 && !w.__lastProbeStats; i++) await new Promise((r) => setTimeout(r, 25));
        const s = w.__lastProbeStats ?? {};
        const t = await w.__iron.timings(true);
        const phase = (p: string) => t.phases[p];
        return {
          fps: s.effectiveFps,
          frames: s.frames,
          elapsed_ms: s.elapsedMs,
          tick_p50: s.tickMs?.p50,
          tick_p95: s.tickMs?.p95,
          raf_delta: s.rafDeltaMs,
          raf_buckets: s.rafDeltaBuckets,
          display_list: phase("display_list"),
          encode: phase("encode"),
          overlay_encode: phase("overlay_encode"),
          render: phase("render"),
          extract: phase("extract"),
          gesture_reconcile: phase("gesture_reconcile"),
          sync: phase("sync"),
          pick: phase("pick"),
          overlay_geometry: phase("overlay_geometry"),
          bounds: phase("bounds"),
          propagate: phase("propagate"),
          bounds_updated: t.bounds_updated,
          pick_candidates: t.pick_candidates,
          frame: phase("frame"),
          changed_frame: phase("changed_frame"),
          commands: t.commands,
          culled: t.culled,
          drawn: t.drawn,
          // Raster tiles (src/vector/raster.rs): bake time, and the counts
          // without the per-view picture.
          bake: phase("bake"),
          raster: t.raster && (({ views, ...rest }: any) => rest)(t.raster),
        };
      });
      return out(`measure ${label} ${JSON.stringify(m)}`);
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
    case "clickel": {
      // Click the element a CSS selector names, as a pointer would: Playwright
      // refuses when another element (the canvas) would receive the click.
      await page.locator(rest.join(" ")).first().click({ timeout: 5000 });
      return out(`clicked ${rest.join(" ")}`);
    }
    case "pclick": {
      // Click inside a named panel, panel-relative.
      const r = await panelRect(rest[0]);
      const [x, y] = rest.slice(1).map(Number);
      await page.mouse.move(r.x + x * panelUnit, r.y + y * panelUnit);
      await page.waitForTimeout(200);
      await page.mouse.down();
      await page.waitForTimeout(200);
      await page.mouse.up();
      await page.waitForTimeout(200);
      return;
    }
    case "pdouble": {
      // Double-click inside a named panel, panel-relative: two clicks well
      // inside the select tool's 0.4 s (`pclick` takes 0.6 s each).
      const r = await panelRect(rest[0]);
      const [x, y] = rest.slice(1).map(Number);
      await page.mouse.move(r.x + x * panelUnit, r.y + y * panelUnit);
      await page.waitForTimeout(200);
      for (let i = 0; i < 2; i++) {
        await page.mouse.down();
        await page.waitForTimeout(30);
        await page.mouse.up();
        await page.waitForTimeout(60);
      }
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
    case "wheel":
      // wheel dx dy: scroll at the pointer (hold Control for a pinch-zoom).
      await page.mouse.wheel(n[0], n[1]);
      await page.waitForTimeout(400);
      return;
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
    case "assert": {
      if (await page.evaluate(rest.join(" ")) !== true) throw new Error("assertion failed");
      return out("assertion passed");
    }
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
    case "errors": {
      if (errors.length || late.length) commandFailed = true;
      const lateLines = late.splice(0).map((l) => `late: ${l}`);
      return out([...lateLines, errors.join("\n") || "(no page errors)"].join("\n"));
    }
    case "quit":
      await browser.close();
      process.exit(commandFailed ? 1 : 0);
    default:
      return out(`unknown command: ${cmd}`);
  }
}

const rl = createInterface({ input: process.stdin });
for await (const line of rl) {
  try {
    await run(line);
  } catch (e) {
    commandFailed = true;
    out(`error in "${line}": ${e instanceof Error ? e.message.split("\n")[0] : e}`);
  }
}
await browser.close();
if (commandFailed) process.exit(1);
