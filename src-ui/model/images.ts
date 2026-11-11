/** Bounded raster attachments shared by the chat and model transports. */
export interface ModelImage {
  name: string;
  mediaType: 'image/png' | 'image/jpeg' | 'image/webp' | 'image/gif';
  /** The image as base64; with `preview`, only a small JPEG of it. */
  data: string;
  /** A small JPEG (base64) made when attached: what saved history keeps once
   *  the full image no longer fits its budget (chat/history.ts). */
  thumb?: string;
  /** `data` is a saved thumbnail, not the image: shown, never sent to a model. */
  preview?: true;
}
export const MAX_IMAGES = 4;
export const MAX_IMAGE_BASE64 = 2_800_000;
const MAX_THUMB_BASE64 = 100_000;
const MEDIA_TYPES = ['image/png', 'image/jpeg', 'image/webp', 'image/gif'];
const BASE64 = /^[A-Za-z0-9+/]+={0,2}$/;
const HEIC = /\.(heic|heif)$/i;
const SVG = /\.svg$/i;

/** The leading bytes each format starts with, so a declared type is checked
 *  against what the data is. */
function signatureMatches(mediaType: string, data: string): boolean {
  let head: string;
  try { head = atob(data.slice(0, 24)); } catch { return false; }
  const bytes = Array.from(head, c => c.charCodeAt(0));
  const starts = (sig: number[], at = 0) => sig.every((b, i) => bytes[at + i] === b);
  switch (mediaType) {
    case 'image/png': return starts([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    case 'image/jpeg': return starts([0xff, 0xd8, 0xff]);
    case 'image/gif': return starts([0x47, 0x49, 0x46, 0x38]);
    case 'image/webp': return starts([0x52, 0x49, 0x46, 0x46]) && starts([0x57, 0x45, 0x42, 0x50], 8);
    default: return false;
  }
}

function validBase64(data: unknown, max: number): data is string {
  return typeof data === 'string' && data.length > 0 && data.length <= max && data.length % 4 === 0 && BASE64.test(data);
}

export function validImages(value: unknown): value is ModelImage[] {
  return Array.isArray(value) && value.length <= MAX_IMAGES && value.every(image =>
    image && typeof image.name === 'string' && image.name.length <= 200 &&
    MEDIA_TYPES.includes(image.mediaType) &&
    validBase64(image.data, MAX_IMAGE_BASE64) && signatureMatches(image.mediaType, image.data) &&
    (image.thumb === undefined || (validBase64(image.thumb, MAX_THUMB_BASE64) && signatureMatches('image/jpeg', image.thumb))) &&
    (image.preview === undefined || (image.preview === true && image.mediaType === 'image/jpeg' && image.data.length <= MAX_THUMB_BASE64)));
}
export function imageUrl(image: ModelImage): string {
  return `data:${image.mediaType};base64,${image.data}`;
}

/** What a model is sent: the image's bytes only, never a saved preview or its thumbnail. */
export function wireImages(images: readonly ModelImage[] | undefined): ModelImage[] {
  return (images ?? []).filter(image => !image.preview).map(({ name, mediaType, data }) => ({ name, mediaType, data }));
}

/** An image as saved once its full bytes are over the history budget: its thumbnail, if it has one. */
export function previewOf(image: ModelImage): ModelImage | null {
  if (image.preview) return image;
  return image.thumb ? { name: image.name, mediaType: 'image/jpeg', data: image.thumb, preview: true } : null;
}

type ImageReply = { id: number; image?: ModelImage; error?: string };
let processor: Worker | undefined;
let nextImageId = 0;
const pending = new Map<number, { resolve: (image: ModelImage) => void; reject: (error: Error) => void; timeout: ReturnType<typeof setTimeout> }>();

function failProcessor(error: Error) {
  processor?.terminate();
  processor = undefined;
  for (const task of pending.values()) { clearTimeout(task.timeout); task.reject(error); }
  pending.clear();
}

/** Decode and resize off the UI thread; output is always a model-supported raster. */
export function readChatImage(file: File): Promise<ModelImage> {
  const heic = file.type === 'image/heic' || file.type === 'image/heif' || HEIC.test(file.name);
  const svg = file.type === 'image/svg+xml' || SVG.test(file.name);
  if (!heic && !svg && !MEDIA_TYPES.includes(file.type)) return Promise.reject(new Error('Use PNG, JPEG, WebP, GIF, HEIC or SVG images.'));
  if (file.size > (svg ? 2_000_000 : 20_000_000)) return Promise.reject(new Error(svg ? 'SVG images must be smaller than 2 MB.' : 'Images must be smaller than 20 MB.'));
  if (!processor) {
    processor = new Worker(new URL('./image-processing.worker.ts', import.meta.url), { type: 'module' });
    processor.onmessage = (event: MessageEvent<ImageReply>) => {
      const task = pending.get(event.data.id);
      if (!task) return;
      pending.delete(event.data.id);
      clearTimeout(task.timeout);
      if (event.data.image) task.resolve(event.data.image);
      else task.reject(new Error(event.data.error ?? 'Could not read this image.'));
    };
    processor.onerror = () => failProcessor(new Error('Could not read this image.'));
    processor.onmessageerror = () => failProcessor(new Error('Could not read this image.'));
  }
  const id = ++nextImageId;
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => failProcessor(new Error('Image conversion took too long.')), 30_000);
    pending.set(id, { resolve, reject, timeout });
    processor!.postMessage({ id, file });
  });
}
