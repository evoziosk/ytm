import { expect, it } from 'vitest';
import { parseLrc } from './lyrics';
it('parses multi-stamp lyrics and fractional seconds without treating metadata as lyrics', () => {
  expect(
    parseLrc('[ar:Artist]\n[00:03.50][01:04.005]Hello\n[00:01]First'),
  ).toEqual([
    { time: 1, text: 'First' },
    { time: 3.5, text: 'Hello' },
    { time: 64.005, text: 'Hello' },
  ]);
});
