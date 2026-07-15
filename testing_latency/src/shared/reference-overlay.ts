import type { Point } from './paths';
import type { LatencySample, TestOptions } from './protocol';
import { setupCanvas } from './render';

const REFERENCE_VISUAL_INSET_PX = 8;

export class ReferenceOverlay {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private pointer: Point = { x: 0, y: 0 };
  private reference: Point = { x: 0, y: 0 };
  private dragOffset: Point = { x: 0, y: 0 };
  private dragging = false;
  private pointerHistory: Array<{ t: number; point: Point }> = [];

  constructor(
    private readonly parent: HTMLElement,
    private readonly options: TestOptions,
  ) {
    this.canvas = document.createElement('canvas');
    this.canvas.className = 'reference-overlay';
    this.ctx = setupCanvas(this.canvas, options);
    this.parent.append(this.canvas);
    this.draw();
  }

  element() {
    return this.canvas;
  }

  start(initial: Point) {
    this.pointer = initial;
    this.reference = initial;
    this.dragOffset = { x: 0, y: 0 };
    this.dragging = false;
    this.pointerHistory = [];
    this.recordPointer(initial);
    this.draw();
  }

  pointerDown(point: Point, rendered: Point) {
    this.pointer = point;
    this.dragging = true;
    this.dragOffset = {
      x: point.x - rendered.x,
      y: point.y - rendered.y,
    };
    this.reference = {
      x: point.x - this.dragOffset.x,
      y: point.y - this.dragOffset.y,
    };
    this.recordPointer(point);
    this.draw();
  }

  pointerMove(point: Point) {
    this.pointer = point;
    this.recordPointer(point);
    if (this.dragging) {
      this.reference = {
        x: point.x - this.dragOffset.x,
        y: point.y - this.dragOffset.y,
      };
    }
    this.draw();
  }

  pointerUp(point: Point) {
    this.pointer = point;
    this.recordPointer(point);
    if (this.dragging) {
      this.reference = {
        x: point.x - this.dragOffset.x,
        y: point.y - this.dragOffset.y,
      };
    }
    this.dragging = false;
    this.draw();
  }

  snapshot(): { point: Point; dragging: boolean; speedPxPerMs: number } {
    return {
      point: this.reference,
      dragging: this.dragging,
      speedPxPerMs: this.speed(),
    };
  }

  dispose() {
    this.canvas.remove();
  }

  private recordPointer(point: Point) {
    const now = performance.now();
    this.pointerHistory.push({ t: now, point });
    this.pointerHistory = this.pointerHistory.filter((sample) => now - sample.t <= 140);
  }

  private speed() {
    if (this.pointerHistory.length < 2) return 0;
    const first = this.pointerHistory[0];
    const last = this.pointerHistory[this.pointerHistory.length - 1];
    const dt = last.t - first.t;
    if (dt <= 0) return 0;
    return Math.hypot(last.point.x - first.point.x, last.point.y - first.point.y) / dt;
  }

  private draw() {
    const ctx = this.ctx;
    ctx.clearRect(0, 0, this.options.width, this.options.height);
    if (!this.dragging) return;

    const referenceSize = Math.max(8, this.options.squareSize - REFERENCE_VISUAL_INSET_PX * 2);
    const half = referenceSize / 2;
    ctx.save();
    ctx.globalAlpha = 0.55;
    ctx.fillStyle = '#00d084';
    ctx.fillRect(this.reference.x - half, this.reference.y - half, referenceSize, referenceSize);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = '#003d26';
    ctx.lineWidth = 3;
    ctx.strokeRect(this.reference.x - half, this.reference.y - half, referenceSize, referenceSize);
    ctx.strokeStyle = '#00a3ff';
    ctx.beginPath();
    ctx.moveTo(this.pointer.x - 14, this.pointer.y);
    ctx.lineTo(this.pointer.x + 14, this.pointer.y);
    ctx.moveTo(this.pointer.x, this.pointer.y - 14);
    ctx.lineTo(this.pointer.x, this.pointer.y + 14);
    ctx.stroke();
    ctx.restore();
  }
}

export function relativeStats(samples: LatencySample[]) {
  const values = samples
    .map((sample) => sample.relativeLatencyMs)
    .filter((value): value is number => value !== null && value !== undefined && Number.isFinite(value))
    .sort((a, b) => a - b);
  return values;
}
