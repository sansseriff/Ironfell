import type { AdapterBridge } from './adapter_bridge';

const OVERRIDE_SCALE = false; // Keep in sync or pass via options if needed
const OVERRIDE_SCALE_FACTOR = 2;

export class ResizeManager {
    private bridge: AdapterBridge | null = null;
    private canvas: HTMLCanvasElement | null = null;
    private canvasId: string = '';
    private width = 0;
    private height = 0;
    private lastPhysicalWidth = -1;
    private lastPhysicalHeight = -1;
    private lastDpr = -1;
    private observerAttached = false;

    init(canvas: HTMLCanvasElement, bridge: AdapterBridge) {
        this.canvas = canvas;
        this.canvasId = canvas.id || 'unknown-canvas';
        this.bridge = bridge;
        this.attachObserver();
    }

    request(canvasId: string, width: number, height: number, force = false) {
        if (!this.canvas || !this.bridge) return;
        const dpr = OVERRIDE_SCALE ? OVERRIDE_SCALE_FACTOR : window.devicePixelRatio;
        if (width > 0) this.width = width;
        if (height > 0) this.height = height;
        if (this.width <= 0 || this.height <= 0) return;

        if (force || this.canvas.style.width !== this.width + 'px') this.canvas.style.width = this.width + 'px';
        if (force || this.canvas.style.height !== this.height + 'px') this.canvas.style.height = this.height + 'px';

        const physicalWidth = Math.floor(this.width * dpr);
        const physicalHeight = Math.floor(this.height * dpr);

        if (!force && physicalWidth === this.lastPhysicalWidth && physicalHeight === this.lastPhysicalHeight && dpr === this.lastDpr) return;

        this.lastPhysicalWidth = physicalWidth;
        this.lastPhysicalHeight = physicalHeight;
        this.lastDpr = dpr;
        this.bridge.post({ ty: 'resize', canvasId, width: physicalWidth, height: physicalHeight });
    }

    private attachObserver() {
        if (this.observerAttached || !this.canvas) return;
        try {
            const ro = new ResizeObserver((entries) => {
                for (const entry of entries) {
                    if (entry.target === this.canvas) {
                        const rect = this.canvas.getBoundingClientRect();
                        this.request(this.canvasId, Math.round(rect.width), Math.round(rect.height));
                    }
                }
            });
            ro.observe(this.canvas);
            (this.canvas as any).__iron_resize_ro = ro;
            this.observerAttached = true;
        } catch { }

        window.addEventListener('resize', () => {
            const dprNow = OVERRIDE_SCALE ? OVERRIDE_SCALE_FACTOR : window.devicePixelRatio;
            if (dprNow !== this.lastDpr) {
                if (this.width === 0 || this.height === 0) {
                    const rect = this.canvas!.getBoundingClientRect();
                    this.width = rect.width; this.height = rect.height;
                }
                this.request(this.canvasId, this.width, this.height, true);
            }
        }, { passive: true });
    }
}
