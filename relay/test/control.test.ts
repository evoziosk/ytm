import {
  env,
  SELF,
  runInDurableObject,
  runDurableObjectAlarm,
} from 'cloudflare:test';
import { describe, expect, it } from 'vitest';
import type { Env as WorkerEnv } from '../src';
import { ControlSession } from '../src/control';
declare global {
  namespace Cloudflare {
    interface Env extends WorkerEnv {}
  }
}
const id = (letter: string) => letter.repeat(64);
async function connect(session: string, origin?: string) {
  return SELF.fetch(`https://relay.test/control/${session}`, {
    headers: { Upgrade: 'websocket', ...(origin ? { Origin: origin } : {}) },
  });
}
function next(ws: WebSocket, type: string): Promise<any> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      ws.removeEventListener('message', listener);
      reject(new Error(`No ${type}`));
    }, 3000);
    function listener(event: MessageEvent) {
      const m = JSON.parse(event.data as string);
      if (m.type === type) {
        clearTimeout(timer);
        ws.removeEventListener('message', listener);
        resolve(m);
      }
    }
    ws.addEventListener('message', listener);
  });
}
async function host(session: string) {
  const r = await connect(session);
  expect(r.status).toBe(101);
  const ws = r.webSocket!;
  ws.accept();
  const ready = next(ws, 'ready');
  ws.send(JSON.stringify({ type: 'host', token: id('f') }));
  await ready;
  return ws;
}
function hello(session: string, token = 'b') {
  return {
    type: 'hello',
    version: 1,
    session_id: session,
    invitation: id('c'),
    credential: id(token),
    mode: 'group',
    name: 'Guest',
  };
}
describe('single-player control relay', () => {
  it('isolates control routes and validates browser origins', async () => {
    expect((await SELF.fetch('https://relay.test/control/ABCD23')).status).toBe(
      404,
    );
    expect((await connect(id('a'), 'https://attacker.test')).status).toBe(403);
    const page = await SELF.fetch('https://relay.test/controller/');
    expect(page.status).toBe(200);
    expect(await page.text()).toContain('Request admission');
    expect(page.headers.get('Content-Security-Policy')).toContain(
      "frame-ancestors 'none'",
    );
  });
  it('routes admission and requests only to the live host and replies only to the target', async () => {
    const session = id('1'),
      h = await host(session),
      response = await connect(session),
      guest = response.webSocket!;
    guest.accept();
    let routed = next(h, 'controller');
    guest.send(JSON.stringify(hello(session)));
    const joined = await routed;
    expect(joined.message.type).toBe('hello');
    const reply = next(guest, 'admission');
    h.send(
      JSON.stringify({
        type: 'deliver',
        connection_id: joined.connection_id,
        message: {
          type: 'admission',
          participant: { id: 'peer', approved: false },
        },
      }),
    );
    expect((await reply).participant.approved).toBe(false);
    routed = next(h, 'controller');
    guest.send(
      JSON.stringify({
        type: 'request',
        request: { id: 'a'.repeat(32), command: { type: 'snapshot' } },
      }),
    );
    expect((await routed).message.request.command.type).toBe('snapshot');
    const closed = new Promise((resolve) =>
      guest.addEventListener('close', resolve, { once: true }),
    );
    guest.send(
      JSON.stringify({
        type: 'deliver',
        connection_id: 'x',
        message: { type: 'state' },
      }),
    );
    await closed;
    h.close();
  });
  it('reconstructs routing metadata from hibernating socket attachments', async () => {
    const session = id('2'),
      h = await host(session),
      r = await connect(session),
      guest = r.webSocket!;
    guest.accept();
    const routed = next(h, 'controller');
    guest.send(JSON.stringify(hello(session)));
    await routed;
    const stub = env.CONTROL.get(env.CONTROL.idFromName(session));
    const saved = await runInDurableObject(stub, async (_instance, state) => ({
      session: await state.storage.get('control'),
      roles: state.getWebSockets().map((ws) => ws.deserializeAttachment()),
    }));
    expect(saved.session).toEqual({ token: id('f'), expires: null });
    expect(
      saved.roles.some(
        (p: any) => p.role === 'controller' && p.hello.credential === id('b'),
      ),
    ).toBe(true);
    const recovered = next(h, 'controller');
    await runInDurableObject(stub, async (_instance, state) => {
      const revived = new ControlSession(state, env);
      await revived.ready;
      const socket = state
        .getWebSockets()
        .find((ws) => ws.deserializeAttachment().role === 'controller')!;
      await revived.webSocketMessage(
        socket,
        JSON.stringify({
          type: 'request',
          request: { id: 'c'.repeat(32), command: { type: 'snapshot' } },
        }),
      );
    });
    expect((await recovered).message.request.command.type).toBe('snapshot');
    h.close();
    guest.close();
  });
  it('disables controllers on host loss, permits authenticated host reconnect, and expires after ten minutes', async () => {
    const session = id('3'),
      h = await host(session),
      r = await connect(session),
      guest = r.webSocket!;
    guest.accept();
    const routed = next(h, 'controller');
    guest.send(JSON.stringify(hello(session)));
    await routed;
    let away = next(guest, 'host_away');
    h.close();
    await away;
    const stub = env.CONTROL.get(env.CONTROL.idFromName(session));
    expect(
      await runInDurableObject(
        stub,
        async (_instance, state) =>
          (await state.storage.get<{ expires: number }>('control'))!.expires,
      ),
    ).toBeGreaterThan(Date.now() + 590000);
    const attacker = (await connect(session)).webSocket!;
    attacker.accept();
    const closed = new Promise((resolve) =>
      attacker.addEventListener('close', resolve, { once: true }),
    );
    attacker.send(JSON.stringify({ type: 'host', token: id('e') }));
    await closed;
    const resumed = (await connect(session)).webSocket!;
    resumed.accept();
    const ready = next(resumed, 'ready');
    const replay = next(resumed, 'controller');
    resumed.send(JSON.stringify({ type: 'host', token: id('f') }));
    await ready;
    expect((await replay).message.type).toBe('hello');
    away = next(guest, 'host_away');
    resumed.close();
    await away;
    await runInDurableObject(stub, async (instance, state) => {
      const internal = instance as unknown as { session: { expires: number } };
      internal.session.expires = Date.now() - 1;
      await state.storage.put('control', internal.session);
      await state.storage.setAlarm(Date.now());
    });
    const expired = next(guest, 'expired');
    await runDurableObjectAlarm(stub);
    await expired;
    expect((await connect(session)).status).toBe(410);
  });
  it('bounds controller count separately from synchronized rooms', async () => {
    const session = id('4'),
      h = await host(session),
      guests: WebSocket[] = [];
    for (let n = 0; n < 20; n++) {
      const r = await connect(session),
        g = r.webSocket!;
      g.accept();
      const p = next(h, 'controller');
      g.send(
        JSON.stringify(
          hello(
            session,
            n.toString(16).padStart(2, '0').repeat(32).slice(0, 1),
          ),
        ),
      );
      await p;
      guests.push(g);
    }
    expect((await connect(session)).status).toBe(429);
    for (const g of guests) g.close();
    h.close();
  });
});
