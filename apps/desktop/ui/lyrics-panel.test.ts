import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { fromStore, writable } from 'svelte/store';
import Lyrics from './Lyrics.svelte';
import { call, initial, type Snapshot } from './api';

vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  call: vi.fn(),
}));
let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.innerHTML = '';
  vi.mocked(call).mockReset();
});
function song(id: string): Snapshot {
  const data = structuredClone(initial);
  data.player.track = {
    video_id: id,
    title: `Song ${id}`,
    artist: 'Artist',
    artwork: '',
    album: '',
    playlist_id: '',
  };
  data.player.duration = 120;
  return data;
}
function start() {
  const state = writable(song('A'));
  const current = fromStore(state);
  component = mount(Lyrics, {
    target: document.body,
    props: {
      get data() {
        return current.current;
      },
      position: 2,
    },
  });
  flushSync();
  return (data: Snapshot) => flushSync(() => state.set(data));
}
function find() {
  flushSync(() =>
    document.querySelector<HTMLButtonElement>('.secondary')!.click(),
  );
}
const result = (text: string) => ({
  syncedLyrics: `[00:01]${text}`,
  plainLyrics: null,
  instrumental: false,
});
function deferred() {
  let resolve!: (value: any) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
it('keeps manual lookup private by default and retains lyrics through ordinary player updates', async () => {
  vi.mocked(call).mockResolvedValue(result('Current lyrics') as any);
  const update = start();
  expect(call).not.toHaveBeenCalled();
  find();
  await vi.waitFor(() =>
    expect(document.querySelector('.current')?.textContent).toBe(
      'Current lyrics',
    ),
  );
  update({
    ...song('A'),
    player: { ...song('A').player, position: 8, ts: Date.now() },
  });
  expect(document.querySelector('.current')?.textContent).toBe(
    'Current lyrics',
  );
  expect(call).toHaveBeenCalledOnce();
});
it.each(['resolve', 'reject'] as const)(
  'ignores a stale request %s after changing tracks and allows a new lookup immediately',
  async (outcome) => {
    const pending = deferred();
    vi.mocked(call)
      .mockReturnValueOnce(pending.promise as any)
      .mockResolvedValue(result('B lyrics') as any);
    const update = start();
    find();
    update(song('B'));
    expect(
      document.querySelector<HTMLButtonElement>('.secondary')!.disabled,
    ).toBe(false);
    find();
    await vi.waitFor(() =>
      expect(document.querySelector('.current')?.textContent).toBe('B lyrics'),
    );
    if (outcome === 'resolve') pending.resolve(result('Old lyrics'));
    else pending.reject(new Error('Old failure'));
    await pending.promise.catch(() => {});
    flushSync();
    expect(document.querySelector('.current')?.textContent).toBe('B lyrics');
    expect(document.body.textContent).not.toContain('Old');
  },
);
it('rejects an old response even after returning to the same song', async () => {
  const old = deferred(),
    newer = deferred();
  vi.mocked(call)
    .mockReturnValueOnce(old.promise as any)
    .mockReturnValueOnce(newer.promise as any);
  const update = start();
  find();
  update(song('B'));
  update(song('A'));
  find();
  old.resolve(result('Stale A'));
  await old.promise;
  flushSync();
  expect(
    document.querySelector<HTMLButtonElement>('.secondary')!.disabled,
  ).toBe(true);
  expect(document.body.textContent).not.toContain('Stale A');
  newer.resolve(result('Fresh A'));
  await vi.waitFor(() =>
    expect(document.querySelector('.current')?.textContent).toBe('Fresh A'),
  );
});
it('automatically loads only after opt-in, reuses cached lyrics and lets manual refresh bypass the cache', async () => {
  vi.mocked(call)
    .mockResolvedValueOnce(result('A lyrics') as any)
    .mockResolvedValueOnce(result('B lyrics') as any)
    .mockResolvedValueOnce(result('Refreshed A') as any);
  const update = start();
  const toggle = document.querySelector<HTMLButtonElement>('[role="switch"]')!;
  flushSync(() => toggle.click());
  await vi.waitFor(() =>
    expect(document.querySelector('.current')?.textContent).toBe('A lyrics'),
  );
  update(song('B'));
  await vi.waitFor(() =>
    expect(document.querySelector('.current')?.textContent).toBe('B lyrics'),
  );
  update(song('A'));
  expect(document.querySelector('.current')?.textContent).toBe('A lyrics');
  expect(call).toHaveBeenCalledTimes(2);
  find();
  await vi.waitFor(() =>
    expect(document.querySelector('.current')?.textContent).toBe('Refreshed A'),
  );
  flushSync(() => toggle.click());
  update(song('C'));
  expect(call).toHaveBeenCalledTimes(3);
  expect(document.querySelector('.current')).toBeNull();
});
it('keeps host lyrics when the unrelated local player changes, and clears them on host changes', async () => {
  vi.mocked(call).mockResolvedValue(result('Host lyrics') as any);
  const update = start();
  const data = song('A');
  data.remote = {
    active: true,
    target: 'Host',
    message: 'Connected',
    participant: null,
    snapshot: { session_id: 'one' } as any,
    updated_at: 0,
  };
  update(data);
  find();
  await vi.waitFor(() =>
    expect(document.querySelector('.current')?.textContent).toBe('Host lyrics'),
  );
  update(structuredClone(data));
  expect(document.querySelector('.current')?.textContent).toBe('Host lyrics');
  update({
    ...data,
    remote: { ...data.remote!, snapshot: { session_id: 'two' } as any },
  });
  expect(document.querySelector('.current')).toBeNull();
  expect(call).toHaveBeenCalledOnce();
});
it('shows instrumental and lookup errors with a working retry', async () => {
  vi.mocked(call)
    .mockRejectedValueOnce(new Error('Offline'))
    .mockResolvedValueOnce({
      ...result(''),
      syncedLyrics: null,
      instrumental: true,
    } as any);
  start();
  find();
  await vi.waitFor(() =>
    expect(document.querySelector('[role="status"]')?.textContent).toBe(
      'Offline',
    ),
  );
  find();
  await vi.waitFor(() =>
    expect(document.querySelector('[role="status"]')?.textContent).toContain(
      'instrumental',
    ),
  );
});
