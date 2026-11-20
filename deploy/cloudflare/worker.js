// rfops-proxy: forwards every request for rfops.<domain> (WebSockets included) to the Render
// service named in the ORIGIN variable, e.g. "rfops.onrender.com". Changing ORIGIN to
// "rfops-standby.onrender.com" moves everyone to the standby in seconds (reliability §3.6).
// It adds nothing: no caching, no rewriting of bodies.

export default {
  async fetch(request, env) {
    const origin = (env.ORIGIN || '').trim();
    if (!origin) return new Response('rfops-proxy: ORIGIN is not set', { status: 503 });
    const url = new URL(request.url);
    url.hostname = origin;
    url.protocol = 'https:';
    url.port = '';
    // Same method, headers (cookies, Origin, Upgrade) and body. A WebSocket upgrade passes through.
    const upstream = new Request(url.toString(), request);
    return fetch(upstream, { cf: { cacheTtl: 0, cacheEverything: false } });
  },
};
