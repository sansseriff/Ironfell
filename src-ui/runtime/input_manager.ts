import type { AdapterBridge } from './adapter_bridge';

export interface InputManagerOptions {
    enableRaw?: boolean;
}

export class InputManager {
    private canvas: HTMLCanvasElement | null = null;
    private bridge: AdapterBridge | null = null;
    private latestPick: any[] = [];
    private latestX = 0;
    private latestY = 0;
    private options: InputManagerOptions;
    private keyPressed = new Set<string>();
    private keyFrameScheduled = false;
    private attached = false;

    constructor(options: InputManagerOptions = {}) {
        this.options = options;
    }

    getPick() { return this.latestPick; }
    // Set from Rust outbound notifications (optional UI display); no longer drives Rust logic.
    setPick(list: any[]) { this.latestPick = list; }

    init(canvas: HTMLCanvasElement, bridge: AdapterBridge) {
        if (this.attached) return;
        this.canvas = canvas;
        this.bridge = bridge;
        this.attachPointer();
        this.attachKeyboard();
        this.attached = true;
    }

    dispose() {
        // For brevity: skipping removing listeners (can be added if canvases churn)
        this.attached = false;
    }

    private post(data: any) { this.bridge?.post(data); }

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

        window.addEventListener('resize', () => refreshRect(true), { passive: true });
        window.addEventListener('scroll', () => refreshRect(true), { passive: true });
        try {
            const ro = new ResizeObserver(() => refreshRect(true));
            ro.observe(canvas);
            (canvas as any).__iron_input_ro = ro;
        } catch { }

        const moveMsg: any = { ty: 'mousemove', x: 0, y: 0 };
        const send = (cx: number, cy: number) => {
            refreshRect();
            moveMsg.x = cx - rect.left;
            moveMsg.y = cy - rect.top;
            this.latestX = moveMsg.x;
            this.latestY = moveMsg.y;
            this.latestPick = [];
            this.post(moveMsg);
        };

        const onPointerMove = (ev: PointerEvent) => {
            const coalesced = (ev as any).getCoalescedEvents ? (ev as any).getCoalescedEvents() : null;
            if (coalesced && coalesced.length > 1) {
                for (const e of coalesced) send(e.clientX, e.clientY);
                return;
            }
            send(ev.clientX, ev.clientY);
        };

        canvas.addEventListener('pointermove', onPointerMove, { passive: true });
        if (this.options.enableRaw) {
            canvas.addEventListener('pointerrawupdate', onPointerMove as any, { passive: true });
        }
        canvas.addEventListener('mousemove', (e) => onPointerMove(e as any), { passive: true });

        canvas.addEventListener('pointerdown', (e: PointerEvent) => {
            refreshRect();
            const x = e.clientX - rect.left; const y = e.clientY - rect.top;
            this.latestX = x; this.latestY = y;
            this.post({ ty: 'leftBtDown', x, y });
        });
        canvas.addEventListener('pointerup', () => this.post({ ty: 'leftBtUp' }));
        // Click now handled internally by Rust (selection on press)
    }

    private attachKeyboard() {
        window.addEventListener('keydown', (e) => this.onKeyDown(e));
        window.addEventListener('keyup', (e) => this.onKeyUp(e));
    }

    private onKeyDown(event: KeyboardEvent) {
        const key = event.key.toLowerCase();
        const valid = ["w", "a", "s", "d", "f", "shift", "g"];
        if (!valid.includes(key)) return;
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
