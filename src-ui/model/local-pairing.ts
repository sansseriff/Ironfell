import { localEndpoint } from './runtime-options';

export async function discoverLocalAgent(endpoint: string, fetcher: typeof fetch = fetch): Promise<string | null> {
  try {
    const response = await fetcher(new URL('/pair', localEndpoint(endpoint)), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Iron-Local-Pairing': '1' },
      cache: 'no-store', credentials: 'omit', redirect: 'error', referrerPolicy: 'no-referrer',
      signal: AbortSignal.timeout(1500),
    });
    if (!response.ok) return null;
    const result = await response.json();
    return typeof result.token === 'string' && result.token.length >= 32 && result.token.length <= 1024 ? result.token : null;
  } catch { return null; }
}
