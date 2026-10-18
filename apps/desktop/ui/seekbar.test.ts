import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import { fromStore, writable } from 'svelte/store';
import { initial, send } from './api';
import SeekBar from './SeekBar.svelte';

vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  send: vi.fn().mockResolvedValue(undefined),
}));
let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  document.body.innerHTML = '';
  vi.clearAllMocks();
});
it('keeps the drag and committed time when playback updates arrive, then accepts confirmation', async () => {
  const now = Date.now();
  const player = writable({
    ...initial.player,
    track: {
      video_id: 'dQw4w9WgXcQ',
      title: '',
      artist: '',
      album: '',
      playlist_id: '',
      artwork: '',
    },
    duration: 240,
    position: 1,
    ts: now - 100,
  });
  const reactive = fromStore(player);
  component = mount(SeekBar, {
    target: document.body,
    props: {
      get player() {
        return reactive.current;
      },
      get position() {
        return reactive.current.position;
      },
      clock: now,
      locked: false,
      onError: vi.fn(),
    },
  });
  flushSync();
  const input = document.querySelector<HTMLInputElement>('input')!;
  flushSync(() => {
    input.value = '89';
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  flushSync(() => player.update((p) => ({ ...p, position: 2, ts: now })));
  expect(input.value).toBe('89');
  flushSync(() => input.dispatchEvent(new Event('change', { bubbles: true })));
  expect(send).toHaveBeenCalledExactlyOnceWith({ type: 'seek', position: 89 });
  flushSync(() => player.update((p) => ({ ...p, position: 3, ts: now + 100 })));
  expect(input.value).toBe('89');
  expect(document.querySelector('.progress')?.getAttribute('aria-busy')).toBe(
    'true',
  );
  flushSync(() =>
    player.update((p) => ({ ...p, position: 89.2, ts: now + 1000 })),
  );
  expect(input.value).toBe('89.2');
  expect(document.querySelector('.progress')?.getAttribute('aria-busy')).toBe(
    'false',
  );
});
