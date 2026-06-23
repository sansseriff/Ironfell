import type { Point } from '../shared/paths';
import type { TestOptions } from '../shared/protocol';
import { draw2D } from '../shared/render';

let canvas: OffscreenCanvas | null = null;
let ctx: OffscreenCanvasRenderingContext2D | null = null;
let options: TestOptions | null = null;
let pointer: Point = { x: 0, y: 0 };
let rendered: Point = { x: 0, y: 0 };
let dragOffset: Point = { x: 0, y: 0 };
let dragging = false;
let rafId: number | null = null;
let active = false;

self.onmessage = (event: MessageEvent) => {
  const data = event.data;
  switch (data.ty) {
    case 'init': {
      const nextCanvas = data.canvas as OffscreenCanvas;
      const nextOptions = data.options as TestOptions;
      canvas = nextCanvas;
      options = nextOptions;
      nextCanvas.width = Math.round(nextOptions.width * nextOptions.dpr);
      nextCanvas.height = Math.round(nextOptions.height * nextOptions.dpr);
      const nextCtx = nextCanvas.getContext('2d');
      if (!nextCtx) throw new Error('Worker 2D context is unavailable');
      ctx = nextCtx;
      nextCtx.setTransform(nextOptions.dpr, 0, 0, nextOptions.dpr, 0, 0);
      draw();
      rafId = requestAnimationFrame(frame);
      break;
    }
    case 'start': {
      pointer = data.pointer;
      rendered = data.rendered;
      dragging = false;
      active = true;
      draw();
      break;
    }
    case 'stop': {
      active = false;
      break;
    }
    case 'pointerdown': {
      pointer = data.point;
      dragging = isInside(pointer);
      dragOffset = { x: pointer.x - rendered.x, y: pointer.y - rendered.y };
      break;
    }
    case 'pointermove': {
      pointer = data.point;
      break;
    }
    case 'pointerup': {
      pointer = data.point;
      dragging = false;
      break;
    }
    case 'dispose': {
      if (rafId !== null) cancelAnimationFrame(rafId);
      break;
    }
  }
};

function frame(now: number) {
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
