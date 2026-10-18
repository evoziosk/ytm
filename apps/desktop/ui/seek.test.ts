import { expect, it } from 'vitest';
import { initial } from './api';
import { seekResult } from './seek';

const attempt = { position: 89, trackId: 'dQw4w9WgXcQ', sentAt: 10000 };
const player = {
  ...initial.player,
  track: {
    video_id: attempt.trackId,
    title: '',
    artist: '',
    album: '',
    artwork: '',
    playlist_id: '',
  },
  playing: true,
  duration: 240,
  ts: 10001,
};
it('holds the requested position through old and delayed snapshots', () => {
  expect(seekResult(attempt, { ...player, position: 1, ts: 9999 }, 10100)).toBe(
    'pending',
  );
  expect(seekResult(attempt, { ...player, position: 2 }, 10500)).toBe(
    'pending',
  );
  expect(
    seekResult(attempt, { ...player, position: 89, ts: 9999 }, 10500),
  ).toBe('pending');
  expect(seekResult(attempt, { ...player, position: 89 }, 10500)).toBe(
    'confirmed',
  );
});
it('confirms a seek after playback advances but expires a rejected seek', () => {
  expect(
    seekResult(attempt, { ...player, position: 92, ts: 13000 }, 13000),
  ).toBe('confirmed');
  expect(seekResult(attempt, { ...player, position: 5 }, 20000)).toBe(
    'timeout',
  );
  expect(seekResult(attempt, { ...player, track: null }, 10100)).toBe(
    'cancelled',
  );
  expect(seekResult(attempt, { ...player, ad: true }, 10100)).toBe('cancelled');
});
