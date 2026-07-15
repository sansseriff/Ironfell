export interface InputPoster {
    post(data: any): void;
}

export interface InputManagerOptions {
    enableRaw?: boolean;
}

/**
 * Binds pointer/keyboard/wheel input on the full-window canvas and forwards it to the
 * render session. All coordinates are canvas-relative CSS pixels; Rust converts to
 * physical pixels via the scale factor it was given at window creation.
 *
 * Fully rebindable: `dispose()` removes every listener so the manager can be re-attached
 * to a fresh canvas after a worker/main mode switch (the old canvas is replaced because
 * transferred canvases cannot be reused).
 */
export class InputManager {
    private canvas: HTMLCanvasElement | null = null;
    private poster: InputPoster | null = null;
    private latestPick: any[] = [];
    private options: InputManagerOptions;
    private keyPressed = new Set<string>();
    private keyFrameScheduled = false;
    private attached = false;

    // Bound handlers kept for removal on dispose
    private cleanups: Array<() => void> = [];

    constructor(options: InputManagerOptions = {}) {
        this.options = options;
    }

    getPick() { return this.latestPick; }
    setPick(list: any[]) { this.latestPick = list; }

    init(canvas: HTMLCanvasElement, poster: InputPoster) {
        // Rebind cleanly if already attached (e.g. after a mode switch).
        if (this.attached) this.dispose();
        this.canvas = canvas;
        this.poster = poster;
        this.attachPointer();
        this.attachKeyboard();
        this.attached = true;
    }

    dispose() {
        for (const cleanup of this.cleanups) {
            try { cleanup(); } catch { }
        }
        this.cleanups = [];
        this.keyPressed.clear();
        this.keyFrameScheduled = false;
        this.canvas = null;
        this.poster = null;
        this.attached = false;
    }

    private post(data: any) { this.poster?.post(data); }

    private listen<K extends keyof HTMLElementEventMap>(
        target: HTMLElement | Window,
        type: string,
        handler: (ev: any) => void,
        options?: AddEventListenerOptions
    ) {
        target.addEventListener(type, handler as any, options);
        this.cleanups.push(() => target.removeEventListener(type, handler as any, options as any));
    }

    private attachPointer() {
        const canvas = this.canvas!;
        let rect = canvas.getBoundingClientRect();
        let rectStamp = performance.now();
        const RECT_REFRESH_MS = 500;
        const refreshRect = (force = false) => {
            const now = performance.now();
            if (force || now - rectStamp > RECT_REFRESH_MS) {
                rect = canvas.getBoundingClientRect();
                rectStamp = now;
            }
        };

        this.listen(window, 'resize', () => refreshRect(true), { passive: true });
        this.listen(window, 'scroll', () => refreshRect(true), { passive: true });

        const moveMsg: any = { ty: 'mousemove', x: 0, y: 0 };
        const send = (cx: number, cy: number) => {
            refreshRect();
            moveMsg.x = cx - rect.left;
            moveMsg.y = cy - rect.top;
            this.latestPick = [];
            this.post(moveMsg);
        };

        const onPointerMove = (ev: PointerEvent) => {
            const coalesced = (ev as any).getCoalescedEvents ? (ev as any).getCoalescedEvents() : null;
            if (coalesced && coalesced.length > 0) {
                // Rust consumes one cursor position per rendered frame, so posting the
                // whole history only creates worker-queue backlog. Keep the newest point.
                const latest = coalesced[coalesced.length - 1];
                send(latest.clientX, latest.clientY);
                return;
            }
            send(ev.clientX, ev.clientY);
        };

        if (this.options.enableRaw && 'onpointerrawupdate' in window) {
            this.listen(canvas, 'pointerrawupdate', onPointerMove as any, { passive: true });
        } else {
            this.listen(canvas, 'pointermove', onPointerMove, { passive: true });
        }

        this.listen(canvas, 'pointerdown', (e: PointerEvent) => {
            refreshRect();
            const x = e.clientX - rect.left; const y = e.clientY - rect.top;
            this.post({ ty: 'leftBtDown', x, y });
        });
        this.listen(canvas, 'pointerup', () => this.post({ ty: 'leftBtUp' }));

        this.listen(canvas, 'wheel', (e: WheelEvent) => {
            e.preventDefault();
            this.post({ ty: 'mouseWheel', dx: e.deltaX, dy: e.deltaY, mode: e.deltaMode });
        }, { passive: false });
    }

    private attachKeyboard() {
        this.listen(window, 'keydown', (e: KeyboardEvent) => this.onKeyDown(e));
        this.listen(window, 'keyup', (e: KeyboardEvent) => this.onKeyUp(e));
    }

    private onKeyDown(event: KeyboardEvent) {
        const key = event.key.toLowerCase();
        const valid = ["w", "a", "s", "d", "f", "shift", "g", "control", "controlleft", " "]; // include space & control variants
        if (!valid.includes(key)) return;
        // Don't steal keystrokes from HTML form controls layered over the canvas
        const target = event.target as HTMLElement | null;
        if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) return;
        event.preventDefault();
        this.keyPressed.add(key);
        if (!this.keyFrameScheduled) {
            this.keyFrameScheduled = true;
            requestAnimationFrame(() => {
                this.keyPressed.forEach(k => this.post({ ty: 'keydown', key: k }));
                this.keyFrameScheduled = false;
            });
        }
    }

    private onKeyUp(event: KeyboardEvent) {
        const key = event.key.toLowerCase();
        if (this.keyPressed.has(key)) {
            this.keyPressed.delete(key);
            this.post({ ty: 'keyup', key });
        }
    }
}
