/** Image decoding and resizing away from the composer and rendering thread. */
import type { ModelImage } from './images';
import { MAX_IMAGE_BASE64 } from './images';
import resvgWasmUrl from '@resvg/resvg-wasm/index_bg.wasm?url';

const EDGE = 1568;
const THUMB_EDGE = 192;
const MAX_THUMB_BASE64 = 100_000;
let resvgReady: Promise<void> | undefined;
let svgFont: Promise<Uint8Array> | undefined;

function base64(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return btoa(binary);
}

async function encoded(blob: Blob): Promise<string> {
  return base64(new Uint8Array(await blob.arrayBuffer()));
}

function canvasFor(width: number, height: number): OffscreenCanvas {
  return new OffscreenCanvas(Math.max(1, Math.round(width)), Math.max(1, Math.round(height)));
}

async function svgBitmap(file: File): Promise<ImageBitmap> {
  const { Resvg, initWasm } = await import('@resvg/resvg-wasm');
  // A failed load clears its promise, so the next SVG retries instead of
  // failing for the rest of the session.
  resvgReady ??= initWasm(resvgWasmUrl).catch(error => {
    resvgReady = undefined;
    throw error;
  });
  await resvgReady;
  // The same Roboto the font catalog serves from public/fonts (OFL.txt there).
  svgFont ??= fetch(new URL(`${import.meta.env.BASE_URL}fonts/Roboto.ttf`, self.location.origin).href)
    .then(async response => {
      if (!response.ok) throw new Error('Could not load the SVG font.');
      return new Uint8Array(await response.arrayBuffer());
    })
    .catch(error => {
      svgFont = undefined;
      throw error;
    });
  const svg = new Uint8Array(await file.arrayBuffer());
  const options = { font: { fontBuffers: [await svgFont], defaultFontFamily: 'Roboto' } };
  let renderer = new Resvg(svg, options);
  try {
    if (!renderer.width || !renderer.height) throw new Error('SVG needs a viewBox or dimensions.');
    if (Math.max(renderer.width, renderer.height) > EDGE) {
      const fitTo = renderer.width >= renderer.height
        ? { mode: 'width' as const, value: EDGE }
        : { mode: 'height' as const, value: EDGE };
      const fitted = new Resvg(svg, { ...options, fitTo });
      renderer.free();
      renderer = fitted;
    }
    const rendered = renderer.render();
    try {
      const png = rendered.asPng();
      return await createImageBitmap(new Blob([png.slice().buffer], { type: 'image/png' }));
    } finally { rendered.free(); }
  } finally { renderer.free(); }
}

async function decode(file: File): Promise<ImageBitmap> {
  if (file.type === 'image/svg+xml' || /\.svg$/i.test(file.name)) return svgBitmap(file);
  if (file.type === 'image/heic' || file.type === 'image/heif' || /\.(heic|heif)$/i.test(file.name)) {
    // This entry point decodes HEIF in a worker; our outer worker also keeps
    // resizing and encoding off the UI thread.
    const { heicTo } = await import('heic-to/next');
    return heicTo({ blob: file, type: 'bitmap' });
  }
  return createImageBitmap(file);
}

async function normalize(file: File): Promise<ModelImage> {
  const bitmap = await decode(file);
  try {
    const scale = Math.min(1, EDGE / Math.max(bitmap.width, bitmap.height));
    const canvas = canvasFor(bitmap.width * scale, bitmap.height * scale);
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('Could not read this image.');
    ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    let mediaType: ModelImage['mediaType'] = 'image/png';
    let data = await encoded(await canvas.convertToBlob({ type: mediaType }));
    if (data.length > MAX_IMAGE_BASE64) {
      // A detailed photo needs JPEG; fill transparency with white first.
      ctx.globalCompositeOperation = 'destination-over';
      ctx.fillStyle = '#ffffff';
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      mediaType = 'image/jpeg';
      data = await encoded(await canvas.convertToBlob({ type: mediaType, quality: 0.85 }));
    }
    if (data.length > MAX_IMAGE_BASE64) throw new Error('This image is too detailed; crop it or use a smaller image.');

    const thumbScale = Math.min(1, THUMB_EDGE / Math.max(bitmap.width, bitmap.height));
    const thumbCanvas = canvasFor(bitmap.width * thumbScale, bitmap.height * thumbScale);
    const thumbCtx = thumbCanvas.getContext('2d');
    if (!thumbCtx) throw new Error('Could not make an image preview.');
    thumbCtx.fillStyle = '#ffffff';
    thumbCtx.fillRect(0, 0, thumbCanvas.width, thumbCanvas.height);
    thumbCtx.drawImage(bitmap, 0, 0, thumbCanvas.width, thumbCanvas.height);
    const thumb = await encoded(await thumbCanvas.convertToBlob({ type: 'image/jpeg', quality: 0.7 }));
    return { name: file.name.slice(0, 200) || 'Pasted image', mediaType, data, ...(thumb.length <= MAX_THUMB_BASE64 ? { thumb } : {}) };
  } finally { bitmap.close(); }
}

self.onmessage = async (event: MessageEvent<{ id: number; file: File }>) => {
  const { id, file } = event.data;
  try { self.postMessage({ id, image: await normalize(file) }); }
  catch (error) { self.postMessage({ id, error: error instanceof Error ? error.message : String(error) }); }
};
