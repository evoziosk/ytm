import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mount, unmount, flushSync } from 'svelte';
import Music from './Music.svelte';
import { initial } from './api';
import { call } from './api';
import { fromStore, writable } from 'svelte/store';

vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  native: true,
  call: vi.fn().mockResolvedValue({
    title: 'Home',
    signed_in: false,
    message: '',
    continuation: '',
    playlist_id: '',
    items: [
      {
        key: 'song:dQw4w9WgXcQ',
        title: 'A song',
        subtitle: 'Artist',
        artist: 'Artist',
        kind: 'song',
        video_id: 'dQw4w9WgXcQ',
        browse_id: '',
        artwork: '',
      },
    ],
  }),
}));
let component: ReturnType<typeof mount> | undefined;
beforeEach(() => {
  vi.mocked(call).mockResolvedValue({
    title: 'Home',
    signed_in: false,
    message: '',
    continuation: '',
    playlist_id: '',
    items: [
      {
        key: 'dQw4w9WgXcQ',
        title: 'A song',
        subtitle: 'Artist',
        artist: 'Artist',
        kind: 'song',
        video_id: 'dQw4w9WgXcQ',
        browse_id: '',
        artwork: '',
      },
    ],
  } as any);
});
afterEach(async () => {
  if (component) await unmount(component);
  document.body.innerHTML = '';
  vi.clearAllMocks();
});
it('restores search results and collection Back history after switching tabs', async () => {
  const state = writable<'home' | 'search' | 'library' | 'queue'>('search');
  const current = fromStore(state);
  const album = {
    key: 'MPREabc',
    title: 'An album',
    subtitle: '',
    artist: '',
    artwork: '',
    kind: 'album',
    video_id: '',
    browse_id: 'MPREabc',
  };
  const song = {
    ...album,
    key: 'dQw4w9WgXcQ',
    title: 'Album track',
    kind: 'song',
    video_id: 'dQw4w9WgXcQ',
    browse_id: '',
  };
  vi.mocked(call).mockImplementation(async (command, args) => {
    const request = args?.request as { kind: string };
    return {
      title: '',
      signed_in: false,
      message: '',
      continuation: '',
      playlist_id: '',
      items: request.kind === 'browse' ? [song] : [album],
    } as any;
  });
  component = mount(Music, {
    target: document.body,
    props: {
      data: structuredClone(initial),
      get view() {
        return current.current;
      },
      onError: vi.fn(),
      onAccount: vi.fn(),
    },
  });
  flushSync();
  const input = document.querySelector<HTMLInputElement>('input')!;
  flushSync(() => {
    input.value = 'Test search';
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  document
    .querySelector('form')!
    .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  await vi.waitFor(() =>
    expect(document.querySelector('.music-card')?.textContent).toContain(
      'An album',
    ),
  );
  document.querySelector<HTMLButtonElement>('.music-card')!.click();
  await vi.waitFor(() =>
    expect(document.querySelector('.track-row')?.textContent).toContain(
      'Album track',
    ),
  );
  Object.defineProperty(document, 'scrollingElement', {
    configurable: true,
    value: document.documentElement,
  });
  document.documentElement.scrollTop = 240;
  window.dispatchEvent(new Event('scroll'));
  flushSync(() => state.set('home'));
  await vi.waitFor(() =>
    expect(document.querySelector('.music-card')).not.toBeNull(),
  );
  flushSync(() => state.set('search'));
  await vi.waitFor(() => expect(document.documentElement.scrollTop).toBe(240));
  expect(document.querySelector('.track-row')?.textContent).toContain(
    'Album track',
  );
  flushSync(() =>
    document
      .querySelector<HTMLButtonElement>('[aria-label="Back to music"]')!
      .click(),
  );
  expect(document.querySelector('.music-card')?.textContent).toContain(
    'An album',
  );
  expect(document.querySelector<HTMLInputElement>('input')!.value).toBe(
    'Test search',
  );
  flushSync(() => {
    input.value = 'Next query draft';
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  flushSync(() => state.set('home'));
  flushSync(() => state.set('search'));
  expect(document.querySelector<HTMLInputElement>('input')!.value).toBe(
    'Next query draft',
  );
});
it('appends unique next-page results and retains playlist context when playing', async () => {
  const song = {
    key: 'dQw4w9WgXcQ',
    title: 'First track',
    subtitle: '',
    artist: '',
    artwork: '',
    kind: 'song',
    video_id: 'dQw4w9WgXcQ',
    browse_id: '',
  };
  const next = {
    ...song,
    key: 'Rgrt_8mXrK8',
    video_id: 'Rgrt_8mXrK8',
    title: 'Second track',
  };
  vi.mocked(call).mockImplementation(async (command, args) => {
    if (command !== 'music_query') return undefined as any;
    const request = args?.request as { continuation: string };
    return {
      title: '',
      signed_in: false,
      message: '',
      playlist_id: 'PRIVATE',
      continuation: request.continuation ? '' : 'next',
      items: request.continuation ? [song, next] : [song],
    } as any;
  });
  component = mount(Music, {
    target: document.body,
    props: {
      data: structuredClone(initial),
      view: 'home',
      onError: vi.fn(),
      onAccount: vi.fn(),
    },
  });
  await vi.waitFor(() =>
    expect(document.querySelector('.music-card')).not.toBeNull(),
  );
  [...document.querySelectorAll<HTMLButtonElement>('button')]
    .find((button) => button.textContent?.trim() === 'Load more')!
    .click();
  await vi.waitFor(() =>
    expect(document.querySelectorAll('.music-card')).toHaveLength(2),
  );
  document.querySelector<HTMLButtonElement>('.music-card')!.click();
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith(
      'play_items',
      expect.objectContaining({
        playlistId: 'PRIVATE',
        items: expect.arrayContaining([
          expect.objectContaining({ title: 'Second track' }),
        ]),
      }),
    ),
  );
});
it('exposes queue editing and repeat controls without rebuilding the queue on selection', async () => {
  const data = structuredClone(initial);
  data.queue = [0, 1].map((index) => ({
    key: String(index),
    title: `Track ${index}`,
    subtitle: '',
    artist: '',
    artwork: '',
    kind: 'song',
    video_id: index ? 'Rgrt_8mXrK8' : 'dQw4w9WgXcQ',
    browse_id: '',
  }));
  vi.mocked(call).mockResolvedValue(undefined as any);
  component = mount(Music, {
    target: document.body,
    props: { data, view: 'queue', onError: vi.fn(), onAccount: vi.fn() },
  });
  flushSync();
  document
    .querySelector<HTMLButtonElement>('[aria-label="Move Track 1 up"]')!
    .click();
  expect(call).toHaveBeenCalledWith('queue_action', {
    action: { type: 'move', from: 1, to: 0 },
  });
  document.querySelectorAll<HTMLButtonElement>('.track-row')[1].click();
  expect(call).toHaveBeenCalledWith('queue_action', {
    action: { type: 'select', index: 1 },
  });
  expect(call).not.toHaveBeenCalledWith('play_items', expect.anything());
});
it('can switch an empty Home filter back to All or Songs without reloading', async () => {
  component = mount(Music, {
    target: document.body,
    props: {
      data: structuredClone(initial),
      view: 'home',
      onError: vi.fn(),
      onAccount: vi.fn(),
    },
  });
  await vi.waitFor(() =>
    expect(document.querySelector('.music-card')).not.toBeNull(),
  );
  const button = (label: string) =>
    [
      ...document.querySelectorAll<HTMLButtonElement>('.music-filters button'),
    ].find((el) => el.textContent?.trim() === label)!;
  flushSync(() => button('Artists').click());
  expect(document.querySelector('.music-empty h2')?.textContent).toContain(
    'No artists',
  );
  expect(button('All')).toBeDefined();
  flushSync(() => button('All').click());
  expect(document.querySelector('.music-card')?.textContent).toContain(
    'A song',
  );
  flushSync(() => button('Albums').click());
  flushSync(() => button('Songs').click());
  expect(document.querySelector('.track-row')?.textContent).toContain('A song');
});
