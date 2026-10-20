import type {
  ControlParticipant,
  ControlResult,
  RemoteSnapshot,
  RemoteQueueEdit,
} from '@ytm/protocol';
import type { Snapshot } from './api';

export interface PlaybackDevice {
  active: boolean;
  target: string;
  message: string;
  participant: ControlParticipant | null;
  snapshot: RemoteSnapshot | null;
  updated_at: number;
  devices?: { name: string; address: string }[];
}
export type DeviceQueue = Extract<ControlResult, { type: 'queue' }>;
export const noDevice: PlaybackDevice = {
  active: false,
  target: '',
  message: '',
  participant: null,
  snapshot: null,
  updated_at: 0,
};
export function allowed(
  device: PlaybackDevice,
  permission: keyof ControlParticipant['permissions'],
) {
  return (
    !device.active ||
    Boolean(
      device.message === 'Connected' &&
      device.snapshot &&
      device.participant?.approved &&
      device.participant.permissions[permission],
    )
  );
}
export function playbackView(
  local: Snapshot,
  device: PlaybackDevice,
  queue: DeviceQueue | null,
): Snapshot {
  if (!device.active) return local;
  const host = device.snapshot;
  const ready =
    device.message === 'Connected' &&
    Boolean(host && device.participant?.approved);
  const entries =
    ready && queue?.revision === host?.queue_revision
      ? (queue?.entries ?? [])
      : [];
  return {
    ...local,
    remote: device,
    player: {
      track: host?.track ? { ...host.track, album: '', playlist_id: '' } : null,
      playing: ready && Boolean(host?.playing),
      position: host?.position ?? 0,
      duration: host?.duration ?? 0,
      volume: host?.volume ?? 0,
      muted: host?.muted ?? false,
      buffering: !ready || Boolean(host?.buffering),
      playback_rate: 1,
      ad: false,
      ts: device.updated_at,
    },
    queue: entries.map((entry) => ({
      key: entry.id,
      ...entry.track,
      subtitle: entry.track.artist,
      kind: 'song',
      browse_id: '',
    })),
    queue_index: entries.findIndex((entry) => entry.id === host?.current_entry),
    queue_playlist_id: '',
    shuffle: false,
    repeat: 'off',
    playback_error: '',
  };
}
export function queueEdit(
  queue: Snapshot['queue'],
  action: Record<string, unknown>,
  total = queue.length,
): RemoteQueueEdit {
  const entry = (index: unknown) => {
    if (typeof index !== 'number' || !Number.isInteger(index) || !queue[index])
      throw new Error('Queue changed; refresh before editing');
    return queue[index].key;
  };
  switch (action.type) {
    case 'select':
      return { type: 'select', entry_id: entry(action.index) };
    case 'remove':
      return { type: 'remove', entry_id: entry(action.index) };
    case 'move': {
      const from = Number(action.from),
        to = Number(action.to);
      const id = entry(from);
      entry(to);
      if (from < to && to + 1 === queue.length && total > queue.length)
        throw new Error(
          'Load more queue before moving this track farther down',
        );
      return {
        type: 'move',
        entry_id: id,
        before_id:
          from < to && to + 1 === queue.length
            ? ''
            : entry(from < to ? to + 1 : to),
      };
    }
    case 'clear':
      return { type: 'clear' };
    default:
      throw new Error('Change shuffle and repeat on the playback device');
  }
}
