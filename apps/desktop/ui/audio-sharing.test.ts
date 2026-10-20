import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mount, unmount, flushSync } from 'svelte';
import AudioSharing from './AudioSharing.svelte';
import { initialAudio, type AudioRoutingStatus } from './audio-routing';
import { call } from './api';
vi.mock('./api', () => ({ native: true, call: vi.fn() }));
let component: ReturnType<typeof mount> | undefined;
let status: AudioRoutingStatus;
beforeEach(() => {
  status = {
    ...structuredClone(initialAudio),
    supported: true,
    message: 'Ready',
  };
  status.preferences.output_id = 'cable';
  status.preferences.microphone_id = 'physical';
  vi.mocked(call)
    .mockReset()
    .mockImplementation(async (command, args) => {
      if (command === 'audio_status') return structuredClone(status) as any;
      if (command === 'audio_devices')
        return {
          outputs: [{ id: 'cable', name: 'CABLE Input' }],
          microphones: [{ id: 'physical', name: 'USB microphone' }],
        } as any;
      if (command === 'audio_stop')
        status = {
          ...status,
          active: false,
          microphone_enabled: false,
          microphone_muted: false,
        };
      if (command === 'audio_apply')
        status = { ...status, active: true, ...(args as any).routing };
      return structuredClone(status) as any;
    });
});
afterEach(async () => {
  if (component) await unmount(component);
  document.body.innerHTML = '';
});
async function open() {
  component = mount(AudioSharing, {
    target: document.body,
    props: { visible: true },
  });
  await vi.waitFor(() =>
    expect(document.querySelector('select')?.options.length).toBe(2),
  );
}
function click(text: string) {
  flushSync(() =>
    [...document.querySelectorAll('button')]
      .find((button) => button.textContent?.trim() === text)!
      .click(),
  );
}
it('never opens audio on mount and starts music without enabling the saved microphone', async () => {
  await open();
  expect(call).not.toHaveBeenCalledWith('audio_apply', expect.anything());
  click('Start sharing');
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith(
      'audio_apply',
      expect.objectContaining({
        start: true,
        routing: expect.objectContaining({ microphone_enabled: false }),
      }),
    ),
  );
  expect(document.body.textContent).toContain('Microphone is off');
});
it('requires a separate mic action, mutes it, and stops both sources', async () => {
  await open();
  click('Start sharing');
  await vi.waitFor(() =>
    expect(
      document
        .querySelector('[aria-label="Include microphone"]')
        ?.hasAttribute('disabled'),
    ).toBe(false),
  );
  flushSync(() =>
    document
      .querySelector<HTMLButtonElement>('[aria-label="Include microphone"]')!
      .click(),
  );
  await vi.waitFor(() =>
    expect(document.body.textContent).toContain('Microphone is live'),
  );
  click('Mute microphone');
  await vi.waitFor(() =>
    expect(document.body.textContent).toContain('Microphone is off'),
  );
  expect(call).toHaveBeenCalledWith(
    'audio_apply',
    expect.objectContaining({
      routing: expect.objectContaining({
        microphone_enabled: true,
        microphone_muted: true,
      }),
    }),
  );
  click('Stop sharing');
  await vi.waitFor(() => expect(call).toHaveBeenCalledWith('audio_stop'));
});
it('keeps an active sharing indicator and stop action outside the settings page', async () => {
  status.active = true;
  status.microphone_enabled = true;
  component = mount(AudioSharing, {
    target: document.body,
    props: { visible: false },
  });
  await vi.waitFor(() =>
    expect(
      document.querySelector('[aria-label="Audio sharing active"]')
        ?.textContent,
    ).toContain('Sharing music + microphone'),
  );
  click('Stop sharing');
  await vi.waitFor(() =>
    expect(
      document.querySelector('[aria-label="Audio sharing active"]'),
    ).toBeNull(),
  );
});
it('shows a failed microphone action without reporting a live mic', async () => {
  await open();
  click('Start sharing');
  await vi.waitFor(() => expect(status.active).toBe(true));
  vi.mocked(call).mockImplementationOnce(async () => {
    throw new Error('Microphone access denied');
  });
  flushSync(() =>
    document
      .querySelector<HTMLButtonElement>('[aria-label="Include microphone"]')!
      .click(),
  );
  await vi.waitFor(() =>
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      'Microphone access denied',
    ),
  );
  expect(document.body.textContent).not.toContain('Microphone is live');
});

it('keeps an interruption visible after the output disappears', async () => {
  status.active = true;
  component = mount(AudioSharing, {
    target: document.body,
    props: { visible: false },
  });
  await vi.waitFor(() =>
    expect(
      document.querySelector('[aria-label="Audio sharing active"]'),
    ).not.toBeNull(),
  );
  status.active = false;
  status.message = 'Virtual output disconnected. Sharing stopped.';
  await vi.waitFor(() =>
    expect(
      document.querySelector('[aria-label="Audio sharing stopped"]')
        ?.textContent,
    ).toContain('Virtual output disconnected'),
  );
});

it('opens only the official driver setup page through the scoped native opener', async () => {
  vi.mocked(call).mockImplementation(async (command) =>
    command === 'audio_status'
      ? (structuredClone(status) as any)
      : ({ outputs: [], microphones: [] } as any),
  );
  component = mount(AudioSharing, {
    target: document.body,
    props: { visible: true },
  });
  await vi.waitFor(() =>
    expect(
      document.querySelector('a[href="https://vb-audio.com/Cable/"]'),
    ).not.toBeNull(),
  );
  flushSync(() =>
    document
      .querySelector<HTMLAnchorElement>(
        'a[href="https://vb-audio.com/Cable/"]',
      )!
      .click(),
  );
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith('plugin:opener|open_url', {
      url: 'https://vb-audio.com/Cable/',
      with: null,
    }),
  );
});
