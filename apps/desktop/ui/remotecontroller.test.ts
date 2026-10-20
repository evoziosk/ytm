import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import DeviceConnection from './DeviceConnection.svelte';
import { call } from './api';
import {
  scan,
  checkPermissions,
  requestPermissions,
  cancel,
  openAppSettings,
} from '@tauri-apps/plugin-barcode-scanner';

vi.mock('./api', () => ({ native: true, mobile: true, call: vi.fn() }));
vi.mock('@tauri-apps/plugin-barcode-scanner', () => ({
  Format: { QRCode: 'QR_CODE' },
  scan: vi.fn(),
  checkPermissions: vi.fn(),
  requestPermissions: vi.fn(),
  cancel: vi.fn(),
  openAppSettings: vi.fn(),
}));
let component: ReturnType<typeof mount> | undefined;
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(call).mockResolvedValue({
    active: false,
    target: '',
    message: '',
    participant: null,
    snapshot: null,
  });
  vi.mocked(checkPermissions).mockResolvedValue('granted');
  vi.mocked(cancel).mockResolvedValue();
  vi.mocked(openAppSettings).mockResolvedValue();
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});
function start() {
  component = mount(DeviceConnection, { target: document.body });
  flushSync();
}
function button(label: string) {
  return [...document.querySelectorAll('button')].find(
    (b) => b.textContent?.trim() === label,
  )!;
}
it('offers Android scanning and connects using the full decoded invitation', async () => {
  const invitation = `http://192.168.1.2:9863/#session=${'a'.repeat(64)}&invite=${'b'.repeat(64)}&mode=group`;
  vi.mocked(scan).mockResolvedValue({
    content: invitation,
    format: 'QR_CODE' as any,
    bounds: null,
  });
  start();
  button('Scan host QR code').click();
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith('remote_connect', { invitation }),
  );
  expect(scan).toHaveBeenCalledWith({ formats: ['QR_CODE'], windowed: true });
  expect(document.querySelector('input')!.value).toBe(invitation);
});
it('accepts a plain IP and port without browser URL validation blocking submission', async () => {
  start();
  const input = document.querySelector('input')!;
  input.value = '192.168.1.2:9863';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  flushSync();
  expect(input.type).toBe('text');
  document
    .querySelector('form')!
    .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith('remote_connect', {
      invitation: '192.168.1.2:9863',
    }),
  );
});
it('discovers nearby hosts and connects directly without entering or scanning an invitation', async () => {
  const address = 'http://192.168.1.2:9863/';
  vi.mocked(call).mockImplementation(
    async (command) =>
      (command === 'remote_discover'
        ? [
            {
              id: 'a'.repeat(64),
              name: 'Living room',
              address,
              mode: 'personal',
            },
          ]
        : {
            active: false,
            target: '',
            message: '',
            participant: null,
            snapshot: null,
            updated_at: 0,
          }) as any,
  );
  start();
  await vi.waitFor(() =>
    expect(document.querySelector('.nearby-device')).not.toBeNull(),
  );
  (document.querySelector('.nearby-device') as HTMLButtonElement).click();
  await vi.waitFor(() =>
    expect(call).toHaveBeenCalledWith('remote_connect', {
      invitation: address,
    }),
  );
  expect(scan).not.toHaveBeenCalled();
});
it('explains camera denial and leaves manual entry available', async () => {
  vi.mocked(checkPermissions).mockResolvedValue('denied');
  vi.mocked(requestPermissions).mockResolvedValue('denied');
  start();
  button('Scan host QR code').click();
  await vi.waitFor(() =>
    expect(document.querySelector('[role=alert]')?.textContent).toContain(
      'Camera access',
    ),
  );
  expect(scan).not.toHaveBeenCalled();
  expect(call).not.toHaveBeenCalledWith('remote_connect', expect.anything());
  button('Open camera settings').click();
  expect(openAppSettings).toHaveBeenCalled();
  expect(document.querySelector('input')).not.toBeNull();
});
it('does not attempt a connection when scanning is cancelled', async () => {
  vi.mocked(scan).mockRejectedValue(new Error('Scan cancelled'));
  start();
  button('Scan host QR code').click();
  await vi.waitFor(() => expect(button('Scan host QR code')).toBeDefined());
  expect(call).not.toHaveBeenCalledWith('remote_connect', expect.anything());
  expect(document.querySelector('[role=alert]')).toBeNull();
});
it('shows an accessible cancel action over the camera and restores the app after cancellation', async () => {
  let rejectScan: (reason: Error) => void = () => {};
  vi.mocked(scan).mockImplementation(
    () =>
      new Promise((_, reject) => {
        rejectScan = reject;
      }),
  );
  vi.mocked(cancel).mockImplementation(async () => {
    rejectScan(new Error('cancelled'));
  });
  start();
  button('Scan host QR code').click();
  await vi.waitFor(() => expect(scan).toHaveBeenCalled());
  expect(document.documentElement.classList.contains('host-qr-scanning')).toBe(
    true,
  );
  button('Cancel scan').click();
  await vi.waitFor(() =>
    expect(
      document.documentElement.classList.contains('host-qr-scanning'),
    ).toBe(false),
  );
  expect(call).not.toHaveBeenCalledWith('remote_connect', expect.anything());
});
