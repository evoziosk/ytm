import { DurableObject } from 'cloudflare:workers';
import {
  parseClientMessage,
  validRoom,
  type Member,
  type PartyMsg,
  type SyncFrame,
} from '@ytm/protocol';

export interface Env {
  ROOM: DurableObjectNamespace<Room>;
  RELEASES_URL: string;
}
interface Connection {
  id: string;
  token: string;
  member: Member | null;
  deadline: number;
  count: number;
  window: number;
}
interface RoomState {
  hostToken: string;
  hostId: string;
  lastSync: SyncFrame | null;
  expires: number | null;
}
const TEN_MINUTES = 600_000;
const json = (body: unknown, status = 200) => Response.json(body, { status });

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (request.method !== 'GET')
      return new Response('Method not allowed', { status: 405 });
    if (url.pathname === '/health') return json({ ok: true, protocol: 1 });
    const match = /^\/(room|join)\/([A-Z2-9]{6,8})$/.exec(url.pathname);
    if (!match || !validRoom(match[2]))
      return new Response('Not found', { status: 404 });
    const [, route, code] = match;
    if (route === 'join') {
      let configured: URL;
      try {
        configured = new URL(env.RELEASES_URL || 'https://github.com');
      } catch {
        configured = new URL('https://github.com');
      }
      const nonce = crypto.randomUUID().replaceAll('-', '');
      const releases =
        configured.protocol === 'https:'
          ? configured.href
          : 'https://github.com';
      const safeLink = releases.replace(
        /[&<>"']/g,
        (c) =>
          ({
            '&': '&amp;',
            '<': '&lt;',
            '>': '&gt;',
            '"': '&quot;',
            "'": '&#39;',
          })[c]!,
      );
      return new Response(
        `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Listen together · YTM Listen</title><style>body{background:#111715;color:#e5eee9;font:18px system-ui;display:grid;min-height:90vh;place-content:center;text-align:center}a{color:#9ce7c7}p{color:#a7b4ac}h1{font-size:42px}code{letter-spacing:8px}</style><main><p>YTM LISTEN</p><h1>Good music. Better together.</h1><p>You've been invited to room <code>${code}</code>.</p><p><a href="ytmlisten://join/${code}">Open YTM Listen →</a></p><p>Need the app? <a href="${safeLink}">Download from Releases</a></p><p>Everyone plays from their own YouTube Music account.</p></main><script nonce="${nonce}">window.location.href="ytmlisten://join/${code}";</script></html>`,
        {
          headers: {
            'Content-Type': 'text/html;charset=UTF-8',
            'Content-Security-Policy': `default-src 'none'; script-src 'nonce-${nonce}'; style-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'`,
            'Referrer-Policy': 'no-referrer',
            'X-Content-Type-Options': 'nosniff',
          },
        },
      );
    }
    if (request.headers.get('Upgrade')?.toLowerCase() !== 'websocket')
      return new Response('WebSocket required', { status: 426 });
    return env.ROOM.get(env.ROOM.idFromName(code)).fetch(request);
  },
} satisfies ExportedHandler<Env>;

