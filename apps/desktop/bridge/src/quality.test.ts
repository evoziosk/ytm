// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from 'vitest';
import { audioStatus, preferSongMode, trackParameters } from './quality';
import { control } from './controls';

beforeEach(() => {
  document.body.innerHTML = '<div id="movie_player"></div><video></video>';
});
it('selects the website Song mode for eligible paired tracks without overriding subscription restrictions', () => {
  window.ytcfg = { get: () => true };
  const toggle = document.createElement('ytmusic-av-toggle');
  const selectSong = vi.fn();
  Object.assign(toggle, {
    playbackMode: 'OMV_PREFERRED',
    audioOnlyPlaybackAvailable: true,
    selectedQueueItemHasCounterpart: true,
    isNonSubscriberPlayingMusic: true,
    onSongAvToggleTap: selectSong,
  });
  document.body.append(toggle);
  preferSongMode();
  expect(selectSong).not.toHaveBeenCalled();
  Object.assign(toggle, { isNonSubscriberPlayingMusic: false });
  preferSongMode();
  expect(selectSong).toHaveBeenCalledOnce();
});
it('requests high audio with the same account entitlement and parameters as YouTube Music', async () => {
  const config = new Map<string, unknown>([['IS_SUBSCRIBER', true]]);
  window.ytcfg = {
    get: (key) => config.get(key),
    set: (key, value) => {
      config.set(key, value);
    },
  };
  document.body.append(document.createElement('ytmusic-app'));
  const navigate = vi.fn();
  document.addEventListener('yt-navigate', navigate, { once: true });
  await control({ type: 'load_track', video_id: 'dQw4w9WgXcQ' });
  expect(config.get('AUDIO_QUALITY')).toBe('AUDIO_QUALITY_HIGH');
  expect(navigate.mock.calls[0][0].detail.endpoint.watchEndpoint.videoId).toBe(
    'dQw4w9WgXcQ',
  );
  expect(trackParameters('dQw4w9WgXcQ')).toEqual(
    expect.objectContaining({
      video_id: 'dQw4w9WgXcQ',
      aac_high: true,
      audio_only: '1',
      prefer_low_quality_audio: false,
    }),
  );
  config.set('IS_SUBSCRIBER', false);
  expect(trackParameters('dQw4w9WgXcQ').aac_high).toBe(false);
});
it('reports the active audio format, rather than claiming an available Premium format is playing', () => {
  Object.assign(document.getElementById('movie_player')!, {
    getStatsForNerds: () => ({ codecs: 'av01 (399) / mp4a.40.2 (140)' }),
    getPlayerResponse: () => ({
      streamingData: {
        adaptiveFormats: [
          {
            itag: 140,
            mimeType: 'audio/mp4; codecs="mp4a.40.2"',
            averageBitrate: 128000,
            audioQuality: 'AUDIO_QUALITY_MEDIUM',
            url: 'https://private-stream.example',
          },
          {
            itag: 141,
            mimeType: 'audio/mp4; codecs="mp4a.40.2"',
            averageBitrate: 256000,
            audioQuality: 'AUDIO_QUALITY_HIGH',
          },
        ],
      },
    }),
  });
  expect(audioStatus()).toMatchObject({
    stream_quality: 'normal',
    codec: 'mp4a.40.2',
    bitrate: 128000,
  });
  expect(JSON.stringify(audioStatus())).not.toContain('private-stream');
});
it('requests high quality once for a restored Premium track and keeps its position', async () => {
  window.ytcfg = {
    get: (key) => (key === 'IS_SUBSCRIBER' ? true : 'AUDIO_QUALITY_HIGH'),
    set: vi.fn(),
  };
  const load = vi.fn();
  const play = vi.fn();
  document.querySelector('video')!.currentTime = 89;
  Object.assign(document.getElementById('movie_player')!, {
    getVideoData: () => ({ video_id: '5NV6Rdv1a3I' }),
    loadVideoByPlayerVars: load,
    playVideo: play,
    stopVideo: vi.fn(),
    clearQueue: vi.fn(),
  });
  await control({ type: 'play' });
  expect(load).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({
      video_id: '5NV6Rdv1a3I',
      start: 89,
      aac_high: true,
    }),
  );
  await control({ type: 'play' });
  expect(load).toHaveBeenCalledTimes(1);
  expect(play).toHaveBeenCalledTimes(1);
});
