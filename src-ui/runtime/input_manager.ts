import type { UiRequest } from './protocol';
import { oppositeVectorBackendUrl } from './variant_policy';
import { beginCopy, canvasOwnsCopy, pastedFragment } from './clipboard';
import { isToolLetter } from './tool_keys';
import { commandsFor } from './commands.svelte';
import { chordOf } from './chords';

export interface InputPoster {
    post(data: UiRequest): void;
}

export interface InputManagerOptions {
    enableRaw?: boolean;
    /** A right-click on the canvas, in window and canvas CSS px: the shell
     *  asks the engine for that view's menu (CanvasMenu.svelte). */
    onMenu?: (client: { x: number; y: number }, canvas: { x: number; y: number }) => void;
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
            // When it left this thread, on the shared clock (cadence probe).
            moveMsg.t = performance.timeOrigin + performance.now();
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
            if (e.button === 0) {
                canvas.setPointerCapture?.(e.pointerId);
                this.post({ ty: 'leftBtDown', x, y });
            }
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
        this.listen(canvas, 'pointercancel', () => this.cancelInput());
        this.listen(window, 'blur', () => this.cancelInput());
        // A right-click opens Iron's menu, not the browser's.
        this.listen(canvas, 'contextmenu', (e: MouseEvent) => {
            e.preventDefault();
            const r = canvas.getBoundingClientRect();
            this.options.onMenu?.({ x: e.clientX, y: e.clientY }, { x: e.clientX - r.left, y: e.clientY - r.top });
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

    private cancelInput() {
        this.post({ ty: 'toolKey', key: 'Escape', m: 0 });
        this.post({ ty: 'leftBtUp' });
        this.post({ ty: 'middleBt', down: false });
        for (const key of this.keyPressed) this.post({ ty: 'keyup', key });
        this.keyPressed.clear();
        this.sendMods(0);
    }

    private attachKeyboard() {
        this.listen(window, 'keydown', (e: KeyboardEvent) => this.onKeyDown(e));
        this.listen(window, 'keyup', (e: KeyboardEvent) => this.onKeyUp(e));
        // Paste arrives as an event so the clipboard is readable without a
        // permission prompt; Cmd+V itself is left to the browser.
        this.listen(window, 'paste', (e: ClipboardEvent) => {
            const text = pastedFragment(e);
            if (text === null) return;
            e.preventDefault();
            this.post({ ty: 'paste', text });
        });
        // An .svg file dropped on the canvas imports where it was dropped:
        // the pointer moves there first, then the text arrives as a paste.
        this.listen(window, 'dragover', (e: DragEvent) => {
            if (this.dropTarget(e)) e.preventDefault();
        });
        this.listen(window, 'drop', (e: DragEvent) => {
            const at = this.dropTarget(e);
            const file = at && [...(e.dataTransfer?.files ?? [])].find((f) => f.type === 'image/svg+xml' || f.name.toLowerCase().endsWith('.svg'));
            if (!at) return;
            e.preventDefault();
            if (!file) return;
            void file.text().then((text) => {
                this.post({ ty: 'mousemove', x: at.x, y: at.y });
                this.post({ ty: 'paste', text });
            });
        });
    }

    /** Where a file drag is over the canvas, in canvas px; `null` elsewhere
     *  or when it carries no files. */
    private dropTarget(e: DragEvent): { x: number; y: number } | null {
        if (!this.canvas || !e.dataTransfer?.types.includes('Files')) return null;
        const rect = this.canvas.getBoundingClientRect();
        const x = e.clientX - rect.left, y = e.clientY - rect.top;
        return x >= 0 && y >= 0 && x <= rect.width && y <= rect.height ? { x, y } : null;
    }

    /** Select-tool shortcuts, sent as commands with their modifiers. */
    private static TOOL_KEYS = ['escape', 'enter', 'tab', 'arrowleft', 'arrowright', 'arrowup', 'arrowdown', 'delete', 'backspace'];

    private onKeyDown(event: KeyboardEvent) {
        const key = event.key.toLowerCase();
        this.sendMods(InputManager.mods(event));
        // Headless controls (menus, popovers, selects) consume their keys
        // before this window listener. Closing one must not also act on canvas.
        if (event.defaultPrevented) return;
        // Modal settings own the keyboard, including controls portalled out
        // of the dialog (select menus). Canvas shortcuts must stay idle.
        if (document.querySelector('[role="dialog"][data-state="open"], [role="alertdialog"][data-state="open"]')) return;
        const target = event.target as HTMLElement | null;
        const editing = !!target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable);
        // Tool keys: whatever the views' keymaps bind (context data,
        // contexts.md §2.7); by default Figma's V, P, R, O and T.
        const toolSwitch = isToolLetter(key) && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
        if (!editing && (InputManager.TOOL_KEYS.includes(key) || toolSwitch)) {
            event.preventDefault();
            this.post({ ty: 'toolKey', key, m: InputManager.mods(event) });
            return;
        }
        // Commands: a chord any view's `keys.commands` binds goes to the
        // engine, which picks the command by the keymap of the view under
        // the pointer (commands.svelte.ts). A command on the selection
        // leaves the key to the browser when the canvas has none, or when
        // page text is selected (Cmd+C copies that text); a clipboard
        // command starts the write here, inside the user gesture.
        const chord = editing ? null : chordOf(event);
        const bound = chord ? commandsFor(chord) : [];
        if (chord && bound.length) {
            const onSelection = bound.every((c) => c.selection);
            const copies = bound.some((c) => c.clip === 'write');
            if (!(onSelection || copies) || canvasOwnsCopy(event.target)) {
                event.preventDefault();
                if (event.repeat) return;
                const send = () => this.post({ ty: 'toolKey', key: `chord:${chord}`, m: 0 });
                if (copies) beginCopy(send);
                else send();
                // Without Cmd/Ctrl the key goes on to Bevy too (Shift+A also
                // moves a 3D camera left).
                if (event.metaKey || event.ctrlKey) return;
            }
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
        if (editing) return;
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
