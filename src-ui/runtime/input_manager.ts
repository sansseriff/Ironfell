import { oppositeVectorBackendUrl } from './wasm_loader';

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

    /** Modifier bits for the select tool: Shift 1, Ctrl 2, Alt 4, Cmd 8. */
    private static mods(e: { shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean }): number {
        return (e.shiftKey ? 1 : 0) | (e.ctrlKey ? 2 : 0) | (e.altKey ? 4 : 0) | (e.metaKey ? 8 : 0);
    }
    private lastMods = 0;
    private sendMods(m: number) {
        if (m === this.lastMods) return;
        this.lastMods = m;
        this.post({ ty: 'modifiers', m });
    }

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
        const send = (cx: number, cy: number, ev?: PointerEvent) => {
            if (ev) this.sendMods(InputManager.mods(ev));
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
                send(latest.clientX, latest.clientY, ev);
                return;
            }
            send(ev.clientX, ev.clientY, ev);
        };

        if (this.options.enableRaw && 'onpointerrawupdate' in window) {
            this.listen(canvas, 'pointerrawupdate', onPointerMove as any, { passive: true });
        } else {
            this.listen(canvas, 'pointermove', onPointerMove, { passive: true });
        }

        // Buttons by `e.button`: 0 left (select, drag), 1 middle (pans a 2D
        // view, as in Figma). Right is not forwarded: it is not a left press.
        this.listen(canvas, 'pointerdown', (e: PointerEvent) => {
            refreshRect();
            const x = e.clientX - rect.left; const y = e.clientY - rect.top;
            this.sendMods(InputManager.mods(e));
            if (e.button === 0) this.post({ ty: 'leftBtDown', x, y });
            else if (e.button === 1) {
                e.preventDefault();
                // Keep the pan going when the pointer leaves the canvas.
                canvas.setPointerCapture?.(e.pointerId);
                this.post({ ty: 'middleBt', down: true });
            }
        });
        this.listen(canvas, 'pointerup', (e: PointerEvent) => {
            this.sendMods(InputManager.mods(e));
            if (e.button === 0) this.post({ ty: 'leftBtUp' });
            else if (e.button === 1) this.post({ ty: 'middleBt', down: false });
        });
        // A middle press must not start the browser's autoscroll.
        this.listen(canvas, 'mousedown', (e: MouseEvent) => {
            if (e.button === 1) e.preventDefault();
        });

        this.listen(canvas, 'wheel', (e: WheelEvent) => {
            e.preventDefault();
            // Ctrl/Cmd+wheel zooms a 2D view; a trackpad pinch arrives as
            // Ctrl+wheel too.
            this.sendMods(InputManager.mods(e));
            this.post({ ty: 'mouseWheel', dx: e.deltaX, dy: e.deltaY, mode: e.deltaMode });
        }, { passive: false });
    }

    private attachKeyboard() {
        this.listen(window, 'keydown', (e: KeyboardEvent) => this.onKeyDown(e));
        this.listen(window, 'keyup', (e: KeyboardEvent) => this.onKeyUp(e));
    }

    /** Select-tool shortcuts, sent as commands with their modifiers. */
    private static TOOL_KEYS = ['escape', 'enter', 'tab', 'arrowleft', 'arrowright', 'arrowup', 'arrowdown', 'delete', 'backspace'];

    private onKeyDown(event: KeyboardEvent) {
        const key = event.key.toLowerCase();
        this.sendMods(InputManager.mods(event));
        {
            const target = event.target as HTMLElement | null;
            const editing = !!target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable);
            const selectAll = key === 'a' && (event.metaKey || event.ctrlKey);
            // Tools, Figma's keys: V select, P pen.
            const toolSwitch = (key === 'p' || key === 'v') && !event.metaKey && !event.ctrlKey && !event.altKey;
            if (!editing && (InputManager.TOOL_KEYS.includes(key) || selectAll || toolSwitch)) {
                event.preventDefault();
                this.post({ ty: 'toolKey', key, m: InputManager.mods(event) });
                return;
            }
            // View zoom (Figma's keys): Shift+1 fit, Shift+2 selection,
            // Shift+0 100%, +/- in and out, alone or with Cmd/Ctrl (instead
            // of the browser's page zoom).
            const zoom = event.shiftKey && event.code === 'Digit1' ? 'zoomfit'
                : event.shiftKey && event.code === 'Digit2' ? 'zoomsel'
                : event.shiftKey && event.code === 'Digit0' ? 'zoom100'
                : !event.altKey && (key === '=' || key === '+') ? 'zoomin'
                : !event.altKey && key === '-' ? 'zoomout'
                : null;
            if (!editing && zoom) {
                event.preventDefault();
                this.post({ ty: 'toolKey', key: zoom, m: InputManager.mods(event) });
                return;
            }
        }
        // Editor commands are decided here and sent as commands, not keys:
        // Cmd/Ctrl+Z undoes, Cmd/Ctrl+Shift+Z or Ctrl+Y redoes.
        if ((event.metaKey || event.ctrlKey) && (key === 'z' || key === 'y')) {
            const target = event.target as HTMLElement | null;
            if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) return;
            event.preventDefault();
            if (event.repeat) return;
            const redo = key === 'y' || event.shiftKey;
            this.post({ ty: redo ? 'redo' : 'undo' });
            return;
        }
        // NOTE: keys are whitelisted twice — here and in `map_key_str_to_bevy_key`
        // in src/web_ffi.rs. A new shortcut must be added to both or it silently
        // never reaches Bevy.
        // f9 cycles the 2D vector backend (classic Vello <-> sparse strips).
        const valid = ["w", "a", "s", "d", "f", "f9", "shift", "g", "control", "controlleft", " "]; // include space & control variants
        if (!valid.includes(key)) return;
        if (key === 'f9') {
            // A held key must not schedule multiple reloads. `replace` avoids
            // filling browser history with backend transitions.
            if (!event.repeat) location.replace(oppositeVectorBackendUrl(new URL(location.href)));
            return;
        }
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
        this.sendMods(InputManager.mods(event));
        if (this.keyPressed.has(key)) {
            this.keyPressed.delete(key);
            this.post({ ty: 'keyup', key });
        }
    }
}
