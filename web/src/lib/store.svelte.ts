import { get, post, send, setOperator, setToken, handleUnauthorized, ApiError } from './api';
import type {
  Alert, BatteryStatus, Effective, Equipment, Id, Kit, Performer, ServerEvent, Snapshot, Song, SwapPayload, User, WsMessage,
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
  operatorName = $state<string>(load('rfops.operator') ?? '');
  token = $state<string>(load('rfops.token') ?? '');
  /** Set when the server rejects this operator for lack of a PIN sign-in. */
  needsPin = $state(false);
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
  user = $derived<User | null>(this.snapshot?.users.find((u) => u.name === this.operatorName) ?? null);
  /** No users configured yet: anyone may act, and the first admin should be created. */
  bootstrap = $derived(this.loaded && (this.snapshot?.users.length ?? 0) === 0);
  canEdit = $derived(this.bootstrap ? !!this.operatorName : !!this.user && this.user.role !== 'viewer');
  isAdmin = $derived(this.bootstrap || this.user?.role === 'admin');
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
    setOperator(this.operatorName);
    setToken(this.token);
    handleUnauthorized(() => {
      this.needsPin = true;
      this.setToken('');
    });
    setInterval(() => (this.now = Date.now()), 1000);
    void this.refresh();
    this.connect();
  }

  setOperator(name: string) {
    this.operatorName = name;
    setOperator(name);
    save('rfops.operator', name || null);
  }

  setToken(t: string) {
    this.token = t;
    setToken(t);
    save('rfops.token', t || null);
  }

  /** Sign in (PIN may be empty for operators without one). */
  async login(name: string, pin: string): Promise<boolean> {
    try {
      const r = await post<{ token: string }>('/api/login', { name, pin });
      this.setOperator(name);
      this.setToken(r.token);
      this.needsPin = false;
      return true;
    } catch (e) {
      this.toast(e instanceof Error ? e.message : String(e), 'error');
      return false;
    }
  }

  async logout() {
    try {
      await send('POST', '/api/logout', {});
    } catch {
      /* ignore */
    }
    this.setToken('');
    this.setOperator('');
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
    const ws = new WebSocket(`${proto}://${location.host}/api/ws?after=${this.lastSeq}`);
    this.ws = ws;
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
      this.ws = null;
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
