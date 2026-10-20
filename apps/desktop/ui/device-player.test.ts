import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mount, unmount, flushSync } from 'svelte';
import App from './App.svelte';
import { initial, call, send } from './api';
import { noDevice, type PlaybackDevice } from './playback-device';
import type { ControlResult } from '@ytm/protocol';

const hooks = vi.hoisted(() => ({
  remote: undefined as
    undefined | ((event: { payload: PlaybackDevice }) => void),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (_name, callback) => {
    hooks.remote = callback;
    return () => {};
  }),
}));
vi.mock('./api', async (original) => ({
  ...(await original<typeof import('./api')>()),
  native: true,
  mobile: true,
  call: vi.fn(),
  send: vi.fn().mockResolvedValue(undefined),
  snapshot: vi.fn(async () => structuredClone(initial)),
  subscribe: vi.fn().mockResolvedValue(() => {}),
}));
let component: ReturnType<typeof mount> | undefined;
let device: PlaybackDevice;
const track = {
  video_id: 'dQw4w9WgXcQ',
  title: 'Playing on the desktop',
  artist: 'Host artist',
  artwork: '',
};
const queue: ControlResult = {
  type: 'queue',
  revision: 4,
  total: 2,
  offset: 0,
  entries: [
    { id: 'one', track },
    { id: 'two', track: { ...track, title: 'Next host song' } },
  ],
};
beforeEach(() => {
  vi.clearAllMocks();
  hooks.remote = undefined;
  device = {
    ...noDevice,
    active: true,
    target: 'Windows PC',
    message: 'Connected',
    updated_at: Date.now(),
    participant: {
      id: 'phone',
      name: 'Phone',
      role: 'personal_remote',
      mode: 'personal',
      approved: true,
      connected: true,
      permissions: {
        transport: true,
        skip: true,
        seek: true,
        queue: true,
        volume: false,
        replace_queue: false,
        catalog: true,
      },
    },
    snapshot: {
      session_id: 'a'.repeat(64),
      host_name: 'Windows PC',
      track,
      playing: true,
      position: 12,
      duration: 120,
      volume: 0.5,
      muted: false,
      buffering: false,
      queue_revision: 4,
      current_entry: 'one',
      participants: [],
    },
  };
  vi.mocked(call).mockImplementation(async (command) => {
    if (command === 'remote_status') return device as any;
    if (command === 'remote_request') return queue as any;
    if (command === 'remote_disconnect') {
      device = { ...noDevice };
      return undefined as any;
    }
    return {
      title: 'Music',
      items: [],
      continuation: '',
      playlist_id: '',
      signed_in: false,
      message: '',
    } as any;
  });
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});
async function start() {
  component = mount(App, { target: document.body });
  await vi.waitFor(() =>
    expect(document.querySelector('.now-track strong')?.textContent).toBe(
      track.title,
    ),
  );
}
it('uses the normal player, queue and device picker for a native remote session', async () => {
  await start();
  expect((document.querySelector('.now-playing') as HTMLElement).hidden).toBe(
    false,
  );
  expect(document.querySelector('.now-controls')?.textContent).toContain(
    'Windows PC',
  );
  document
    .querySelector<HTMLButtonElement>('.now-controls [aria-label="Pause"]')!
    .click();
  expect(send).toHaveBeenCalledWith({ type: 'toggle' });
  expect(
    document.querySelector<HTMLInputElement>('[aria-label="Volume"]')!.disabled,
  ).toBe(true);
  expect(document.querySelector('[aria-label="Like current song"]')).toBeNull();
  flushSync(() =>
    [...document.querySelectorAll<HTMLButtonElement>('nav button')]
      .find((button) => button.textContent?.trim() === 'Play queue')!
      .click(),
  );
  await vi.waitFor(() =>
    expect(
      document.querySelector('[aria-label="Remove Next host song"]'),
    ).not.toBeNull(),
  );
  [...document.querySelectorAll<HTMLButtonElement>('.music-filters button')]
    .find((button) => button.textContent?.trim() === 'Play all')!
    .click();
  expect(call).toHaveBeenCalledWith('remote_request', {
    command: {
      type: 'edit_queue',
      revision: 4,
      edit: { type: 'select', entry_id: 'one' },
    },
  });
  expect(call).not.toHaveBeenCalledWith('play_items', expect.anything());
  document
    .querySelector<HTMLButtonElement>('[aria-label="Remove Next host song"]')!
    .click();
  expect(call).toHaveBeenCalledWith('remote_request', {
    command: {
      type: 'edit_queue',
      revision: 4,
      edit: { type: 'remove', entry_id: 'two' },
    },
  });
  flushSync(() =>
    document
      .querySelector<HTMLButtonElement>(
        '[aria-label="Choose playback device"]',
      )!
      .click(),
  );
  expect(
    document.querySelector('[aria-labelledby="device-title"]'),
  ).not.toBeNull();
  expect(
    document.querySelector(
      '[aria-labelledby="device-title"] [aria-label="Pause"]',
    ),
  ).toBeNull();
  document.querySelector<HTMLButtonElement>('.device-local')!.click();
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith('remote_disconnect'),
  );
  await vi.waitFor(() =>
    expect(
      document.querySelector('.device-picker-button')?.textContent,
    ).toContain('This phone'),
  );
});
it('disables normal transport on host loss and never starts the phone player automatically', async () => {
  await start();
  flushSync(() =>
    hooks.remote!({
      payload: {
        ...device,
        message: 'Host disconnected; reconnecting',
        snapshot: null,
      },
    }),
  );
  expect(
    document.querySelector<HTMLButtonElement>(
      '.now-controls [aria-label="Play"]',
    )!.disabled,
  ).toBe(true);
  expect(
    document.querySelector<HTMLButtonElement>(
      '.now-controls [aria-label="Next track"]',
    )!.disabled,
  ).toBe(true);
  expect(
    document.querySelector<HTMLInputElement>('[aria-label="Track position"]')!
      .disabled,
  ).toBe(true);
  expect(send).not.toHaveBeenCalled();
  expect(call).not.toHaveBeenCalledWith('remote_disconnect');
});

