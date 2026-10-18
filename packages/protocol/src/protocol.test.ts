import { describe, expect, it } from 'vitest';
import { parseClientMessage, validRoom } from './index';
describe('untrusted room messages', () => {
  it('rejects oversized, nonfinite and server-only messages', () => {
    expect(parseClientMessage(' '.repeat(4097))).toBeNull();
    expect(parseClientMessage('{"type":"ping","t0":1e999}')).toBeNull();
    expect(parseClientMessage('{"type":"welcome"}')).toBeNull();
    expect(parseClientMessage('{"type":"sync","frame":null}')).toBeNull();
  });
  it('checks protocol versions and reconnect credentials', () => {
    const hello = {
      type: 'hello',
      version: 1,
      name: 'Listener',
      hosting: false,
      token: 'a'.repeat(64),
    };
    expect(parseClientMessage(JSON.stringify(hello))).toEqual(hello);
    expect(
      parseClientMessage(JSON.stringify({ ...hello, version: 2 })),
    ).toBeNull();
    expect(
      parseClientMessage(JSON.stringify({ ...hello, token: 'guess' })),
    ).toBeNull();
    expect(validRoom('ABC234')).toBe(true);
    expect(validRoom('AB0I23')).toBe(false);
  });
});