export class Room extends DurableObject<Env> {
  private room: RoomState | null = null;
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    ctx.blockConcurrencyWhile(async () => {
      this.room = (await ctx.storage.get<RoomState>('room')) ?? null;
    });
  }
  private connection(ws: WebSocket): Connection {
    return ws.deserializeAttachment() as Connection;
  }
  private peers() {
    return this.ctx
      .getWebSockets()
      .filter(
        (ws) => ws.readyState === WebSocket.OPEN && this.connection(ws)?.member,
      );
  }
  private members() {
    return this.peers().map((ws) => this.connection(ws).member!);
  }
  private send(ws: WebSocket, msg: PartyMsg) {
    try {
      ws.send(JSON.stringify(msg));
    } catch {
      /* close/error callback removes it */
    }
  }
  private broadcast(msg: PartyMsg) {
    for (const ws of this.peers()) this.send(ws, msg);
  }
  private async persist() {
    if (this.room) await this.ctx.storage.put('room', this.room);
  }
  private async schedule() {
    const deadlines = this.ctx
      .getWebSockets()
      .filter((ws) => ws.readyState === WebSocket.OPEN)
      .map((ws) => this.connection(ws))
      .filter((c) => c && !c.member)
      .map((c) => c.deadline);
    if (this.room?.expires) deadlines.push(this.room.expires);
    if (deadlines.length)
      await this.ctx.storage.setAlarm(Math.min(...deadlines));
    else await this.ctx.storage.deleteAlarm();
  }
  async fetch(request: Request): Promise<Response> {
    if (request.headers.get('Upgrade')?.toLowerCase() !== 'websocket')
      return new Response('WebSocket required', { status: 426 });
    if (this.room?.expires && this.room.expires <= Date.now())
      await this.alarm();
    // Allow a few pending reconnect handshakes beyond the established-member limit.
    if (this.ctx.getWebSockets().length >= 24)
      return new Response('Room full', { status: 429 });
    const pair = new WebSocketPair();
    this.ctx.acceptWebSocket(pair[1]);
    pair[1].serializeAttachment({
      id: crypto.randomUUID(),
      token: '',
      member: null,
      deadline: Date.now() + 10000,
      count: 0,
      window: Date.now(),
    } satisfies Connection);
    await this.schedule();
    return new Response(null, { status: 101, webSocket: pair[0] });
  }
  async webSocketMessage(ws: WebSocket, data: string | ArrayBuffer) {
    if (
      typeof data !== 'string' ||
      new TextEncoder().encode(data).length > 4096
    ) {
      ws.close(1009, 'Message too large');
      return;
    }
    const conn = this.connection(ws);
    if (!conn) {
      ws.close(1008, 'Missing session');
      return;
    }
    const now = Date.now();
    if (now - conn.window >= 10000) {
      conn.window = now;
      conn.count = 0;
    }
    conn.count++;
    ws.serializeAttachment(conn);
    if (conn.count > 40) {
      ws.close(1008, 'Rate limit');
      return;
    }
    const msg = parseClientMessage(data);
    if (!msg) {
      ws.close(1008, 'Invalid message or protocol version');
      return;
    }
    if (msg.type === 'hello') {
      if (conn.member) {
        ws.close(1008, 'Already welcomed');
        return;
      }
      if (!this.room && !msg.hosting) {
        this.send(ws, {
          type: 'error',
          message: 'Room not found. Ask the host for a new invite.',
        });
        ws.close(1008, 'Room not found');
        return;
      }
      if (!this.room)
        this.room = {
          hostToken: msg.token,
          hostId: conn.id,
          lastSync: null,
          expires: null,
        };
      if (msg.hosting && this.room.hostToken !== msg.token) {
        this.send(ws, {
          type: 'error',
          message: 'This room already has a host. Create another room.',
        });
        ws.close(1008, 'Room exists');
        return;
      }
      const previous = this.peers().find(
        (peer) => this.connection(peer).token === msg.token,
      );
      if (!previous && this.peers().length >= 20) {
        ws.close(1008, 'Room full');
        return;
      }
      if (previous) {
        const old = this.connection(previous);
        conn.id = old.id;
        old.member = null;
        previous.serializeAttachment(old);
        previous.close(1000, 'Reconnected');
      }
      const role = this.room.hostToken === msg.token ? 'host' : 'member';
      conn.token = msg.token;
      conn.member = {
        id: conn.id,
        name: msg.name.trim(),
        role,
        state: 'synced',
        drift_ms: 0,
      };
      if (role === 'host') {
        this.room.hostId = conn.id;
        this.room.expires = null;
      }
      ws.serializeAttachment(conn);
      await this.persist();
      this.send(ws, {
        type: 'welcome',
        id: conn.id,
        role,
        members: this.members(),
        last_sync: this.room.lastSync,
      });
      this.broadcast({ type: 'members', members: this.members() });
      if (this.room.expires) this.send(ws, { type: 'host_away' });
      await this.schedule();
      return;
    }
    if (!conn.member || !this.room) {
      ws.close(1008, 'Say hello first');
      return;
    }
    switch (msg.type) {
      case 'ping':
        this.send(ws, { type: 'pong', t0: msg.t0, server_t: now });
        break;
      case 'sync': {
        if (conn.member.role !== 'host' || this.room.hostToken !== conn.token) {
          ws.close(1008, 'Host only');
          return;
        }
        if (this.room.lastSync && msg.frame.seq <= this.room.lastSync.seq)
          return;
        const transit = Math.max(
          0,
          Math.min(10, (now - msg.frame.host_ts) / 1000),
        );
        this.room.lastSync = {
          ...msg.frame,
          position: msg.frame.position + (msg.frame.playing ? transit : 0),
          host_ts: now,
        };
        await this.persist();
        this.broadcast({ type: 'sync', frame: this.room.lastSync });
        break;
      }
      case 'status':
        conn.member.state = msg.state;
        conn.member.drift_ms = msg.drift_ms;
        ws.serializeAttachment(conn);
        this.broadcast({ type: 'members', members: this.members() });
        break;
      case 'host_transfer': {
        if (conn.member.role !== 'host' || this.room.hostToken !== conn.token) {
          ws.close(1008, 'Host only');
          return;
        }
        const target = this.peers().find(
          (peer) => this.connection(peer).id === msg.member_id,
        );
        if (!target || target === ws) return;
        const next = this.connection(target);
        next.member!.role = 'host';
        conn.member.role = 'member';
        this.room.hostToken = next.token;
        this.room.hostId = next.id;
        this.room.lastSync = null;
        ws.serializeAttachment(conn);
        target.serializeAttachment(next);
        await this.persist();
        this.broadcast({ type: 'members', members: this.members() });
        break;
      }
    }
  }
  private async disconnected(ws: WebSocket) {
    const conn = this.connection(ws);
    if (!conn?.member) return;
    const wasHost =
      conn.member.role === 'host' && this.room?.hostToken === conn.token;
    conn.member = null;
    ws.serializeAttachment(conn);
    if (wasHost && this.room) {
      this.room.expires = Date.now() + TEN_MINUTES;
      await this.persist();
      this.broadcast({ type: 'host_away' });
    }
    this.broadcast({ type: 'members', members: this.members() });
    await this.schedule();
  }
  async webSocketClose(ws: WebSocket, code: number) {
    // 1005/1006/1015 are receive-only sentinel codes and cannot be sent on the wire.
    const safe =
      code >= 1000 && code <= 4999 && ![1004, 1005, 1006, 1015].includes(code)
        ? code
        : 1000;
    ws.close(safe, 'Disconnected');
    await this.disconnected(ws);
  }
  async webSocketError(ws: WebSocket) {
    await this.disconnected(ws);
    ws.close(1011, 'Connection error');
  }
  async alarm() {
    const now = Date.now();
    for (const ws of this.ctx.getWebSockets()) {
      const c = this.connection(ws);
      if (!c.member && c.deadline <= now) ws.close(1008, 'Hello timeout');
    }
    if (this.room?.expires && this.room.expires <= now) {
      for (const ws of this.ctx.getWebSockets()) {
        const c = this.connection(ws);
        c.member = null;
        ws.serializeAttachment(c);
        ws.close(1000, 'Room expired');
      }
      this.room = null;
      await this.ctx.storage.deleteAll();
      return;
    }
    await this.schedule();
  }
}
