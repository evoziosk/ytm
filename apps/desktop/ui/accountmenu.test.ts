import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import AccountMenu from './AccountMenu.svelte';
let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  document.body.innerHTML = '';
});
it('shows the signed-in account name and exposes sign out in the settings menu', () => {
  const logout = vi.fn();
  component = mount(AccountMenu, {
    target: document.body,
    props: {
      account: { signed_in: true, name: 'My music account' },
      onAccount: vi.fn(),
      onLogout: logout,
    },
  });
  flushSync();
  expect(document.querySelector('summary')?.textContent).toContain(
    'My music account',
  );
  const menu = document.querySelector('details')!;
  menu.open = true;
  const button = [...document.querySelectorAll('button')].find((button) =>
    button.textContent?.includes('Sign out'),
  )!;
  flushSync(() => button.click());
  expect(logout).toHaveBeenCalledOnce();
  expect(menu.open).toBe(false);
});
it('offers Account settings without a sign-out action when signed out', () => {
  component = mount(AccountMenu, {
    target: document.body,
    props: {
      account: { signed_in: false, name: '' },
      onAccount: vi.fn(),
      onLogout: vi.fn(),
    },
  });
  flushSync();
  expect(document.querySelector('summary')?.textContent).toContain('Account');
  expect(document.body.textContent).not.toContain('Sign out');
});
