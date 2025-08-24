import type { AdapterBridge } from './adapter_bridge';

const OVERRIDE_SCALE = false; // Keep in sync or pass via options if needed
const OVERRIDE_SCALE_FACTOR = 2;

export class ResizeManager {
    private bridge: AdapterBridge | null = null;
    private canvases = new Map<string, HTMLCanvasElement>();
    private sizes = new Map<string, { cssW: number; cssH: number; lastPhysW: number; lastPhysH: number; lastDpr: number }>();
    private observers = new Map<string, ResizeObserver>();

    initAll(canvases: Map<string, HTMLCanvasElement>, bridge: AdapterBridge) {
        this.bridge = bridge;
        this.canvases = new Map(canvases);
        // Attach observers per canvas for passive resize tracking
        for (const [id, canvas] of this.canvases) {
            this.attachObserver(id, canvas);
        }
        // Watch DPR changes globally and refresh all known sizes
        window.addEventListener('resize', () => {
            const dprNow = OVERRIDE_SCALE ? OVERRIDE_SCALE_FACTOR : window.devicePixelRatio;
            for (const [id, s] of this.sizes) {
                if (dprNow !== s.lastDpr) {
                    const canvas = this.canvases.get(id);
                    if (!canvas) continue;
                    if (!s.cssW || !s.cssH) {
                        const rect = canvas.getBoundingClientRect();
                        s.cssW = Math.round(rect.width);
                        s.cssH = Math.round(rect.height);
                    }
                    this.request(id, s.cssW, s.cssH, true);
                }
            }
        }, { passive: true });
    }

    request(canvasId: string, width: number, height: number, force = false) {
        if (!this.bridge) return;
        const canvas = this.canvases.get(canvasId);
        if (!canvas) return;
        const dpr = OVERRIDE_SCALE ? OVERRIDE_SCALE_FACTOR : window.devicePixelRatio;

        const state = this.sizes.get(canvasId) || { cssW: 0, cssH: 0, lastPhysW: -1, lastPhysH: -1, lastDpr: -1 };
        if (width > 0) state.cssW = width;
        if (height > 0) state.cssH = height;
        if (state.cssW <= 0 || state.cssH <= 0) return;

        if (force || canvas.style.width !== state.cssW + 'px') canvas.style.width = state.cssW + 'px';
        if (force || canvas.style.height !== state.cssH + 'px') canvas.style.height = state.cssH + 'px';

        const physicalWidth = Math.floor(state.cssW * dpr);
        const physicalHeight = Math.floor(state.cssH * dpr);

        if (!force && physicalWidth === state.lastPhysW && physicalHeight === state.lastPhysH && dpr === state.lastDpr) {
            this.sizes.set(canvasId, state);
            return;
        }

        state.lastPhysW = physicalWidth;
        state.lastPhysH = physicalHeight;
        state.lastDpr = dpr;
        this.sizes.set(canvasId, state);

        this.bridge.post({ ty: 'resize', canvasId, width: physicalWidth, height: physicalHeight });
    }

    private attachObserver(id: string, canvas: HTMLCanvasElement) {
        if (this.observers.has(id)) return;
        try {
            const ro = new ResizeObserver((entries) => {
                for (const entry of entries) {
                    if (entry.target === canvas) {
                        const rect = canvas.getBoundingClientRect();
                        this.request(id, Math.round(rect.width), Math.round(rect.height));
                    }
                }
            });
            ro.observe(canvas);
            (canvas as any).__iron_resize_ro = ro;
            this.observers.set(id, ro);
        } catch { /* ignore */ }
    }
}
