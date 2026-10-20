import { expect, it } from 'vitest';
import { initial } from './api';
import {
  playbackView,
  allowed,
  noDevice,
  queueEdit,
  type PlaybackDevice,
  type DeviceQueue,
} from './playback-device';

export function connectedDevice(): PlaybackDevice {
  return {
    active: true,
    target: 'Living room',
    message: 'Connected',
    updated_at: 1000,
    participant: {
      id: 'peer',
      name: 'Phone',
      approved: true,
      connected: true,
      mode: 'personal',
      role: 'personal_remote',
      permissions: {
        transport: true,
        skip: true,
        seek: true,
        queue: true,
        replace_queue: false,
        volume: false,
        catalog: true,
      },
    },
    snapshot: {
      session_id: 'a'.repeat(64),
      host_name: 'Living room',
      track: {
        video_id: 'dQw4w9WgXcQ',
        title: 'Host track',
        artist: 'Host artist',
        artwork: '',
      },
      playing: true,
      position: 12,
      duration: 120,
      volume: 0.5,
      muted: false,
      buffering: false,
      queue_revision: 7,
      current_entry: 'second',
      participants: [],
    },
  };
}
export function hostQueue(): DeviceQueue {
  const track = connectedDevice().snapshot!.track!;
  return {
    type: 'queue',
    revision: 7,
    total: 3,
    offset: 0,
    entries: ['first', 'second', 'third'].map((id) => ({ id, track })),
  };
}
it('projects host playback into the main player while retaining the local account and preferences', () => {
  const local = structuredClone(initial);
  local.account = { signed_in: true, name: 'My private account' };
  const view = playbackView(local, connectedDevice(), hostQueue());
  expect(view.player.track?.title).toBe('Host track');
  expect(view.player.position).toBe(12);
  expect(view.player.ts).toBe(1000);
  expect(view.queue.map((item) => item.key)).toEqual([
    'first',
    'second',
    'third',
  ]);
  expect(view.queue_index).toBe(1);
  expect(view.account).toBe(local.account);
  expect(view.settings).toBe(local.settings);
  expect(local.player.track).toBeNull();
});
it('keeps pending and disconnected host controls disabled without falling back to local playback', () => {
  const device = connectedDevice();
  device.snapshot = null;
  device.message = 'Connection lost; reconnecting';
  const local = structuredClone(initial);
  local.player.playing = true;
  const view = playbackView(local, device, hostQueue());
  expect(view.player.playing).toBe(false);
  expect(view.player.track).toBeNull();
  expect(view.queue).toEqual([]);
  expect(allowed(device, 'transport')).toBe(false);
  expect(playbackView(local, noDevice, null)).toBe(local);
});
it('uses current permissions and hides queue pages whose revision changed', () => {
  const device = connectedDevice();
  expect(allowed(device, 'volume')).toBe(false);
  expect(allowed(device, 'skip')).toBe(true);
  device.participant!.approved = false;
  expect(allowed(device, 'skip')).toBe(false);
  device.participant!.approved = true;
  device.snapshot!.queue_revision++;
  expect(playbackView(initial, device, hostQueue()).queue).toEqual([]);
  const outsidePage = connectedDevice();
  outsidePage.snapshot!.current_entry = 'not-loaded';
  expect(playbackView(initial, outsidePage, hostQueue()).queue_index).toBe(-1);
});
it('translates queue controls into stable IDs including moving to the end', () => {
  const queue = playbackView(initial, connectedDevice(), hostQueue()).queue;
  expect(queueEdit(queue, { type: 'select', index: 1 })).toEqual({
    type: 'select',
    entry_id: 'second',
  });
  expect(queueEdit(queue, { type: 'remove', index: 1 })).toEqual({
    type: 'remove',
    entry_id: 'second',
  });
  expect(queueEdit(queue, { type: 'move', from: 0, to: 1 })).toEqual({
    type: 'move',
    entry_id: 'first',
    before_id: 'third',
  });
  expect(queueEdit(queue, { type: 'move', from: 1, to: 2 })).toEqual({
    type: 'move',
    entry_id: 'second',
    before_id: '',
  });
  expect(() => queueEdit(queue, { type: 'remove', index: 8 })).toThrow(
    'Queue changed',
  );
  expect(() => queueEdit(queue, { type: 'move', from: 1, to: 2 }, 50)).toThrow(
    'Load more queue',
  );
});
