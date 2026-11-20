import { get, post, send, handleUnauthorized, ApiError, clientId } from './api';
import type {
  Alert, BatteryStatus, Effective, Equipment, Id, Kit, Performer, Principal, ServerEvent, Snapshot, Song, SwapPayload, User, WsMessage,
} from './types';

function load(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function save(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* storage unavailable: keep in memory only */
  }
}

export interface Toast { id: number; text: string; kind: 'error' | 'info' | 'critical' }

class Store {
  snapshot = $state<Snapshot | null>(null);
  showId = $state<Id | null>(Number(load('rfops.show')) || null);
  /** Who this browser is signed in as (a person or an enrolled screen); null = signed out. */
  me = $state<Principal | null>(null);
  /** False until the first /api/auth/status answer. */
  authChecked = $state(false);
  /** Some Lead can sign in (else the server needs RFOPS_ADMIN_NAME / RFOPS_ADMIN_PIN). */
  adminExists = $state(true);
  /** The session ended while in use (shown on the sign-in screen). */
  sessionEnded = $state(false);
  dismissedSwaps = $state<number[]>([]);
  connected = $state(false);
  loaded = $state(false);
  lastSyncAt = $state(0);
  now = $state(Date.now());
  toasts = $state<Toast[]>([]);
  /** Bumped whenever a new critical alert arrives, so views can flash. */
  criticalPulse = $state(0);
  lastSeq = 0;

  // ---------- lookups ----------
  equipment = $derived(new Map<Id, Equipment>((this.snapshot?.equipment ?? []).map((e) => [e.id, e])));
  performers = $derived(new Map<Id, Performer>((this.snapshot?.performers ?? []).map((p) => [p.id, p])));
  users = $derived(new Map<Id, User>((this.snapshot?.users ?? []).map((u) => [u.id, u])));
  show = $derived(this.snapshot?.show ?? null);
  kits = $derived(new Map<Id, Kit>((this.show?.kits ?? []).map((k) => [k.id, k])));
  songs = $derived<Song[]>([...(this.show?.songs ?? [])].sort((a, b) => a.position - b.position));
  songById = $derived(new Map<Id, Song>(this.songs.map((s) => [s.id, s])));
  battery = $derived(new Map<Id, BatteryStatus>((this.show?.battery ?? []).map((b) => [b.equipment_id, b])));
  alerts = $derived<Alert[]>(this.snapshot?.alerts ?? []);
  operatorName = $derived(this.me?.name ?? '');
  user = $derived<User | null>(
    this.me?.kind === 'u' ? (this.snapshot?.users.find((u) => u.id === this.me!.id) ?? null) : null,
  );
  isScreen = $derived(this.me?.kind === 's');
  canEdit = $derived(!!this.me && this.me.role !== 'viewer');
  isAdmin = $derived(this.me?.role === 'admin');
  staleSeconds = $derived(this.lastSyncAt ? Math.floor((this.now - this.lastSyncAt) / 1000) : 0);
  showMode = $derived(this.show?.show.mode === 'show');
  /** Swaps from the last 3 minutes that this screen has not dismissed, newest first. */
  swapBanners = $derived(
    (this.show?.recent_swaps ?? [])
      .filter((e) => this.now - Date.parse(e.at) < 180_000 && !this.dismissedSwaps.includes(e.seq))
      .map((e) => ({ seq: e.seq, at: e.at, actor: e.actor, swap: e.payload as SwapPayload })),
  );

  private ws: WebSocket | null = null;
  private backoff = 500;
  private refreshing = false;
  private refreshAgain = false;
  private refreshTimer: ReturnType<typeof setTimeout> | null = null;
  private toastSeq = 0;

  start() {
    handleUnauthorized(() => {
      if (this.me) this.sessionEnded = true;
      this.signedOut();
    });
    setInterval(() => (this.now = Date.now()), 1000);
    void this.checkAuth();
  }

