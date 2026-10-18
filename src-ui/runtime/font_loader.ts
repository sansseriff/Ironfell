/**
 * The web host's font service (plans/specs/text.md §4).
 *
 * The engine asks for font files (`send_fonts_from_worker`), most needed
 * first; this fetches them from the self-hosted catalog under
 * `${BASE_URL}fonts/`, at most two at a time, and hands the bytes to the
 * engine (`register_font`). Nothing waits on it: text lays out in the
 * default face until its own arrives, then re-measures.
 *
 * Files go through the Cache API, so a second visit reads them from disk
 * instead of the network. Runs in the worker; the main thread never sees
 * font bytes.
 */

export type FontRequest = { family: string; file: string };

const CACHE = "iron-fonts-v1";
const PARALLEL = 2;

export class FontLoader {
  private queue: FontRequest[] = [];
  private active = 0;
  private seen = new Set<string>();

  constructor(
    private register: (bytes: Uint8Array) => string,
    private loaded: (families: string[]) => void,
  ) {}

  /** Queue `fonts` ahead of anything still waiting: the engine sends the most needed first. */
  request(fonts: FontRequest[]) {
    const fresh = fonts.filter((f) => !this.seen.has(f.file));
    fresh.forEach((f) => this.seen.add(f.file));
    this.queue.unshift(...fresh);
    this.pump();
  }

  private pump() {
    while (this.active < PARALLEL && this.queue.length > 0) {
      const next = this.queue.shift()!;
      this.active++;
      this.load(next)
        .catch((e) => console.warn(`font ${next.family} (${next.file}) failed to load`, e))
        .finally(() => {
          this.active--;
          this.pump();
        });
    }
  }

  private async load(f: FontRequest) {
    const url = new URL(`${import.meta.env.BASE_URL}fonts/${f.file}`, self.location.origin).href;
    let response: Response | undefined;
    const cache = "caches" in self ? await caches.open(CACHE).catch(() => undefined) : undefined;
    response = await cache?.match(url);
    if (!response) {
      response = await fetch(url);
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      await cache?.put(url, response.clone()).catch(() => {});
    }
    const bytes = new Uint8Array(await response.arrayBuffer());
    const families = this.register(bytes);
    this.loaded(families ? families.split(", ") : [f.family]);
  }
}
