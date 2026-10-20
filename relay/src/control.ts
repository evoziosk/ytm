import { DurableObject } from 'cloudflare:workers';
import type { Env } from './index';

interface Peer {
  id: string;
  role: 'pending' | 'host' | 'controller' | 'closed';
  hello?: unknown;
  deadline: number;
  count: number;
  window: number;
}
interface Session {
  token: string;
  expires: number | null;
}
const EXPIRE = 600_000;
const key = (value: unknown): value is string =>
  typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);

/** Separate from synchronized rooms. Stores routing/admission metadata, never playback authority. */
export class ControlSession extends DurableObject<Env> {
  private session: Session | null = null;
  readonly ready: Promise<void>;
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.ready = ctx.blockConcurrencyWhile(async () => {
      this.session = (await ctx.storage.get<Session>('control')) ?? null;
    });
  }
  private peer(ws: WebSocket): Peer {
    return ws.deserializeAttachment() as Peer;
  }
  private sockets() {
    return this.ctx
      .getWebSockets()
      .filter(
        (ws) =>
          ws.readyState === WebSocket.OPEN && this.peer(ws).role !== 'closed',
      );
  }
  private host() {
    return this.sockets().find((ws) => this.peer(ws).role === 'host');
  }
  private send(ws: WebSocket, message: unknown) {
    try {
      ws.send(JSON.stringify(message));
    } catch {
      /* close callback owns cleanup */
    }
  }
  private async schedule() {
    const deadlines = this.sockets()
      .filter((ws) => this.peer(ws).role === 'pending')
      .map((ws) => this.peer(ws).deadline);
    if (this.session?.expires) deadlines.push(this.session.expires);
    if (deadlines.length)
      await this.ctx.storage.setAlarm(Math.min(...deadlines));
    else await this.ctx.storage.deleteAlarm();
  }
  async fetch(request: Request): Promise<Response> {
    if (request.headers.get('Upgrade')?.toLowerCase() !== 'websocket')
      return new Response('WebSocket required', { status: 426 });
    const origin = request.headers.get('Origin');
    if (origin && origin !== new URL(request.url).origin)
      return new Response('Invalid origin', { status: 403 });
    if (this.sockets().length >= 21)
      return new Response('Session full', { status: 429 });
    if (this.session?.expires && this.session.expires <= Date.now())
      return new Response('Session expired', { status: 410 });
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    this.ctx.acceptWebSocket(server);
    server.serializeAttachment({
      id: crypto.randomUUID(),
      role: 'pending',
      deadline: Date.now() + 10_000,
      count: 0,
      window: Date.now(),
    } satisfies Peer);
    await this.schedule();
    return new Response(null, { status: 101, webSocket: client });
  }
  async webSocketMessage(ws: WebSocket, raw: string | ArrayBuffer) {
    const peer = this.peer(ws);
    const now = Date.now();
    if (!peer || peer.role === 'closed') return;
    if (
      typeof raw !== 'string' ||
      new TextEncoder().encode(raw).byteLength >
        (peer.role === 'host' ? 131072 : 65536)
    ) {
      ws.close(1009, 'Message too large');
      return;
    }
    if (now - peer.window >= 10_000) {
      peer.count = 0;
      peer.window = now;
    }
    if (++peer.count > (peer.role === 'host' ? 1000 : 40)) {
      ws.close(1008, 'Rate limit');
      return;
    }
    ws.serializeAttachment(peer);
    let message: any;
    try {
      message = JSON.parse(raw);
    } catch {
      ws.close(1008, 'Invalid JSON');
      return;
    }
    if (!message || typeof message !== 'object') {
      ws.close(1008, 'Invalid message');
      return;
    }
    if (this.session?.expires && this.session.expires <= now) {
      this.send(ws, { type: 'expired' });
      ws.close(1000, 'Expired');
      return;
    }
    if (peer.role === 'pending') {
      if (peer.deadline <= now) {
        ws.close(1008, 'Hello timeout');
        return;
      }
      if (message.type === 'host' && key(message.token)) {
        if (this.session && this.session.token !== message.token) {
          ws.close(1008, 'Host credential mismatch');
          return;
        }
        const previous = this.host();
        if (previous) {
          ws.close(1008, 'Host already connected');
          return;
        }
        this.session = { token: message.token, expires: null };
        peer.role = 'host';
        ws.serializeAttachment(peer);
        await this.ctx.storage.put('control', this.session);
        this.send(ws, { type: 'ready' });
        for (const other of this.sockets()) {
          const p = this.peer(other);
          if (p.role === 'controller')
            this.send(ws, {
              type: 'controller',
              connection_id: p.id,
              message: p.hello,
            });
        }
      } else if (
        message.type === 'hello' &&
        this.session &&
        key(message.session_id) &&
        key(message.credential) &&
        typeof message.name === 'string' &&
        message.name.length > 0 &&
        message.name.length <= 64 &&
        ['personal', 'group'].includes(message.mode) &&
        message.version === 1 &&
        typeof message.invitation === 'string' &&
        message.invitation.length <= 64
      ) {
        if (
          this.sockets().filter((s) => this.peer(s).role === 'controller')
            .length >= 20
        ) {
          ws.close(1008, 'Session full');
          return;
        }
        peer.role = 'controller';
        peer.hello = message;
        ws.serializeAttachment(peer);
        const host = this.host();
        if (host)
          this.send(host, {
            type: 'controller',
            connection_id: peer.id,
            message,
          });
        else this.send(ws, { type: 'host_away' });
      } else {
        ws.close(1008, 'Session not found or invalid hello');
        return;
      }
      await this.schedule();
      return;
    }
    if (peer.role === 'host') {
      if (message.type === 'end') {
        this.session = { token: this.session!.token, expires: Date.now() - 1 };
        await this.ctx.storage.put('control', this.session);
        for (const other of this.sockets()) {
          this.send(other, { type: 'ended' });
          const p = this.peer(other);
          p.role = 'closed';
          p.hello = undefined;
          other.serializeAttachment(p);
          other.close(1000, 'Sharing ended');
        }
        await this.ctx.storage.deleteAlarm();
        return;
      }
      if (
        message.type !== 'deliver' ||
        typeof message.connection_id !== 'string' ||
        !message.message ||
        typeof message.message !== 'object'
      ) {
        ws.close(1008, 'Invalid host envelope');
        return;
      }
      const target = this.sockets().find(
        (s) =>
          this.peer(s).role === 'controller' &&
          this.peer(s).id === message.connection_id,
      );
      if (target) {
        this.send(target, message.message);
        if (['ended', 'error'].includes(message.message.type))
          target.close(1000, 'Sharing ended');
      }
    } else if (peer.role === 'controller') {
      if (message.type === 'disconnect') {
        ws.close(1000, 'Disconnected');
        return;
      }
      if (message.type !== 'request') {
        ws.close(1008, 'Invalid controller envelope');
        return;
      }
      const host = this.host();
      if (host)
        this.send(host, {
          type: 'controller',
          connection_id: peer.id,
          message,
        });
      else this.send(ws, { type: 'host_away' });
    }
  }
  private async disconnected(ws: WebSocket) {
    const peer = this.peer(ws);
    if (!peer || peer.role === 'closed') return;
    const wasHost = peer.role === 'host';
    peer.role = 'closed';
    peer.hello = undefined;
    ws.serializeAttachment(peer);
    if (wasHost && this.session) {
      this.session.expires = Date.now() + EXPIRE;
      await this.ctx.storage.put('control', this.session);
      for (const other of this.sockets())
        this.send(other, { type: 'host_away' });
    } else {
      const host = this.host();
      if (host)
        this.send(host, { type: 'disconnected', connection_id: peer.id });
    }
    await this.schedule();
  }
  async webSocketClose(ws: WebSocket) {
    await this.disconnected(ws);
    ws.close(1000, 'Disconnected');
  }
  async webSocketError(ws: WebSocket) {
    await this.disconnected(ws);
    ws.close(1011, 'Connection error');
  }
  async alarm() {
    const now = Date.now();
    for (const ws of this.sockets())
      if (this.peer(ws).role === 'pending' && this.peer(ws).deadline <= now) {
        const p = this.peer(ws);
        p.role = 'closed';
        ws.serializeAttachment(p);
        ws.close(1008, 'Hello timeout');
      }
    if (this.session?.expires && this.session.expires <= now) {
      for (const ws of this.sockets()) {
        const p = this.peer(ws);
        p.role = 'closed';
        ws.serializeAttachment(p);
        this.send(ws, { type: 'expired' });
        ws.close(1000, 'Session expired');
      }
      // Keep a tombstone: a restart must create a fresh session ID, never adopt old guests.
      await this.ctx.storage.put('control', this.session);
      await this.ctx.storage.deleteAlarm();
      return;
    }
    await this.schedule();
  }
}
