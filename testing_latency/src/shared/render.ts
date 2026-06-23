import type { TestOptions } from './protocol';
import type { Point } from './paths';

export function setupCanvas(canvas: HTMLCanvasElement, options: TestOptions): CanvasRenderingContext2D {
  canvas.width = Math.round(options.width * options.dpr);
  canvas.height = Math.round(options.height * options.dpr);
  canvas.style.width = `${options.width}px`;
  canvas.style.height = `${options.height}px`;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('2D canvas context is unavailable');
  ctx.setTransform(options.dpr, 0, 0, options.dpr, 0, 0);
  return ctx;
}

export function draw2D(
  ctx: CanvasRenderingContext2D,
  options: TestOptions,
  pointer: Point,
  rendered: Point,
  dragging: boolean,
) {
  ctx.clearRect(0, 0, options.width, options.height);
  ctx.fillStyle = options.background;
  ctx.fillRect(0, 0, options.width, options.height);
  drawGrid(ctx, options);
  drawReference(ctx, options, pointer);
  drawSquare(ctx, options, rendered, dragging);
}

export function drawGrid(ctx: CanvasRenderingContext2D, options: TestOptions) {
  ctx.save();
  ctx.strokeStyle = '#dfe3e8';
  ctx.lineWidth = 1;
  for (let x = 0; x <= options.width; x += 80) {
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, options.height);
    ctx.stroke();
  }
  for (let y = 0; y <= options.height; y += 80) {
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(options.width, y);
    ctx.stroke();
  }
  ctx.restore();
}

export function drawReference(ctx: CanvasRenderingContext2D, options: TestOptions, pointer: Point) {
  ctx.save();
  ctx.strokeStyle = options.reference;
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.moveTo(pointer.x - 12, pointer.y);
  ctx.lineTo(pointer.x + 12, pointer.y);
  ctx.moveTo(pointer.x, pointer.y - 12);
  ctx.lineTo(pointer.x, pointer.y + 12);
  ctx.stroke();
  ctx.restore();
}

export function drawSquare(
  ctx: CanvasRenderingContext2D,
  options: TestOptions,
  center: Point,
  dragging: boolean,
) {
  const half = options.squareSize / 2;
  ctx.save();
  ctx.fillStyle = dragging ? options.squareDragging : options.squareIdle;
  ctx.fillRect(center.x - half, center.y - half, options.squareSize, options.squareSize);
  ctx.strokeStyle = '#111827';
  ctx.lineWidth = 2;
  ctx.strokeRect(center.x - half, center.y - half, options.squareSize, options.squareSize);
  ctx.restore();
}

export function normalizePointer(event: PointerEvent, element: HTMLElement): Point {
  const rect = element.getBoundingClientRect();
  return { x: event.clientX - rect.left, y: event.clientY - rect.top };
}
