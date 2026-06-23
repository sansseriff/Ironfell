import { readSharedPointer, viewSharedPointer, type SharedPointerView } from '../shared/sab-pointer';
import type { Point } from '../shared/paths';
import type { TestOptions } from '../shared/protocol';
import { draw2D } from '../shared/render';

let ctx: OffscreenCanvasRenderingContext2D | null = null;
let options: TestOptions | null = null;
let pointerView: SharedPointerView | null = null;
let pointer: Point = { x: 0, y: 0 };
let rendered: Point = { x: 0, y: 0 };
let dragOffset: Point = { x: 0, y: 0 };
let dragging = false;
let previousButtons = 0;
let rafId: number | null = null;
let active = false;

self.onmessage = (event: MessageEvent) => {
  const data = event.data;
  switch (data.ty) {
    case 'init': {
      const canvas = data.canvas as OffscreenCanvas;
      options = data.options as TestOptions;
      pointerView = viewSharedPointer(data.pointerBuffer as SharedArrayBuffer);
      canvas.width = Math.round(options.width * options.dpr);
      canvas.height = Math.round(options.height * options.dpr);
      const nextCtx = canvas.getContext('2d');
      if (!nextCtx) throw new Error('Worker SAB 2D context is unavailable');
      ctx = nextCtx;
      ctx.setTransform(options.dpr, 0, 0, options.dpr, 0, 0);
      draw();
      rafId = requestAnimationFrame(frame);
      break;
    }
    case 'start': {
      pointer = data.pointer;
      rendered = data.rendered;
      dragging = false;
      previousButtons = 0;
      active = true;
      draw();
      break;
    }
    case 'stop':
      active = false;
      break;
    case 'dispose':
      if (rafId !== null) cancelAnimationFrame(rafId);
      break;
  }
};

function frame(now: number) {
  const snapshot = pointerView ? readSharedPointer(pointerView) : null;
  if (snapshot) {
    pointer = snapshot.point;
    const leftPressed = (snapshot.buttons & 1) === 1;
    const wasPressed = (previousButtons & 1) === 1;
    if (leftPressed && !wasPressed) {
      dragging = isInside(pointer);
      dragOffset = { x: pointer.x - rendered.x, y: pointer.y - rendered.y };
    } else if (!leftPressed && wasPressed) {
      dragging = false;
    }
    previousButtons = snapshot.buttons;
  }

  if (dragging) {
    rendered = { x: pointer.x - dragOffset.x, y: pointer.y - dragOffset.y };
  }
  draw();
  if (active) {
    self.postMessage({ ty: 'rendered', t: now, pointer, rendered });
  }
  rafId = requestAnimationFrame(frame);
}

function draw() {
  if (!ctx || !options) return;
  draw2D(ctx as unknown as CanvasRenderingContext2D, options, pointer, rendered, dragging);
}

function isInside(point: Point) {
  if (!options) return false;
  const half = options.squareSize / 2;
  return Math.abs(point.x - rendered.x) <= half && Math.abs(point.y - rendered.y) <= half;
}
