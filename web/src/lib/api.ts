// HTTP client. Identity is the signed session cookie set by the server at sign-in; this adds the
// device id and an idempotency key per command, and retries network failures with the same key
// so a command is never applied twice.

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

/** UUID without crypto.randomUUID, which browsers only provide on https/localhost. */
export function uid(): string {
  const b = new Uint8Array(16);
  crypto.getRandomValues(b);
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  const h = Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

function sessionValue(key: string, make: () => string): string {
  try {
    const v = sessionStorage.getItem(key);
    if (v) return v;
    const n = make();
    sessionStorage.setItem(key, n);
    return n;
  } catch {
    return make();
  }
}

export const clientId = sessionValue('rfops.client', () => `web-${uid().slice(0, 8)}`);

/** Stable per browser (sign-in lockouts are per name and device). */
function localValue(key: string, make: () => string): string {
  try {
    const v = localStorage.getItem(key);
    if (v) return v;
    const n = make();
    localStorage.setItem(key, n);
    return n;
  } catch {
    return make();
  }
}
export const deviceId = localValue('rfops.device', () => `dev-${uid().slice(0, 12)}`);

/** Open mode: the name this device picked, sent with every request for attribution. */
let operatorName = '';
export function setOperatorName(n: string) {
  operatorName = n;
}
function nameHeader(): Record<string, string> {
  // Header values must be ISO-8859-1; encode anything else.
  return operatorName ? { 'x-rfops-actor': /^[\x20-\x7e]*$/.test(operatorName) ? operatorName : encodeURIComponent(operatorName) } : {};
}

/** Called when the server says the session ended (signed out, expired, revoked). */
let onUnauthorized: (() => void) | null = null;
export function handleUnauthorized(fn: () => void) {
  onUnauthorized = fn;
}

export async function get<T>(path: string): Promise<T> {
  const res = await fetch(path, { headers: { accept: 'application/json', ...nameHeader() }, credentials: 'same-origin' });
  if (!res.ok) {
    if (res.status === 401) onUnauthorized?.();
    throw new ApiError(res.status, await errorText(res));
  }
  return res.json() as Promise<T>;
}

async function errorText(res: Response): Promise<string> {
  try {
    const j = await res.json();
    return j.error ?? res.statusText;
  } catch {
    return res.statusText;
  }
}

/** Send a command. Retries network errors up to 3 times with the same idempotency key. */
export async function send<T = unknown>(method: 'POST' | 'PATCH' | 'PUT' | 'DELETE', path: string, body?: unknown): Promise<T> {
  const key = uid();
  let lastErr: unknown;
  for (let attempt = 0; attempt < 4; attempt++) {
    if (attempt > 0) await new Promise((r) => setTimeout(r, 300 * attempt));
    let res: Response;
    try {
      res = await fetch(path, {
        method,
        credentials: 'same-origin',
        headers: {
          'content-type': 'application/json',
          'x-rfops-client': deviceId,
          'x-rfops-idempotency-key': key,
          ...nameHeader(),
        },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch (e) {
      lastErr = e;
      continue;
    }
    if (!res.ok) {
      // A failed sign-in is also a 401, but it isn't a lost session.
      if (res.status === 401 && !path.startsWith('/api/auth/')) onUnauthorized?.();
      throw new ApiError(res.status, await errorText(res));
    }
    if (res.status === 204) return undefined as T;
    const text = await res.text();
    return (text ? JSON.parse(text) : undefined) as T;
  }
  throw new ApiError(0, `Network error: ${String(lastErr)}`);
}

export const post = <T = unknown>(p: string, b?: unknown) => send<T>('POST', p, b ?? {});
export const patch = <T = unknown>(p: string, b: unknown) => send<T>('PATCH', p, b);
export const put = <T = unknown>(p: string, b: unknown) => send<T>('PUT', p, b);
export const del = (p: string) => send('DELETE', p);

/** POST a raw text body (CSV imports). Returns status and parsed JSON. */
export async function postText(path: string, body: string): Promise<{ status: number; json: any }> {
  const res = await fetch(path, {
    method: 'POST',
    credentials: 'same-origin',
    headers: {
      'content-type': 'text/csv',
      'x-rfops-client': deviceId,
      ...nameHeader(),
    },
    body,
  });
  if (res.status === 401) onUnauthorized?.();
  let json: any = null;
  try {
    json = await res.json();
  } catch {
    /* empty */
  }
  return { status: res.status, json };
}
