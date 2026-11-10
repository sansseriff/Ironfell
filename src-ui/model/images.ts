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
const THUMB_EDGE = 192;
const MEDIA_TYPES = ['image/png', 'image/jpeg', 'image/webp', 'image/gif'];
const BASE64 = /^[A-Za-z0-9+/]+={0,2}$/;

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

/** Decode before accepting; normalize animation to its first frame and cap dimensions. */
export async function readChatImage(file: File): Promise<ModelImage> {
  if (!MEDIA_TYPES.includes(file.type)) throw new Error('Use PNG, JPEG, WebP or GIF images.');
  if (file.size > 20_000_000) throw new Error('Images must be smaller than 20 MB.');
  const bitmap = await createImageBitmap(file);
  try {
    const scale = Math.min(1, 1568 / Math.max(bitmap.width, bitmap.height));
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(bitmap.width * scale));
    canvas.height = Math.max(1, Math.round(bitmap.height * scale));
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('Could not read this image.');
    ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    let mediaType: ModelImage['mediaType'] = 'image/png';
    let data = canvas.toDataURL(mediaType).split(',')[1];
    if (data.length > MAX_IMAGE_BASE64) {
      // Detailed photographs compress far better as JPEG. Composite transparency
      // onto white first so a transparent source never turns black.
      ctx.globalCompositeOperation = 'destination-over';
      ctx.fillStyle = '#ffffff';
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      mediaType = 'image/jpeg';
      data = canvas.toDataURL(mediaType, 0.85).split(',')[1];
    }
    if (data.length > MAX_IMAGE_BASE64) throw new Error('This image is too detailed; crop it or use a smaller image.');
    return { name: file.name.slice(0, 200) || 'Pasted image', mediaType, data, ...thumbnail(bitmap) };
  } finally { bitmap.close(); }
}

/** A small JPEG on white, for saved history (`previewOf`). */
function thumbnail(bitmap: ImageBitmap): { thumb?: string } {
  const scale = Math.min(1, THUMB_EDGE / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(bitmap.width * scale));
  canvas.height = Math.max(1, Math.round(bitmap.height * scale));
  const ctx = canvas.getContext('2d');
  if (!ctx) return {};
  ctx.fillStyle = '#ffffff';
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  const thumb = canvas.toDataURL('image/jpeg', 0.7).split(',')[1];
  return thumb && thumb.length <= MAX_THUMB_BASE64 ? { thumb } : {};
}
