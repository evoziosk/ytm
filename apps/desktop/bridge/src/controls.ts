import type { PlayerCommand } from '@ytm/protocol';
import { validVideo } from '@ytm/protocol';
import { player, snapshot } from './player';
import { selectors } from './selectors';
import { resumeParameters, trackParameters } from './quality';

let locked = false;
export function installLock() {
  document.addEventListener(
    'click',
    (event) => {
      if (locked && event.isTrusted) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    },
    true,
  );
  document.addEventListener(
    'keydown',
    (event) => {
      if (
        locked &&
        event.isTrusted &&
        !event.ctrlKey &&
        !event.metaKey &&
        !event.altKey
      ) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    },
    true,
  );
}
export async function control(command: PlayerCommand) {
  const api = player();
  const video = document.querySelector<HTMLVideoElement>(selectors.video);
  switch (command.type) {
    case 'play': {
      const restored = api?.loadVideoByPlayerVars ? resumeParameters() : null;
      if (restored && api?.loadVideoByPlayerVars) {
        api.stopVideo?.();
        api.clearQueue?.();
        api.loadVideoByPlayerVars(restored);
        break;
      }
      if (api?.playVideo) api.playVideo();
      else await video?.play();
      break;
    }
    case 'pause':
      if (api?.pauseVideo) api.pauseVideo();
      else video?.pause();
      break;
    case 'toggle':
      await control({ type: video?.paused ? 'play' : 'pause' });
      break;
    case 'next':
      api?.nextVideo?.();
      break;
    case 'previous':
      api?.previousVideo?.();
      break;
    case 'seek':
      if (!Number.isFinite(command.position) || command.position < 0) return;
      if (api?.seekTo) api.seekTo(command.position, true);
      else if (video) video.currentTime = command.position;
      break;
    case 'volume':
      if (Number.isFinite(command.volume)) {
        const volume = Math.min(1, Math.max(0, command.volume));
        if (api?.setVolume) api.setVolume(volume * 100);
        else if (video) video.volume = volume;
        if (volume > 0) {
          api?.unMute?.();
          if (video) video.muted = false;
        }
      }
      break;
    case 'mute':
      if (command.muted) api?.mute?.();
      else api?.unMute?.();
      if (video) video.muted = command.muted;
      break;
    case 'rate':
      if ([0.95, 1, 1.05].includes(command.rate) && video)
        video.playbackRate = command.rate;
      break;
    case 'load_track':
      if (!validVideo(command.video_id)) return;
      trackParameters(command.video_id);
      if (document.querySelector('ytmusic-app')) {
        // Let Music update its page queue and counterpart metadata along with
        // the player. Direct player loads leave that queue pointing at old music.
        document.dispatchEvent(
          new CustomEvent('yt-navigate', {
            detail: {
              endpoint: { watchEndpoint: { videoId: command.video_id } },
            },
            bubbles: true,
            composed: true,
          }),
        );
      } else
        location.assign(`/watch?v=${encodeURIComponent(command.video_id)}`);
      break;
    case 'like':
      document.querySelector<HTMLElement>(selectors.like)?.click();
      break;
    case 'dislike':
      document.querySelector<HTMLElement>(selectors.dislike)?.click();
      break;
    case 'set_locked': {
      if (
        locked === command.locked &&
        (!locked || document.getElementById('ytm-listen-lock'))
      )
        break;
      locked = command.locked;
      document.getElementById('ytm-listen-lock')?.remove();
      if (locked) {
        const badge = document.createElement('div');
        badge.id = 'ytm-listen-lock';
        badge.textContent =
          '↗ Listening together · Playback follows the host · Leave from the YTM Listen party panel';
        badge.style.cssText =
          'position:fixed;top:0;left:0;right:0;padding:8px;background:#163d32;color:#b6f5d8;text-align:center;font:12px system-ui;z-index:2147483647;pointer-events:none';
        document.body?.append(badge);
      }
      break;
    }
  }
}

const pending = new Map<string, symbol>();
export async function confirmedControl(command: PlayerCommand): Promise<void> {
  const group = ['load_track', 'next', 'previous'].includes(command.type)
    ? 'track'
    : ['play', 'pause', 'toggle'].includes(command.type)
      ? 'transport'
      : ['volume', 'mute'].includes(command.type)
        ? 'volume'
        : command.type;
  const token = Symbol();
  pending.set(group, token);
  const before = snapshot();
  try {
    if (!document.querySelector(selectors.video))
      throw new Error('The player is still loading. Please retry.');
    if (command.type === 'seek' && before.ad)
      throw new Error('Seeking is available after the advertisement.');
    if (command.type === 'like' || command.type === 'dislike') {
      const button = document.querySelector<HTMLElement>(
        selectors[command.type],
      );
      if (
        !button ||
        button.hasAttribute('disabled') ||
        button.getAttribute('aria-disabled') === 'true'
      )
        throw new Error('Sign in and start a song before rating it.');
    }
    await control(command);
    const start = performance.now();
    let matched = 0;
    while (performance.now() - start < 10000) {
      if (pending.get(group) !== token) throw new Error('Superseded');
      const state = snapshot();
      const current = state.track?.video_id;
      let ready = false;
      switch (command.type) {
        case 'load_track': {
          const requested =
            new URL(location.href).searchParams.get('v') === command.video_id;
          ready = Boolean(
            state.playing &&
            (current === command.video_id ||
              (requested &&
                (state.ad || (current && current !== before.track?.video_id)))),
          );
          const status = player()?.getPlayerResponse?.()?.playabilityStatus;
          if (
            current === command.video_id &&
            status?.status &&
            !['OK', 'LIVE_STREAM_OFFLINE'].includes(status.status)
          )
            throw new Error(
              status.reason ||
                'This recording is unavailable for your account. Choose another result.',
            );
          break;
        }
        case 'play':
          ready = state.playing;
          break;
        case 'pause':
          ready = !state.playing;
          break;
        case 'toggle':
          ready = state.playing !== before.playing;
          break;
        case 'seek':
          ready =
            Math.abs(
              state.position -
                Math.min(command.position, state.duration || command.position),
            ) < 1.5 && state.track?.video_id === before.track?.video_id;
          break;
        case 'volume':
          ready =
            Math.abs(state.volume - command.volume) < 0.02 &&
            (command.volume === 0 || !state.muted);
          break;
        case 'mute':
          ready = state.muted === command.muted;
          break;
        case 'next':
        case 'previous':
          ready = Boolean(
            current && current !== before.track?.video_id && state.playing,
          );
          break;
        default:
          ready = true;
      }
      matched = ready ? matched + 1 : 0;
      if (matched >= (command.type === 'seek' ? 2 : 1)) return;
      await new Promise((resolve) => setTimeout(resolve, 80));
    }
    throw new Error(
      'The player could not complete this request. Check your connection, then retry.',
    );
  } finally {
    if (pending.get(group) === token) pending.delete(group);
  }
}
