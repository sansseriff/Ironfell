// A slow Model for `query.txt` (run-iron): POST {k, n} to /wave?ms=800 and,
// that many ms later, get {xs, ys} with ys = sin(k x) over n points of
// 0..2π. /fail answers 500. Start: `bun run model-server.ts` (port 5199).
const cors = { 'access-control-allow-origin': '*', 'access-control-allow-headers': 'content-type', 'access-control-allow-methods': 'POST, OPTIONS' };
Bun.serve({
  port: Number(process.env.PORT ?? 5199),
  async fetch(req) {
    const url = new URL(req.url);
    if (req.method === 'OPTIONS') return new Response(null, { headers: cors });
    if (url.pathname === '/fail') return new Response('down', { status: 500, statusText: 'Model down', headers: cors });
    const { k = 1, n = 9 } = (await req.json().catch(() => ({}))) as { k?: number; n?: number };
    await Bun.sleep(Number(url.searchParams.get('ms') ?? 800));
    const xs = Array.from({ length: n }, (_, i) => (2 * Math.PI * i) / (n - 1));
    return Response.json({ k, xs, ys: xs.map((x) => Math.sin(k * x)) }, { headers: cors });
  },
});
console.log('model server on 5199');