  /** Ask the server who we are; start syncing if signed in. */
  async checkAuth() {
    try {
      const st = await get<{ signed_in: Principal | null; admin_exists: boolean }>('/api/auth/status');
      this.adminExists = st.admin_exists;
      if (st.signed_in) this.signedIn(st.signed_in);
    } catch {
      setTimeout(() => void this.checkAuth(), 2000);
      return;
    } finally {
      this.authChecked = true;
    }
  }

  private async recheck() {
    try {
      const st = await get<{ signed_in: Principal | null }>('/api/auth/status');
      if (!st.signed_in) {
        if (this.me) this.sessionEnded = true;
        this.signedOut();
      } else this.connect();
    } catch {
      setTimeout(() => void this.recheck(), this.backoff);
    }
  }

  private signedIn(p: Principal) {
    this.me = p;
    this.sessionEnded = false;
    void this.refresh();
    if (!this.ws) this.connect();
  }

  private signedOut() {
    this.me = null;
    this.snapshot = null;
    this.loaded = false;
    const ws = this.ws;
    this.ws = null;
    ws?.close();
  }

  /** Sign in with name + PIN. */
  async login(name: string, pin: string): Promise<boolean> {
    try {
      await post('/api/auth/login', { name, pin });
      await this.checkAuth();
      return !!this.me;
    } catch (e) {
      this.toast(e instanceof Error ? e.message : String(e), 'error');
      return false;
    }
  }

  /** Sign in a helper with the 6-digit code from the Lead. */
  async join(code: string): Promise<boolean> {
    try {
      await post('/api/auth/join', { code });
      await this.checkAuth();
      return !!this.me;
    } catch (e) {
      this.toast(e instanceof Error ? e.message : String(e), 'error');
      return false;
    }
  }

  async logout() {
    try {
      await send('POST', '/api/auth/logout', {});
    } catch {
      /* ignore */
    }
    this.signedOut();
  }

  selectShow(id: Id | null) {
    this.showId = id;
    save('rfops.show', id === null ? null : String(id));
    void this.refresh();
  }

  async refresh() {
    if (this.refreshing) {
      this.refreshAgain = true;
      return;
    }
    this.refreshing = true;
    try {
      do {
        this.refreshAgain = false;
        const q = this.showId ? `?show_id=${this.showId}` : '';
        let snap: Snapshot;
        try {
          snap = await get<Snapshot>(`/api/snapshot${q}`);
        } catch (e) {
          if (e instanceof ApiError && e.status === 404 && this.showId) {
            this.selectShow(null);
            continue;
          }
          throw e;
        }
        if (!this.showId && snap.shows.length > 0) {
          // Pick the most recent show automatically on first load.
          this.showId = snap.shows[0].id;
          save('rfops.show', String(this.showId));
          this.refreshAgain = true;
        }
        this.snapshot = snap;
        this.lastSeq = Math.max(this.lastSeq, snap.seq);
        this.lastSyncAt = Date.now();
        this.loaded = true;
      } while (this.refreshAgain);
    } catch (e) {
      this.toast(`Could not load data: ${e instanceof Error ? e.message : String(e)}`, 'error');
    } finally {
      this.refreshing = false;
    }
  }

