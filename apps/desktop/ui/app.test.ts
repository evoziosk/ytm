import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mount, unmount, flushSync } from 'svelte';
import App from './App.svelte';
import { initial, call, snapshot } from './api';

vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  native: true,
  snapshot: vi.fn().mockImplementation(async () => structuredClone(initial)),
  subscribe: vi.fn().mockResolvedValue(() => {}),
  call: vi.fn().mockResolvedValue({
    title: '',
    signed_in: false,
    message: '',
    continuation: '',
    playlist_id: '',
    items: [],
  }),
}));
let component: ReturnType<typeof mount> | undefined;
const emptyPage = {
  title: '',
  signed_in: false,
  message: '',
  continuation: '',
  playlist_id: '',
  items: [],
};
beforeEach(() => {
  vi.mocked(call)
    .mockReset()
    .mockResolvedValue(emptyPage as any);
  vi.mocked(snapshot)
    .mockReset()
    .mockImplementation(async () => structuredClone(initial));
});
afterEach(async () => {
  if (component) await unmount(component);
  document.body.innerHTML = '';
});

const navigate = (label: string) =>
  flushSync(() =>
    [...document.querySelectorAll<HTMLButtonElement>('nav button')]
      .find((button) => button.textContent?.trim() === label)!
      .click(),
  );
async function preferences() {
  component = mount(App, { target: document.body });
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith('music_query', expect.anything()),
  );
  navigate('Preferences');
  return document.querySelector<HTMLButtonElement>(
    '[aria-label="Window blur"]',
  )!;
}

it('applies and saves blur immediately without requiring the preferences Save button', async () => {
  const toggle = await preferences();
  flushSync(() => toggle.click());
  await vi.waitFor(() =>
    expect(document.documentElement.classList.contains('app-blur')).toBe(true),
  );
  expect(call).toHaveBeenCalledWith('set_window_blur', { enabled: true });
  expect(document.querySelector('.save-bar')).toBeNull();
  flushSync(() => toggle.click());
  await vi.waitFor(() =>
    expect(document.documentElement.classList.contains('app-blur')).toBe(false),
  );
  expect(call).toHaveBeenCalledWith('set_window_blur', { enabled: false });
});

it('preserves unsaved preferences while saving the appearance choice', async () => {
  const toggle = await preferences();
  flushSync(() =>
    document
      .querySelector<HTMLButtonElement>('[aria-label="Launch at startup"]')!
      .click(),
  );
  flushSync(() => toggle.click());
  await vi.waitFor(() =>
    expect(toggle.getAttribute('aria-checked')).toBe('true'),
  );
  expect(
    document
      .querySelector<HTMLButtonElement>('[aria-label="Launch at startup"]')!
      .getAttribute('aria-checked'),
  ).toBe('true');
  document.querySelector<HTMLButtonElement>('.save-bar .primary')!.click();
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith('save_settings', {
      settings: expect.objectContaining({ autostart: true, window_blur: true }),
    }),
  );
});

it('keeps the previous appearance when saving blur fails', async () => {
  const toggle = await preferences();
  vi.mocked(call).mockRejectedValueOnce(
    new Error('Could not save preferences'),
  );
  flushSync(() => toggle.click());
  await vi.waitFor(() =>
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      'Could not save preferences',
    ),
  );
  expect(toggle.getAttribute('aria-checked')).toBe('false');
  expect(document.documentElement.classList.contains('app-blur')).toBe(false);
});

it('restores a saved blur choice on launch', async () => {
  const saved = structuredClone(initial);
  saved.settings.window_blur = true;
  vi.mocked(snapshot).mockResolvedValue(saved);
  const toggle = await preferences();
  await vi.waitFor(() =>
    expect(toggle.getAttribute('aria-checked')).toBe('true'),
  );
  expect(document.documentElement.classList.contains('app-blur')).toBe(true);
  expect(call).not.toHaveBeenCalledWith('set_window_blur', expect.anything());
});

it('keeps header search available and preserves its query through Preferences', async () => {
  component = mount(App, { target: document.body });
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith('music_query', expect.anything()),
  );
  expect(document.querySelector('main')?.textContent).not.toContain(
    'An open source desktop companion',
  );
  const navigation = (label: string) =>
    [...document.querySelectorAll<HTMLButtonElement>('nav button')].find(
      (button) => button.textContent?.trim() === label,
    )!;
  flushSync(() => navigation('Search').click());
  const input = document.querySelector<HTMLInputElement>(
    '[aria-label="Search music"]',
  )!;
  flushSync(() => {
    input.value = 'Saved query';
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  flushSync(() => navigation('Preferences').click());
  expect(document.querySelector('[aria-label="Search music"]')).toBe(input);
  expect(input.closest('[hidden]')).toBeNull();
  flushSync(() => navigation('Search').click());
  expect(
    document.querySelector<HTMLInputElement>('[aria-label="Search music"]')
      ?.value,
  ).toBe('Saved query');
  expect(input.closest('[hidden]')).toBeNull();
});

it('searches from Preferences in the music view without losing the submitted query', async () => {
  component = mount(App, { target: document.body });
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith('music_query', expect.anything()),
  );
  navigate('Preferences');
  const input = document.querySelector<HTMLInputElement>(
    '[aria-label="Search music"]',
  )!;
  flushSync(() => {
    input.value = 'Massive Attack';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.form!.dispatchEvent(
      new Event('submit', { bubbles: true, cancelable: true }),
    );
  });
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith('music_query', {
      request: {
        kind: 'search',
        query: 'Massive Attack',
        browse_id: '',
        continuation: '',
      },
    }),
  );
  expect(
    document.querySelector('.music-workspace')?.hasAttribute('hidden'),
  ).toBe(false);
  expect(input.value).toBe('Massive Attack');
});

it('plays an Up next entry directly from Home without replacing the queue or navigating', async () => {
  const saved = structuredClone(initial);
  saved.queue = [0, 1].map((index) => ({
    key: `entry-${index}`,
    kind: 'song',
    title: index ? 'Next song' : 'Current song',
    artist: 'Artist',
    subtitle: 'Artist',
    artwork: '',
    video_id: 'dQw4w9WgXcQ',
    browse_id: '',
  }));
  vi.mocked(snapshot).mockResolvedValue(saved);
  component = mount(App, { target: document.body });
  await vi.waitFor(() =>
    expect(
      document.querySelector('[aria-label="Play Next song from Up next"]'),
    ).not.toBeNull(),
  );
  document
    .querySelector<HTMLButtonElement>(
      '[aria-label="Play Next song from Up next"]',
    )!
    .click();
  expect(call).toHaveBeenCalledWith('queue_action', {
    action: { type: 'select', index: 1 },
  });
  expect(document.querySelector('nav button.active')?.textContent?.trim()).toBe(
    'Home',
  );
  expect(call).not.toHaveBeenCalledWith('play_items', expect.anything());
});
