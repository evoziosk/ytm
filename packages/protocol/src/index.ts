export * from './generated';
import type { PartyMsg, SyncFrame } from './generated';
export const VERSION = 1;
export const ROOM_ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
export const validRoom = (code: string) => /^[A-HJ-NP-Z2-9]{6,8}$/.test(code);
export const validVideo = (id: string) => /^[\w-]{11}$/.test(id);
const finite = (n: unknown): n is number =>
  typeof n === 'number' && Number.isFinite(n);
export function validFrame(value: unknown): value is SyncFrame {
  if (!value || typeof value !== 'object') return false;
  const f = value as SyncFrame;
  return (
    typeof f.video_id === 'string' &&
    validVideo(f.video_id) &&
    finite(f.position) &&
    f.position >= 0 &&
    f.position <= 604800 &&
    typeof f.playing === 'boolean' &&
    finite(f.host_ts) &&
    finite(f.seq) &&
    Number.isInteger(f.seq) &&
    f.seq >= 0 &&
    f.seq <= 4294967295
  );
}
/** Validate untrusted client input before it reaches room state. */
export function parseClientMessage(raw: string): PartyMsg | null {
  if (raw.length > 4096) return null;
  try {
    const m = JSON.parse(raw);
    if (!m || typeof m !== 'object') return null;
    switch (m.type) {
      case 'hello':
        return typeof m.name === 'string' &&
          m.name.trim().length > 0 &&
          m.name.length <= 32 &&
          m.version === VERSION &&
          typeof m.token === 'string' &&
          /^[a-f0-9]{64}$/.test(m.token) &&
          typeof m.hosting === 'boolean'
          ? m
          : null;
      case 'ping':
        return finite(m.t0) ? m : null;
      case 'sync':
        return validFrame(m.frame) ? m : null;
      case 'status':
        return ['synced', 'buffering', 'ad', 'behind'].includes(m.state) &&
          finite(m.drift_ms) &&
          Math.abs(m.drift_ms) <= 604800000
          ? m
          : null;
      case 'host_transfer':
        return typeof m.member_id === 'string' && m.member_id.length <= 64
          ? m
          : null;
      default:
        return null;
    }
  } catch {
    return null;
  }
}
