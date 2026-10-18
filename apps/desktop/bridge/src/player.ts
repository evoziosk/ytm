import type { PlaybackState } from '@ytm/protocol';
import { validVideo } from '@ytm/protocol';
import { selectors } from './selectors';

export interface YTPlayer extends HTMLElement {
  getVideoData?: () => { video_id?: string; title?: string; author?: string };
  playVideo?: () => void;
  pauseVideo?: () => void;
  stopVideo?: () => void;
  clearQueue?: () => void;
  seekTo?: (seconds: number, allowSeekAhead: boolean) => void;
  nextVideo?: () => void;
  previousVideo?: () => void;
  setVolume?: (volume: number) => void;
  mute?: () => void;
  unMute?: () => void;
  setPlaybackRate?: (rate: number) => void;
  loadVideoById?: (id: string) => void;
  loadVideoByPlayerVars?: (vars: Record<string, unknown>) => void;
  getStatsForNerds?: () => { codecs?: string };
  getPlayerResponse?: () => {
    playabilityStatus?: { status?: string; reason?: string };
    streamingData?: {
      adaptiveFormats?: Array<{
        itag?: number;
        audioQuality?: string;
        averageBitrate?: number;
        bitrate?: number;
        mimeType?: string;
      }>;
    };
  };
}
export const player = () => document.querySelector<YTPlayer>(selectors.player);
const safe = (v: unknown, fallback = 0) =>
  typeof v === 'number' && Number.isFinite(v) ? Math.max(0, v) : fallback;
export function snapshot(now = Date.now()): PlaybackState {
  const video = document.querySelector<HTMLVideoElement>(selectors.video);
  const data = player()?.getVideoData?.();
  const metadata = navigator.mediaSession?.metadata;
  const id = data?.video_id;
  const art = metadata?.artwork?.at(-1)?.src ?? '';
  return {
    track:
      id && validVideo(id)
        ? {
            video_id: id,
            title: (data?.title || metadata?.title || 'Unknown track').slice(
              0,
              512,
            ),
            artist: (data?.author || metadata?.artist || '').slice(0, 512),
            album: (metadata?.album || '').slice(0, 512),
            artwork: /^https:\/\//.test(art) ? art.slice(0, 2048) : '',
            playlist_id:
              new URL(location.href).searchParams.get('list')?.slice(0, 128) ??
              '',
          }
        : null,
    playing: Boolean(video && !video.paused && !video.ended),
    position: safe(video?.currentTime),
    duration: safe(video?.duration),
    volume: safe(video?.volume, 1),
    muted: Boolean(video?.muted),
    playback_rate: safe(video?.playbackRate, 1),
    buffering: Boolean(video && (video.seeking || video.readyState < 3)),
    ad: Boolean(document.querySelector(selectors.ad)),
    ts: now,
  };
}
