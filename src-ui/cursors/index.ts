/**
 * The canvas cursors, Figma's: 256 px PNGs drawn for 8 device px per CSS px,
 * so a 32 px cursor sharp at any pixel ratio. The engine names a cursor
 * (`bevy_app::select::cursor_for`); this turns the name into CSS.
 *
 * Names: `default`, `grab`, `grabbing`, `pen`, `pen-add` (on a segment, to
 * add a point), `pen-close` (on the first point, to close), `pen-remove` (on
 * a point, to remove it), `point` (Cmd over a path's point: a drag pulls out
 * its handles), `bend` (Cmd over a segment: a drag bends it), `radius` (a
 * shape's corner or arc handle), `crosshair`, `resize:<axis degrees>` and `rotate:<degrees
 * toward the grabbed corner>`. The last two turn with the shape, as Figma's
 * do: `resize.png` and `rotate.png` are the arrows without their shadow,
 * rotated on a canvas and given the shadow again beneath, so it always falls
 * downward.
 */
import defaultUrl from './default.png';
import grabUrl from './grab.png';
import grabbingUrl from './grabbing.png';
import penUrl from './pen.png';
import bendUrl from './bend.png';
import penAddUrl from './pen-add.png';
import penCloseUrl from './pen-close.png';
import penRemoveUrl from './pen-remove.png';
import pointUrl from './point.png';
import radiusUrl from './radius.png';
import resizeUrl from './resize.png';
import rotateUrl from './rotate.png';
import { isNativeHost } from '../runtime/native_host';

/** Device px per CSS px the PNGs are drawn for. */
const DENSITY = 8;
/** The canvas the turned arrows are drawn on, and their hotspot, device px. */
const SIZE = 256;
const CENTRE = 128;
// Use ordinary 2x Retina rasters in WKWebView. Keep the 8x artwork for
// rendering/rotation and the browser, then hand the native host 2x pixels.
const NATIVE = isNativeHost();
const CURSOR_DENSITY = NATIVE ? 2 : DENSITY;

/** A fixed cursor: its image, hotspot (CSS px) and the keyword it stands in for. */
const fixed: Record<string, [string, number, number, string]> = {
  default: [defaultUrl, 4, 4, 'default'],
  grab: [grabUrl, 16, 16, 'grab'],
  grabbing: [grabbingUrl, 16, 16, 'grabbing'],
  pen: [penUrl, 4, 4, 'crosshair'],
  'pen-add': [penAddUrl, 4, 4, 'crosshair'],
  'pen-close': [penCloseUrl, 4, 4, 'crosshair'],
  'pen-remove': [penRemoveUrl, 4, 4, 'crosshair'],
  bend: [bendUrl, 4, 4, 'pointer'],
  point: [pointUrl, 4, 4, 'move'],
  radius: [radiusUrl, 4, 4, 'pointer'],
};

/** The turnable arrows: image, the angle it is drawn at, its loaded pixels. */
const turned: Record<string, { url: string; base: number; img?: HTMLImageElement }> = {
  // Its axis runs top-left to bottom-right.
  resize: { url: resizeUrl, base: 45 },
  // It curls round the top-right corner.
  rotate: { url: rotateUrl, base: -45 },
};
const cache = new Map<string, string>();
const waiting: (() => void)[] = [];
const nativeFixed = new Map<string, string>();
function encodeCursor(c: HTMLCanvasElement): string {
  if (!NATIVE) return c.toDataURL();
  const out = document.createElement('canvas');
  out.width = out.height = SIZE * CURSOR_DENSITY / DENSITY;
  out.getContext('2d')!.drawImage(c, 0, 0, out.width, out.height);
  return out.toDataURL();
}
if (NATIVE) for (const [name, [url]] of Object.entries(fixed)) {
  const img = new Image();
  img.onload = () => {
    const c = document.createElement('canvas'); c.width = c.height = SIZE;
    c.getContext('2d')!.drawImage(img, 0, 0);
    nativeFixed.set(name, encodeCursor(c));
    for (const f of waiting.splice(0)) f();
  };
  img.src = url;
}
for (const t of Object.values(turned)) {
  const img = new Image();
  img.onload = () => {
    t.img = img;
    for (const f of waiting.splice(0)) f();
  };
  img.src = t.url;
}

function imageSet(url: string, x: number, y: number, fallback: string): string {
  return `image-set(url("${url}") ${CURSOR_DENSITY}x) ${x} ${y}, ${fallback}`;
}

/** An arrow turned `deg` from how it is drawn, as a data URL; cached per degree. */
function turn(img: HTMLImageElement, deg: number): string {
  const key = `${img.src}@${deg}`;
  let url = cache.get(key);
  if (!url) {
    const c = document.createElement('canvas');
    c.width = c.height = SIZE;
    const g = c.getContext('2d')!;
    // Figma's shadow: 1 CSS px down, 0.75 px blur, half the arrow's alpha.
    g.shadowColor = 'rgba(0, 0, 0, 0.5)';
    g.shadowBlur = 12;
    g.shadowOffsetY = DENSITY;
    g.translate(CENTRE, CENTRE);
    g.rotate((deg * Math.PI) / 180);
    g.translate(-CENTRE, -CENTRE);
    g.drawImage(img, 0, 0);
    url = encodeCursor(c);
    cache.set(key, url);
  }
  return url;
}

/** A standard keyword for when the image cannot be used. */
function keyword(kind: string, deg: number): string {
  if (kind === 'rotate') return 'crosshair';
  const a = ((deg % 180) + 180) % 180;
  return ['ew-resize', 'nwse-resize', 'ns-resize', 'nesw-resize'][Math.floor((a + 22.5) / 45) % 4];
}

/**
 * Show the cursor `name` on `el`. The keyword goes first, so a browser that
 * rejects `image-set` in `cursor` keeps it; a turned arrow whose image is
 * still loading shows the keyword until it arrives.
 */
export function applyCursor(el: HTMLElement, name: string) {
  el.dataset.cursor = name;
  const f = fixed[name];
  if (f) {
    el.style.cursor = f[3];
    if (NATIVE && !nativeFixed.has(name)) {
      waiting.push(() => el.dataset.cursor === name && applyCursor(el, name));
      return;
    }
    el.style.cursor = imageSet(nativeFixed.get(name) ?? f[0], f[1], f[2], f[3]);
    return;
  }
  const [kind, arg] = name.split(':');
  const t = turned[kind];
  if (!t) {
    el.style.cursor = name;
    return;
  }
  const deg = Math.round(Number(arg));
  el.style.cursor = keyword(kind, deg);
  if (!t.img) {
    waiting.push(() => el.dataset.cursor === name && applyCursor(el, name));
    return;
  }
  el.style.cursor = imageSet(turn(t.img, deg - t.base), CENTRE / DENSITY, CENTRE / DENSITY, keyword(kind, deg));
}
