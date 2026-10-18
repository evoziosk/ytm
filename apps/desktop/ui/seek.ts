import type { PlaybackState } from '@ytm/protocol';

export interface SeekAttempt {
  position: number;
  trackId: string;
  sentAt: number;
}

export function seekResult(
  attempt: SeekAttempt,
  player: PlaybackState,
  now: number,
): 'pending' | 'confirmed' | 'cancelled' | 'timeout' {
  if (player.track?.video_id !== attempt.trackId || player.ad)
    return 'cancelled';
  // Old snapshots must not undo a newer user request. Allow normal playback since the seek.
  const advanced = player.playing
    ? Math.max(0, (player.ts - attempt.sentAt) / 1000) * player.playback_rate
    : 0;
  if (
    player.ts >= attempt.sentAt &&
    player.position >= attempt.position - 1 &&
    player.position <= attempt.position + advanced + 1.5
  )
    return 'confirmed';
  return now - attempt.sentAt >= 10000 ? 'timeout' : 'pending';
}

export const formatTime = (seconds: number) =>
  `${Math.floor(Math.max(0, seconds) / 60)}:${Math.floor(
    Math.max(0, seconds) % 60,
  )
    .toString()
    .padStart(2, '0')}`;