it('selects a host queue entry from Home and respects revoked queue permission', async () => {
  await start();
  await vi.waitFor(() =>
    expect(
      document.querySelector('[aria-label="Play Next host song from Up next"]'),
    ).not.toBeNull(),
  );
  const next = document.querySelector<HTMLButtonElement>(
    '[aria-label="Play Next host song from Up next"]',
  )!;
  next.click();
  expect(call).toHaveBeenCalledWith('remote_request', {
    command: {
      type: 'edit_queue',
      revision: 4,
      edit: { type: 'select', entry_id: 'two' },
    },
  });
  expect(call).not.toHaveBeenCalledWith('queue_action', expect.anything());
  expect(document.querySelector('nav button.active')?.textContent?.trim()).toBe(
    'Home',
  );
  flushSync(() =>
    hooks.remote!({
      payload: {
        ...device,
        participant: {
          ...device.participant!,
          permissions: { ...device.participant!.permissions, queue: false },
        },
      },
    }),
  );
  expect(next.disabled).toBe(true);
  vi.mocked(call).mockClear();
  next.click();
  expect(call).not.toHaveBeenCalled();
});

it('offers native Google library authorization on Android without promising Premium playback', async () => {
  await start();
  flushSync(() =>
    document
      .querySelector<HTMLButtonElement>('.account-dropdown button')!
      .click(),
  );
  const dialog = document.querySelector('[aria-labelledby="account-title"]')!;
  expect(dialog.textContent).toContain('Connect your Google library.');
  expect(dialog.textContent?.replace(/\s+/g, ' ')).toContain(
    'does not enable signed-in Music Premium playback',
  );
  expect(dialog.textContent).not.toContain('separate account window');
  const connect = [
    ...dialog.querySelectorAll<HTMLButtonElement>('button'),
  ].find((button) => button.textContent?.includes('Connect Google library'))!;
  connect.click();
  expect(call).toHaveBeenCalledWith('open_account');
});
