import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import QuickSearch from './QuickSearch.svelte';
import { initial, call, snapshot } from './api';

const windowMock = vi.hoisted(() => ({
  hide: vi.fn().mockResolvedValue(undefined),
  onFocusChanged: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => windowMock,
}));
vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  native: true,
  call: vi.fn(),
  snapshot: vi.fn(),
  subscribe: vi.fn().mockResolvedValue(() => {}),
}));
const song = (id: string, title: string, kind = 'song') => ({
  key: id,
  title,
  subtitle: 'Artist',
  artist: 'Artist',
  artwork: '',
  kind,
  video_id: id,
  browse_id: '',
});
const first = song('dQw4w9WgXcQ', 'First song');
const second = song('Rgrt_8mXrK8', 'Second song');
const page = (items = [first, second]) => ({
  title: 'Results',
  items,
  signed_in: false,
  message: '',
  continuation: '',
  playlist_id: '',
});
let component: ReturnType<typeof mount>;
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(call)
    .mockReset()
    .mockResolvedValue(page() as any);
  vi.mocked(snapshot).mockResolvedValue(structuredClone(initial));
  component = mount(QuickSearch, { target: document.body });
  flushSync();
});
afterEach(async () => {
  await unmount(component);
  document.body.innerHTML = '';
});
function type(query: string) {
  const input = document.querySelector<HTMLInputElement>('input')!;
  flushSync(() => {
    input.value = query;
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  return input;
}
function key(input: HTMLInputElement, key: string, isComposing = false) {
  flushSync(() =>
    input.dispatchEvent(
      new KeyboardEvent('keydown', { key, bubbles: true, isComposing }),
    ),
  );
}
it('focuses the input, searches while typing, and plays the keyboard-selected result', async () => {
  const input = type('Test song');
  await vi.waitFor(() => expect(document.activeElement).toBe(input));
  await vi.waitFor(() =>
    expect(document.querySelectorAll('[role="option"]')).toHaveLength(2),
  );
  expect(call).toHaveBeenCalledWith('music_query', {
    request: {
      kind: 'search',
      query: 'Test song',
      browse_id: '',
      continuation: '',
    },
  });
  key(input, 'ArrowDown');
  expect(input.getAttribute('aria-activedescendant')).toBe('quick-result-1');
  key(input, 'Enter');
  await vi.waitFor(() => expect(windowMock.hide).toHaveBeenCalledOnce());
  expect(call).toHaveBeenCalledWith('play_items', {
    items: [first, second],
    index: 1,
    playlistId: '',
  });
});
it('wraps keyboard selection and lets Escape close without playing', async () => {
  const input = type('Artist');
  await vi.waitFor(() =>
    expect(document.querySelectorAll('[role="option"]')).toHaveLength(2),
  );
  key(input, 'ArrowUp');
  expect(input.getAttribute('aria-activedescendant')).toBe('quick-result-1');
  key(input, 'ArrowDown');
  expect(input.getAttribute('aria-activedescendant')).toBe('quick-result-0');
  key(input, 'Escape');
  expect(windowMock.hide).toHaveBeenCalledOnce();
  expect(call).not.toHaveBeenCalledWith('play_items', expect.anything());
});
it('coalesces queued searches and ignores an older response', async () => {
  let finish!: (value: any) => void;
  vi.mocked(call).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  type('Old');
  await vi.waitFor(() => expect(call).toHaveBeenCalledOnce());
  type('Middle');
  const input = type('Newest');
  key(input, 'Enter');
  await new Promise((resolve) => setTimeout(resolve, 280));
  vi.mocked(call).mockResolvedValueOnce(
    page([song('5NV6Rdv1a3I', 'Newest song')]) as any,
  );
  finish(page());
  await vi.waitFor(() =>
    expect(document.querySelector('[role="option"]')?.textContent).toContain(
      'Newest song',
    ),
  );
  expect(call).toHaveBeenCalledTimes(2);
  expect(document.querySelector('main')?.textContent).not.toContain(
    'First song',
  );
});
it('does not treat IME composition as a playback command and retains playback errors', async () => {
  const input = type('Test');
  await vi.waitFor(() =>
    expect(document.querySelectorAll('[role="option"]')).toHaveLength(2),
  );
  key(input, 'Enter', true);
  expect(call).not.toHaveBeenCalledWith('play_items', expect.anything());
  vi.mocked(call).mockRejectedValueOnce(
    new Error('This recording is unavailable'),
  );
  key(input, 'Enter');
  await vi.waitFor(() =>
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      'unavailable',
    ),
  );
  expect(windowMock.hide).not.toHaveBeenCalled();
});
it('respects the listening-party host lock', async () => {
  const data = structuredClone(initial);
  data.party.role = 'member';
  await unmount(component);
  vi.mocked(snapshot).mockResolvedValue(data);
  component = mount(QuickSearch, { target: document.body });
  flushSync();
  const input = type('Test');
  await vi.waitFor(() =>
    expect(document.querySelectorAll('[role="option"]')).toHaveLength(2),
  );
  key(input, 'Enter');
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    'party host',
  );
  expect(call).not.toHaveBeenCalledWith('play_items', expect.anything());
});

it.each(['next', 'end'] as const)(
  'queues the selected song at %s without hiding the popup or starting playback',
  async (placement) => {
    const input = type('Test');
    await vi.waitFor(() =>
      expect(document.querySelectorAll('[role="option"]')).toHaveLength(2),
    );
    key(input, 'ArrowDown');
    flushSync(() =>
      input.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 'Enter',
          bubbles: true,
          shiftKey: placement === 'next',
          ctrlKey: placement === 'end',
        }),
      ),
    );
    await vi.waitFor(() =>
      expect(call).toHaveBeenCalledWith('queue_track', {
        track: {
          video_id: second.video_id,
          title: second.title,
          artist: second.artist,
          artwork: '',
        },
        placement,
      }),
    );
    await vi.waitFor(() =>
      expect(document.querySelector('[role="status"]')?.textContent).toContain(
        second.title,
      ),
    );
    expect(call).not.toHaveBeenCalledWith('play_items', expect.anything());
    expect(windowMock.hide).not.toHaveBeenCalled();
  },
);

it('keeps queue failures visible and retries from the action buttons', async () => {
  type('Test');
  await vi.waitFor(() =>
    expect(document.querySelectorAll('[role="option"]')).toHaveLength(2),
  );
  const button = [
    ...document.querySelectorAll<HTMLButtonElement>('.quick-actions button'),
  ].find((button) => button.textContent === 'Add to queue')!;
  vi.mocked(call).mockRejectedValueOnce(
    new Error('Host denied queue permission'),
  );
  flushSync(() => button.click());
  await vi.waitFor(() =>
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      'Host denied',
    ),
  );
  flushSync(() => button.click());
  await vi.waitFor(() =>
    expect(document.querySelector('[role="status"]')?.textContent).toContain(
      'added to queue',
    ),
  );
  expect(document.querySelector('[role="alert"]')).toBeNull();
  expect(windowMock.hide).not.toHaveBeenCalled();
});
