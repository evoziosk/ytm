import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  Member,
  PlaybackState,
  PlayerCommand,
  Role,
  MusicItem,
  AudioStatus,
  AccountStatus,
} from '@ytm/protocol';
export interface Settings {
  close_to_tray: boolean;
  autostart: boolean;
  window_blur: boolean;
  discord_enabled: boolean;
  discord_app_id: string;
  hide_paused: boolean;
  idle_minutes: number;
  incognito: boolean;
  blocked_artists: string[];
  blocked_playlists: string[];
  relay_url: string;
  display_name: string;
  hotkey_toggle: string;
  hotkey_next: string;
  hotkey_previous: string;
  hotkey_like: string;
  hotkey_mini: string;
  hotkey_search: string;
}
export interface PartyState {
  code: string;
  role: Role | null;
  members: Member[];
  connected: boolean;
  reconnecting: boolean;
  drift_ms: number;
  message: string;
  invite_url: string;
}
export interface Snapshot {
  player: PlaybackState;
  settings: Settings;
  party: PartyState;
  discord: string;
  bridge_connected: boolean;
  pending_room: string | null;
  queue: MusicItem[];
  queue_index: number;
  queue_playlist_id: string;
  shuffle: boolean;
  repeat: string;
  playback_error: string;
  account_message: string;
  audio: AudioStatus;
  account: AccountStatus;
}
export const native = '__TAURI_INTERNALS__' in window;
export const defaults: Settings = {
  close_to_tray: true,
  autostart: false,
  window_blur: false,
  discord_enabled: true,
  discord_app_id: '',
  hide_paused: true,
  idle_minutes: 5,
  incognito: false,
  blocked_artists: [],
  blocked_playlists: [],
  relay_url: '',
  display_name: 'Listener',
  hotkey_toggle: 'Ctrl+Shift+Space',
  hotkey_next: 'Ctrl+Shift+ArrowRight',
  hotkey_previous: 'Ctrl+Shift+ArrowLeft',
  hotkey_like: 'Ctrl+Shift+L',
  hotkey_mini: 'Ctrl+Shift+M',
  hotkey_search: 'Ctrl+Shift+K',
};
export const initial: Snapshot = {
  player: {
    track: null,
    playing: false,
    position: 0,
    duration: 0,
    volume: 1,
    muted: false,
    playback_rate: 1,
    buffering: false,
    ad: false,
    ts: 0,
  },
  settings: defaults,
  party: {
    code: '',
    role: null,
    members: [],
    connected: false,
    reconnecting: false,
    drift_ms: 0,
    message: '',
    invite_url: '',
  },
  discord: 'Not configured',
  bridge_connected: false,
  pending_room: null,
  queue: [],
  queue_index: 0,
  queue_playlist_id: '',
  shuffle: false,
  repeat: 'off',
  playback_error: '',
  account_message: '',
  account: { signed_in: false, name: '' },
  audio: {
    requested_high: false,
    premium: false,
    stream_quality: 'unknown',
    codec: '',
    bitrate: 0,
  },
};
export async function snapshot(): Promise<Snapshot> {
  return native ? invoke('get_snapshot') : structuredClone(initial);
}
export async function subscribe(callback: (snapshot: Snapshot) => void) {
  if (!native) return () => {};
  let current = await snapshot();
  const full = await listen<Snapshot>('state', (event) => {
    current = event.payload;
    callback(current);
  });
  const player = await listen<PlaybackState>('player-state', (event) => {
    current = { ...current, player: event.payload, bridge_connected: true };
    callback(current);
  });
  return () => {
    full();
    player();
  };
}
export async function call<T = void>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!native)
    throw new Error(
      'Open the desktop app to use this feature. This browser shows the interface preview.',
    );
  return invoke<T>(command, args);
}
export const send = (command: PlayerCommand) =>
  call('send_command', { command });
