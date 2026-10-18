import type { PlayerEvent } from '@ytm/protocol';
import { snapshot } from './player';
import { selectors } from './selectors';
import { audioStatus, requestHighQuality, preferSongMode } from './quality';
import { accountStatus } from './account';

export function observe(send: (event: PlayerEvent) => void): () => void {
  let video: HTMLVideoElement | null = null;
  let binding = new AbortController();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let last = '';
  let lastAudio = '';
  let lastAccount = '';
  let checkingAccount = false;
  let nextAccountCheck = 0;
  requestHighQuality();
  const publish = (seeked = false, ended = false) => {
    const state = snapshot();
    const signature = JSON.stringify({
      ...state,
      ts: 0,
      position: Math.floor(state.position),
    });
    if (signature !== last || seeked || ended) {
      last = signature;
      send({ type: ended ? 'ended' : seeked ? 'seeked' : 'snapshot', state });
    }
    const status = audioStatus();
    const audioSignature = JSON.stringify(status);
    if (audioSignature !== lastAudio) {
      lastAudio = audioSignature;
      send({ type: 'audio', status });
    }
  };
  const bind = () => {
    preferSongMode();
    const signedIn = Boolean(window.ytcfg?.get('LOGGED_IN'));
    const accountKey = String(signedIn);
    if (
      accountKey !== lastAccount ||
      (signedIn && Date.now() >= nextAccountCheck)
    ) {
      lastAccount = accountKey;
      if (!signedIn)
        send({ type: 'account', status: { signed_in: false, name: '' } });
      else if (!checkingAccount) {
        checkingAccount = true;
        nextAccountCheck = Date.now() + 60000;
        send({ type: 'account', status: { signed_in: true, name: '' } });
        void accountStatus()
          .then((status) => {
            if (status.name) nextAccountCheck = Infinity;
            send({ type: 'account', status });
          })
          .catch(console.warn)
          .finally(() => {
            checkingAccount = false;
          });
      }
    }
    const current = document.querySelector<HTMLVideoElement>(selectors.video);
    if (current !== video) {
      binding.abort();
      binding = new AbortController();
      video = current;
      for (const event of [
        'play',
        'pause',
        'seeking',
        'seeked',
        'timeupdate',
        'ratechange',
        'loadedmetadata',
        'ended',
        'waiting',
        'playing',
        'volumechange',
        'durationchange',
      ]) {
        video?.addEventListener(
          event,
          () => publish(event === 'seeked', event === 'ended'),
          {
            signal: binding.signal,
          },
        );
      }
    }
    publish();
  };
  const observer = new MutationObserver(() => {
    if (timer) return;
    timer = setTimeout(() => {
      timer = undefined;
      bind();
    }, 250);
  });
  observer.observe(document.documentElement, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ['class'],
  });
  const heartbeat = setInterval(bind, 1000);
  document.addEventListener('yt-navigate-finish', bind);
  bind();
  return () => {
    observer.disconnect();
    binding.abort();
    clearInterval(heartbeat);
    clearTimeout(timer);
    document.removeEventListener('yt-navigate-finish', bind);
  };
}