  private scheduleRefresh() {
    if (this.refreshTimer) return;
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = null;
      void this.refresh();
    }, 40);
  }

  private connect() {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    if (!this.me) return;
    const screen = location.hash.replace(/^#\/?/, '').split('?')[0] || 'backstage';
    const ws = new WebSocket(
      `${proto}://${location.host}/api/ws?after=${this.lastSeq}&client=${encodeURIComponent(clientId)}&screen=${encodeURIComponent(screen)}`,
    );
    this.ws = ws;
    let opened = false;
    ws.addEventListener('open', () => (opened = true));
    ws.onopen = () => {
      this.connected = true;
      this.backoff = 500;
    };
    ws.onmessage = (m) => {
      let msg: WsMessage;
      try {
        msg = JSON.parse(m.data);
      } catch {
        return;
      }
      if (msg.type === 'hello') {
        this.lastSyncAt = Date.now();
        if (msg.seq !== this.lastSeq) this.scheduleRefresh();
      } else if (msg.type === 'resync') {
        this.lastSeq = msg.seq;
        this.scheduleRefresh();
      } else if (msg.type === 'event') {
        this.onEvent(msg.event);
      }
    };
    ws.onclose = () => {
      this.connected = false;
      if (this.ws !== ws) return; // closed on purpose (signed out)
      this.ws = null;
      // Refused before opening: maybe the session ended. checkAuth reconnects if still signed in.
      if (!opened) {
        setTimeout(() => void this.recheck(), this.backoff);
        this.backoff = Math.min(this.backoff * 2, 5000);
        return;
      }
      setTimeout(() => this.connect(), this.backoff);
      this.backoff = Math.min(this.backoff * 2, 5000);
    };
    ws.onerror = () => ws.close();
  }

  private onEvent(e: ServerEvent) {
    if (e.seq <= this.lastSeq) return;
    this.lastSeq = e.seq;
    this.lastSyncAt = Date.now();
    const p = e.payload as Record<string, unknown> | null;
    if (e.type === 'swap.performed') {
      this.criticalPulse++;
      try {
        navigator.vibrate?.([120, 60, 120]);
      } catch {
        /* not supported */
      }
    }
    if (e.type === 'alert.opened' && p?.severity === 'critical') {
      this.criticalPulse++;
      this.toast(`CRITICAL: ${String(p.title)}`, 'critical');
      try {
        navigator.vibrate?.([200, 100, 200]);
      } catch {
        /* not supported */
      }
    }
    this.scheduleRefresh();
  }

  toast(text: string, kind: Toast['kind'] = 'info') {
    const id = ++this.toastSeq;
    this.toasts = [...this.toasts, { id, text, kind }];
    setTimeout(() => (this.toasts = this.toasts.filter((t) => t.id !== id)), kind === 'info' ? 2500 : 6000);
  }

  /** Run a command; blocked while offline or without permission. Errors become toasts. */
  async act<T>(fn: () => Promise<T>, okMessage?: string): Promise<T | undefined> {
    if (!this.connected) {
      this.toast('Offline — changes are blocked until the connection returns.', 'error');
      return undefined;
    }
    if (!this.canEdit) {
      this.toast('Your operator role cannot make changes.', 'error');
      return undefined;
    }
    try {
      const r = await fn();
      if (okMessage) this.toast(okMessage);
      this.scheduleRefresh();
      return r;
    } catch (e) {
      this.toast(e instanceof Error ? e.message : String(e), 'error');
      return undefined;
    }
  }

  // ---------- derived helpers ----------

  label(id: Id | null | undefined): string {
    if (id == null) return '—';
    return this.equipment.get(id)?.label ?? `#${id}`;
  }

  performerName(id: Id | null | undefined): string {
    if (id == null) return 'Stock';
    return this.performers.get(id)?.name ?? `#${id}`;
  }

  songTitle(id: Id | null | undefined, fallback = ''): string {
    if (id == null) return fallback;
    return this.songById.get(id)?.title ?? `#${id}`;
  }

  kitsFor(eff: Effective | null | undefined, performerId: Id): Kit[] {
    return (eff?.[String(performerId)] ?? []).map((k) => this.kits.get(k)).filter((k): k is Kit => !!k);
  }

  /** Active alerts that touch a performer or any item in their kits. */
  alertsFor(performerId: Id, kits: Kit[]): Alert[] {
    const items = new Set<Id>();
    for (const k of kits) for (const i of [k.base_id, k.carrier_id, k.element_id]) if (i != null) items.add(i);
    return this.alerts.filter((a) => a.performer_id === performerId || (a.equipment_id != null && items.has(a.equipment_id)));
  }

  advance(direction: 'next' | 'prev') {
    if (!this.showId) return;
    return this.act(() => post(`/api/shows/${this.showId}/advance`, { direction }));
  }
}

export const store = new Store();
