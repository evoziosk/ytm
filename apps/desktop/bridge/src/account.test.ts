// @vitest-environment jsdom
import { expect, it } from 'vitest';
import { accountName, accountStatus } from './account';
it('reads only the active display name without exposing account email or other accounts', () => {
  const raw = {
    items: [
      { accountItemRenderer: { accountName: { simpleText: 'Other account' } } },
    ],
    header: {
      activeAccountHeaderRenderer: {
        accountName: { runs: [{ text: 'My music account' }] },
        accountEmail: { simpleText: 'private@example.com' },
      },
    },
  };
  expect(accountName(raw)).toBe('My music account');
  expect(
    accountName({
      header: {
        activeAccountHeaderRenderer: {
          accountEmail: { simpleText: 'private@example.com' },
        },
      },
    }),
  ).toBe('');
});
it('reports signed out without making an account request', async () => {
  window.ytcfg = { get: () => false };
  expect(await accountStatus()).toEqual({ signed_in: false, name: '' });
});
