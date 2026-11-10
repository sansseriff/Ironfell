/** Bounded raster attachments shared by the chat and model transports. */
export interface ModelImage {
  name: string;
  mediaType: 'image/png' | 'image/jpeg' | 'image/webp' | 'image/gif';
  data: string;
}
export const MAX_IMAGES = 4;
export const MAX_IMAGE_BASE64 = 2_800_000;
export function validImages(value: unknown): value is ModelImage[] {
  return Array.isArray(value) && value.length <= MAX_IMAGES && value.every(image =>
    image && typeof image.name === 'string' && image.name.length <= 200 &&
    ['image/png', 'image/jpeg', 'image/webp', 'image/gif'].includes(image.mediaType) &&
    typeof image.data === 'string' && image.data.length > 0 && image.data.length <= MAX_IMAGE_BASE64 &&
    image.data.length % 4 === 0 && /^[A-Za-z0-9+/]+={0,2}$/.test(image.data));
}
export function imageUrl(image: ModelImage): string {
  return `data:${image.mediaType};base64,${image.data}`;
}

/** Decode before accepting; normalize animation to its first frame and cap dimensions. */
export async function readChatImage(file: File): Promise<ModelImage> {
  if (!['image/png', 'image/jpeg', 'image/webp', 'image/gif'].includes(file.type)) throw new Error('Use PNG, JPEG, WebP or GIF images.');
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
    return { name: file.name.slice(0, 200) || 'Pasted image', mediaType, data };
  } finally { bitmap.close(); }
}
