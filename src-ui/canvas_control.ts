import type { RenderSession } from './runtime/render_session';

export type CanvasKind = 'viewer' | 'timeline' | 'other';

export class CanvasControl {
  readonly id: string;
  readonly kind: CanvasKind;
  private element: HTMLCanvasElement;
  private readonly isPrimary: boolean;
  private currentSize = { w: 0, h: 0 };
  private attached = false;

  constructor(id: string, kind: CanvasKind, element: HTMLCanvasElement, isPrimary: boolean) {
    this.id = id;
    this.kind = kind;
    this.element = element;
    this.isPrimary = isPrimary;
  }

  attach(session: RenderSession, dpr: number) {
    session.createWindow(this.element, this.id, this.kind, dpr, this.isPrimary);
    this.attached = true;
    // Immediately push a resize using current DOM size
    const rect = this.element.getBoundingClientRect();
    const w = Math.max(1, Math.floor(rect.width * (window.devicePixelRatio || 1)));
    const h = Math.max(1, Math.floor(rect.height * (window.devicePixelRatio || 1)));
    session.resizeWindow(this.id, w, h);
    // Re-issue on next frame to catch late layout
    requestAnimationFrame(() => {
      const r2 = this.element.getBoundingClientRect();
      const w2 = Math.max(1, Math.floor(r2.width * (window.devicePixelRatio || 1)));
      const h2 = Math.max(1, Math.floor(r2.height * (window.devicePixelRatio || 1)));
      session.resizeWindow(this.id, w2, h2);
    });
  }

  updateSize(session: RenderSession, w: number, h: number, force = false) {
    if (!this.attached) return;
    if (force || w !== this.currentSize.w || h !== this.currentSize.h) {
      this.currentSize = { w, h };
      session.resizeWindow(this.id, w, h);
    }
  }

  detach() {
    this.attached = false;
  }

  recreateDomCanvas(container: HTMLElement) {
    const newCanvas = document.createElement('canvas');
    newCanvas.id = this.id;
    newCanvas.className = this.element.className;
    newCanvas.style.cssText = this.element.style.cssText;
    if (this.element.hasAttribute('tabindex')) {
      newCanvas.setAttribute('tabindex', this.element.getAttribute('tabindex')!);
    }
    try {
      container.replaceChild(newCanvas, this.element);
    } catch {
      container.appendChild(newCanvas);
    }
    this.element = newCanvas;
  }

  getElement(): HTMLCanvasElement {
    return this.element;
  }
}


