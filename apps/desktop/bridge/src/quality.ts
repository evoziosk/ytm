import type { AudioStatus } from '@ytm/protocol';
import { player } from './player';
import { validVideo } from '@ytm/protocol';
import { selectors } from './selectors';

declare global {
  interface Window {
    ytcfg?: {
      get: (key: string) => unknown;
      set?: (key: string, value: unknown) => void;
    };
  }
}

// These are the same preference and player parameters used by the Music web client.
// Entitlement is read from YouTube's account context; the server controls available formats.
export function requestHighQuality() {
  window.ytcfg?.set?.('AUDIO_QUALITY', 'AUDIO_QUALITY_HIGH');
  preferSongMode();
}

let lastSongModeAttempt = 0;
export function preferSongMode() {
  const toggle = document.querySelector<
    HTMLElement & {
      playbackMode?: string;
      audioOnlyPlaybackAvailable?: boolean;
      selectedQueueItemHasCounterpart?: boolean;
      isNonSubscriberPlayingMusic?: boolean;
      onSongAvToggleTap?: () => void;
    }
  >('ytmusic-av-toggle');
  // Use the website's Song action, including its own availability checks.
  // This also selects the audio counterpart of a paired song/video queue item.
  if (
    window.ytcfg?.get('IS_SUBSCRIBER') === true &&
    toggle?.onSongAvToggleTap &&
    !toggle.isNonSubscriberPlayingMusic &&
    !document.querySelector(selectors.ad) &&
    toggle.playbackMode !== 'ATV_PREFERRED' &&
    (toggle.audioOnlyPlaybackAvailable ||
      toggle.selectedQueueItemHasCounterpart) &&
    Date.now() - lastSongModeAttempt > 1000
  ) {
    lastSongModeAttempt = Date.now();
    toggle.onSongAvToggleTap();
  }
}

let requestedTrack = '';
export function trackParameters(videoId: string, position = 0) {
  requestHighQuality();
  requestedTrack = videoId;
  return {
    video_id: videoId,
    start: position,
    external_list: true,
    suppress_creator_endscreen: true,
    audio_only: '1',
    aac_high: window.ytcfg?.get('IS_SUBSCRIBER') === true,
    prefer_low_quality_audio: false,
  };
}

// A track restored by the website may have loaded before our preference was applied.
// Request it once at the same position when a Premium listener resumes playback.
export function resumeParameters() {
  const id = player()?.getVideoData?.().video_id;
  if (
    !id ||
    !validVideo(id) ||
    id === requestedTrack ||
    window.ytcfg?.get('IS_SUBSCRIBER') !== true ||
    document.querySelector(selectors.ad)
  )
    return null;
  const position =
    document.querySelector<HTMLVideoElement>(selectors.video)?.currentTime ?? 0;
  return trackParameters(
    id,
    Number.isFinite(position) ? Math.max(0, position) : 0,
  );
}

export function audioStatus(): AudioStatus {
  const status: AudioStatus = {
    requested_high: window.ytcfg?.get('AUDIO_QUALITY') === 'AUDIO_QUALITY_HIGH',
    premium: window.ytcfg?.get('IS_SUBSCRIBER') === true,
    stream_quality: 'unknown',
    codec: '',
    bitrate: 0,
  };
  try {
    const api = player();
    // Match the active audio itag, rather than advertising the best available format.
    const codecs = api?.getStatsForNerds?.().codecs ?? '';
    const itag = [...codecs.matchAll(/\((\d+)\)/g)].at(-1)?.[1];
    const format = api
      ?.getPlayerResponse?.()
      .streamingData?.adaptiveFormats?.find(
        (item) =>
          item.itag === Number(itag) && item.mimeType?.startsWith('audio/'),
      );
    if (format) {
      status.codec = (
        format.mimeType?.match(/codecs="([^"]+)"/)?.[1] ?? ''
      ).slice(0, 128);
      const bitrate = format.averageBitrate ?? format.bitrate ?? 0;
      status.bitrate = Number.isFinite(bitrate)
        ? Math.round(Math.max(0, Math.min(10000000, bitrate)))
        : 0;
      status.stream_quality =
        format.audioQuality === 'AUDIO_QUALITY_HIGH'
          ? 'high'
          : format.audioQuality === 'AUDIO_QUALITY_MEDIUM'
            ? 'normal'
            : format.audioQuality === 'AUDIO_QUALITY_LOW'
              ? 'low'
              : 'unknown';
    }
  } catch {
    /* Quality reporting is optional while the player is starting. */
  }
  return status;
}
