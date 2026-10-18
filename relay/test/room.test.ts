import {
  env,
  SELF,
  runInDurableObject,
  runDurableObjectAlarm,
} from 'cloudflare:test';
import { describe, expect, it } from 'vitest';
import type { Env as WorkerEnv } from '../src';
declare global {
  namespace Cloudflare {
    interface Env extends WorkerEnv {}
  }
}
const hello = (hosting: boolean, token: string, name = 'Listener') =>
  JSON.stringify({
    type: 'hello',
    name,
    version: 1,
    hosting,
    token: token.repeat(64),
  });
async function connect(code: string) {
  const response = await SELF.fetch(`https://relay.test/room/${code}`, {
    headers: { Upgrade: 'websocket' },
  });
  expect(response.status).toBe(101);
  const ws = response.webSocket!;
  ws.accept();
  return ws;
}
function next(
  ws: WebSocket,
  type: string,
  predicate: (message: any) => boolean = () => true,
): Promise<any> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      ws.removeEventListener('message', listener);
      reject(new Error(`No ${type}`));
    }, 3000);
    function listener(event: MessageEvent) {
      const m = JSON.parse(event.data as string);
      if (m.type === type && predicate(m)) {
        clearTimeout(timer);
        ws.removeEventListener('message', listener);
        resolve(m);
      }
    }
    ws.addEventListener('message', listener);
  });
}
describe('hibernating rooms', () => {
  it('routes only canonical room codes and provides a safe join page', async () => {
    expect((await SELF.fetch('https://relay.test/room/AB0I23')).status).toBe(
      404,
    );
    const page = await SELF.fetch('https://relay.test/join/ABCD23');
    expect(await page.text()).toContain('ytmlisten://join/ABCD23');
    expect(page.headers.get('Content-Security-Policy')).toContain(
      "default-src 'none'",
    );
    expect(env.ROOM).toBeDefined();
  });
  it('welcomes late joiners with the latest host state and rejects member sync', async () => {
    const host = await connect('PART23');
    let welcome = next(host, 'welcome');
    host.send(hello(true, 'a', 'Host'));
    expect((await welcome).role).toBe('host');
    const sync = next(host, 'sync');
    host.send(
      JSON.stringify({
        type: 'sync',
        frame: {
          video_id: 'dQw4w9WgXcQ',
          position: 12,
          playing: true,
          host_ts: Date.now(),
          seq: 1,
        },
      }),
    );
    await sync;
    const member = await connect('PART23');
    welcome = next(member, 'welcome');
    member.send(hello(false, 'b'));
    const joined = await welcome;
    expect(joined.role).toBe('member');
    expect(joined.last_sync.video_id).toBe('dQw4w9WgXcQ');
    expect(joined.members).toHaveLength(2);
    const closed = new Promise((resolve) =>
      member.addEventListener('close', resolve, { once: true }),
    );
    member.send(
      JSON.stringify({ type: 'sync', frame: { ...joined.last_sync, seq: 2 } }),
    );
    await closed;
    host.close();
  });
  it('does not let a new hosting credential claim an existing room', async () => {
    const host = await connect('SAFE23');
    let p = next(host, 'welcome');
    host.send(hello(true, 'a'));
    await p;
    const attacker = await connect('SAFE23');
    p = next(attacker, 'error');
    attacker.send(hello(true, 'b'));
    expect((await p).message).toContain('already has a host');
    host.close();
    attacker.close();
  });
  it('reclaims the host role only with the original random token', async () => {
    const host = await connect('BACK23');
    let p = next(host, 'welcome');
    host.send(hello(true, 'c'));
    const original = await p;
    const replacement = await connect('BACK23');
    p = next(replacement, 'welcome');
    replacement.send(hello(true, 'c'));
    const rejoined = await p;
    expect(rejoined.id).toBe(original.id);
    expect(rejoined.role).toBe('host');
    expect(rejoined.members).toHaveLength(1);
    replacement.close();
  });
  it('transfers host ownership and publishes updated roles', async () => {
    const host = await connect('PASS23');
    let p = next(host, 'welcome');
    host.send(hello(true, 'd', 'Host'));
    const original = await p;
    const member = await connect('PASS23');
    p = next(member, 'welcome');
    member.send(hello(false, 'e', 'Friend'));
    const joined = await p;
    const members = next(member, 'members', (m) =>
      m.members.some(
        (peer: any) => peer.id === joined.id && peer.role === 'host',
      ),
    );
    host.send(JSON.stringify({ type: 'host_transfer', member_id: joined.id }));
    const changed = await members;
    expect(changed.members.find((m: any) => m.id === joined.id).role).toBe(
      'host',
    );
    expect(changed.members.find((m: any) => m.id === original.id).role).toBe(
      'member',
    );
    const sync = next(member, 'sync');
    member.send(
      JSON.stringify({
        type: 'sync',
        frame: {
          video_id: 'dQw4w9WgXcQ',
          position: 5,
          playing: false,
          host_ts: Date.now(),
          seq: 1,
        },
      }),
    );
    await sync;
    member.close();
    host.close();
  });
  it('notifies members when the host leaves and deletes an expired room', async () => {
    const code = 'GANE23';
    const host = await connect(code);
    let p = next(host, 'welcome');
    host.send(hello(true, 'f'));
    await p;
    const member = await connect(code);
    p = next(member, 'welcome');
    member.send(hello(false, 'a'));
    await p;
    const away = next(member, 'host_away');
    host.close(1000, 'Leaving');
    await away;
    const stub = env.ROOM.get(env.ROOM.idFromName(code));
    const expiry = await runInDurableObject(stub, async (instance, state) => {
      const saved = await state.storage.get<{ expires: number }>('room');
      // Fast-forward the deadline without waiting ten real minutes.
      const internal = instance as unknown as { room: { expires: number } };
      internal.room.expires = Date.now() - 1;
      await state.storage.put('room', internal.room);
      await state.storage.setAlarm(Date.now());
      return saved!.expires;
    });
    expect(expiry).toBeGreaterThan(Date.now() + 590000);
    await runDurableObjectAlarm(stub);
    expect(
      await runInDurableObject(
        stub,
        async (_instance, state) => (await state.storage.list()).size,
      ),
    ).toBe(0);
    member.close();
  });
});
