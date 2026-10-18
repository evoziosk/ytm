import type { PlayerCommand, CatalogRequest } from '@ytm/protocol';
import { queryCatalog } from './catalog';
import { installLock, control, confirmedControl } from './controls';
import { observe } from './observe';

declare global {
  interface Window {
    __TAURI_INTERNALS__?: {
      invoke: (command: string, args: unknown) => Promise<unknown>;
    };
    __YTM_LISTEN_COMMAND__?: (command: PlayerCommand) => void;
    __YTM_LISTEN_REQUEST__?: (id: string, command: PlayerCommand) => void;
    __YTM_LISTEN_INSTALLED__?: boolean;
    __YTM_LISTEN_QUERY__?: (id: string, request: CatalogRequest) => void;
  }
}
// Never run in authentication pages, other domains, or subframes.
if (
  location.origin === 'https://music.youtube.com' &&
  window.top === window &&
  !window.__YTM_LISTEN_INSTALLED__
) {
  window.__YTM_LISTEN_INSTALLED__ = true;
  window.__YTM_LISTEN_QUERY__ = (id, request) => {
    void queryCatalog(request)
      .then((page) => ({ id, page, error: '' }))
      .catch((error) => ({
        id,
        page: null,
        error: error instanceof Error ? error.message : 'Music request failed',
      }))
      .then((reply) =>
        window.__TAURI_INTERNALS__?.invoke('catalog_result', { reply }),
      )
      .catch(console.warn);
  };
  window.__YTM_LISTEN_COMMAND__ = (command) => {
    void control(command).catch(console.warn);
  };
  window.__YTM_LISTEN_REQUEST__ = (id, command) => {
    void confirmedControl(command)
      .then(() => '')
      .catch((error) =>
        String(error instanceof Error ? error.message : error).slice(0, 2048),
      )
      .then((error) =>
        window.__TAURI_INTERNALS__?.invoke('player_event', {
          event: { type: 'command_reply', id, error },
        }),
      )
      .catch(console.warn);
  };
  installLock();
  const start = () =>
    observe((event) => {
      void window.__TAURI_INTERNALS__
        ?.invoke('player_event', { event })
        .catch((error) =>
          console.warn('[YTM Listen] bridge event rejected', error),
        );
    });
  if (document.readyState === 'loading')
    document.addEventListener('DOMContentLoaded', start, { once: true });
  else start();
}
