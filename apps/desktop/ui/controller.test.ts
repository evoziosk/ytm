import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';
import { afterEach, expect, it, vi } from 'vitest';

let dom: JSDOM;
function controller() {
  dom = new JSDOM(readFileSync('../controller/index.html', 'utf8'), {
    url: `http://192.168.1.2:9863/#session=${'a'.repeat(64)}&invite=${'b'.repeat(64)}&mode=group`,
    runScripts: 'outside-only',
  });
  class Socket {
    static OPEN = 1;
    static instances: Socket[] = [];
    readyState = 1;
    sent: any[] = [];
    onopen?: () => void;
    onmessage?: (event: { data: string }) => void;
    onclose?: () => void;
    constructor(public url: URL) {
      Socket.instances.push(this);
    }
    send(raw: string) {
      this.sent.push(JSON.parse(raw));
    }
    close() {
      this.readyState = 3;
      this.onclose?.();
    }
    receive(value: unknown) {
      this.onmessage?.({ data: JSON.stringify(value) });
    }
  }
  (dom.window as any).WebSocket = Socket;
  dom.window.eval(readFileSync('../controller/controller.js', 'utf8'));
  const document = dom.window.document;
  document
    .getElementById('connect')!
    .dispatchEvent(new dom.window.Event('submit', { cancelable: true }));
  const ws = Socket.instances[0];
  ws.onopen!();
  return { document, ws, Socket };
}
const peer = {
  id: 'peer',
  name: '<img onerror=alert(1)>',
  mode: 'group',
  approved: true,
  connected: true,
  permissions: {
    transport: true,
    skip: true,
    seek: true,
    queue: true,
    replace_queue: false,
    volume: false,
    catalog: true,
  },
};
const snapshot = {
  session_id: 'a'.repeat(64),
  host_name: '<script>host</script>',
  track: {
    video_id: 'dQw4w9WgXcQ',
    title: '<img onerror=alert(1)>',
    artist: 'Artist',
    artwork: '',
  },
  playing: true,
  position: 3,
  duration: 120,
  volume: 0.5,
  muted: false,
  buffering: false,
  queue_revision: 1,
  current_entry: 'one',
  participants: [peer],
};
afterEach(() => dom?.window.close());
it('discovers a bare LAN address and requests admission without an invitation secret', async () => {
  dom = new JSDOM(readFileSync('../controller/index.html', 'utf8'), {
    url: 'http://192.168.1.2:9863/',
    runScripts: 'outside-only',
  });
  const sent: any[] = [];
  let socket: any;
  class Socket {
    static OPEN = 1;
    readyState = 1;
    onopen?: () => void;
    constructor() {
      socket = this;
    }
    send(raw: string) {
      sent.push(JSON.parse(raw));
    }
    close() {}
  }
  (dom.window as any).WebSocket = Socket;
  (dom.window as any).AbortSignal = AbortSignal;
  const fetch = vi.fn().mockResolvedValue({
    ok: true,
    json: async () => ({
      version: 1,
      session_id: 'a'.repeat(64),
      mode: 'group',
      host_name: 'Living room',
    }),
  });
  (dom.window as any).fetch = fetch;
  dom.window.eval(readFileSync('../controller/controller.js', 'utf8'));
  dom.window.document
    .getElementById('connect')!
    .dispatchEvent(new dom.window.Event('submit', { cancelable: true }));
  await vi.waitFor(() => expect(socket).toBeDefined());
  socket.onopen();
  expect(fetch).toHaveBeenCalledWith(
    '/session',
    expect.objectContaining({ credentials: 'omit', cache: 'no-store' }),
  );
  expect(sent[0]).toMatchObject({
    type: 'hello',
    session_id: 'a'.repeat(64),
    invitation: '',
    mode: 'group',
  });
  expect(dom.window.document.getElementById('controls')!.hidden).toBe(true);
});
it('requires admission, keeps credentials out of URLs, renders metadata safely and disables host-only volume', () => {
  const { ws, document } = controller();
  expect(ws.url.href).toBe('ws://192.168.1.2:9863/ws');
  expect(ws.sent[0]).toMatchObject({
    type: 'hello',
    version: 1,
    mode: 'group',
  });
  expect(dom.window.location.hash).toBe('');
  expect(document.getElementById('controls')!.hidden).toBe(true);
  ws.receive({ type: 'admission', participant: { ...peer, approved: false } });
  expect(document.getElementById('controls')!.hidden).toBe(true);
  ws.receive({ type: 'state', snapshot });
  expect(document.getElementById('controls')!.hidden).toBe(false);
  expect((document.getElementById('volume') as HTMLInputElement).disabled).toBe(
    true,
  );
  expect((document.getElementById('clear') as HTMLButtonElement).disabled).toBe(
    true,
  );
  expect(document.getElementById('title')!.textContent).toBe(
    snapshot.track.title,
  );
  expect(document.getElementById('title')!.querySelector('img')).toBeNull();
});
it('drops uncertain commands on host loss and never resends them on reconnect', async () => {
  const { ws, document } = controller();
  ws.receive({ type: 'admission', participant: peer });
  ws.receive({ type: 'state', snapshot });
  document.getElementById('next')!.click();
  expect(
    ws.sent.filter((m) => m.request?.command.type === 'playback'),
  ).toHaveLength(1);
  ws.receive({ type: 'host_away' });
  await Promise.resolve();
  await Promise.resolve();
  expect(document.getElementById('controls')!.hidden).toBe(true);
  ws.receive({ type: 'admission', participant: peer });
  ws.receive({ type: 'state', snapshot });
  expect(
    ws.sent.filter((m) => m.request?.command.type === 'playback'),
  ).toHaveLength(1);
  document.getElementById('disconnect')!.click();
});
it('expires after ten minutes without a host', () => {
  const { ws, document } = controller();
  ws.receive({ type: 'admission', participant: peer });
  ws.receive({ type: 'state', snapshot });
  let expire: (() => void) | undefined;
  const original = dom.window.setTimeout.bind(dom.window);
  (dom.window as any).setTimeout = (callback: () => void, delay: number) => {
    if (delay === 600000) {
      expire = callback;
      return 12345;
    }
    return original(callback, delay);
  };
  ws.receive({ type: 'host_away' });
  expect(expire).toBeDefined();
  expire!();
  expect(document.getElementById('status')!.textContent).toContain('expired');
  expect(document.getElementById('controls')!.hidden).toBe(true);
});
