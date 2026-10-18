// @vitest-environment jsdom
import { beforeEach, afterEach, expect, it, vi } from 'vitest';
import { snapshot } from './player';
import { control, confirmedControl } from './controls';
beforeEach(() => {
  document.body.innerHTML = '<div id="movie_player"></div><video></video>';
});
afterEach(() => vi.useRealTimers());
it('raising volume unmutes and an explicit mute preserves the slider volume', async () => {
  const video = document.querySelector('video')!;
  video.muted = true;
  await confirmedControl({ type: 'volume', volume: 0.75 });
  expect(snapshot()).toMatchObject({ volume: 0.75, muted: false });
  await confirmedControl({ type: 'mute', muted: true });
  expect(snapshot()).toMatchObject({ volume: 0.75, muted: true });
});
it('reports an unhandled track navigation instead of silently reporting success', async () => {
  vi.useFakeTimers();
  document.body.insertAdjacentHTML('beforeend', '<ytmusic-app></ytmusic-app>');
  Object.assign(document.getElementById('movie_player')!, {
    getVideoData: () => ({ video_id: 'dQw4w9WgXcQ' }),
  });
  const request = expect(
    confirmedControl({ type: 'load_track', video_id: 'Rgrt_8mXrK8' }),
  ).rejects.toThrow('could not complete');
  await vi.advanceTimersByTimeAsync(10100);
  await request;
});
it('confirms the actual seek and supersedes an earlier unacknowledged seek', async () => {
  vi.useFakeTimers();
  const video = document.querySelector('video')!;
  Object.defineProperty(video, 'readyState', { configurable: true, value: 4 });
  const first = expect(
    confirmedControl({ type: 'seek', position: 10 }),
  ).rejects.toThrow('Superseded');
  const second = confirmedControl({ type: 'seek', position: 89 });
  await vi.advanceTimersByTimeAsync(160);
  await first;
  await second;
  expect(video.currentTime).toBe(89);
});
it('confirms an accepted paused seek while new audio is still buffering', async () => {
  vi.useFakeTimers();
  const video = document.querySelector('video')!;
  Object.defineProperty(video, 'readyState', { configurable: true, value: 1 });
  const request = confirmedControl({ type: 'seek', position: 129 });
  await vi.advanceTimersByTimeAsync(160);
  await request;
  expect(snapshot()).toMatchObject({ position: 129, buffering: true });
});
it('uses player data, media metadata and safe values for a live stream', () => {
  const player = document.getElementById('movie_player')!;
  Object.assign(player, {
    getVideoData: () => ({
      video_id: 'dQw4w9WgXcQ',
      title: 'Song',
      author: 'Artist',
    }),
  });
  Object.defineProperty(navigator, 'mediaSession', {
    configurable: true,
    value: {
      metadata: {
        album: 'Album',
        artwork: [{ src: 'https://example.com/art.jpg' }],
      },
    },
  });
  Object.defineProperty(document.querySelector('video'), 'duration', {
    configurable: true,
    value: Infinity,
  });
  expect(snapshot(100)).toMatchObject({
    track: { title: 'Song', album: 'Album' },
    duration: 0,
    ts: 100,
  });
});
it('rejects injection-like video IDs and clamps volume', async () => {
  const load = vi.fn();
  const volume = vi.fn();
  Object.assign(document.getElementById('movie_player')!, {
    loadVideoById: load,
    setVolume: volume,
  });
  await control({ type: 'load_track', video_id: 'bad\";alert(1)' });
  expect(load).not.toHaveBeenCalled();
  await control({ type: 'volume', volume: 2 });
  expect(volume).toHaveBeenCalledWith(100);
});
