import type { AccountStatus } from '@ytm/protocol';
import { musicRequest } from './catalog';

export function accountName(raw: unknown): string {
  let name = '';
  let visited = 0;
  const visit = (node: any, depth = 0) => {
    if (
      !node ||
      typeof node !== 'object' ||
      name ||
      depth > 50 ||
      visited++ > 30000
    )
      return;
    const value = node.activeAccountHeaderRenderer?.accountName;
    if (value)
      name =
        typeof value.simpleText === 'string'
          ? value.simpleText
          : (value.runs?.map((run: any) => run.text ?? '').join('') ?? '');
    for (const child of Object.values(node)) visit(child, depth + 1);
  };
  visit(raw);
  return name.trim().slice(0, 128);
}
export async function accountStatus(): Promise<AccountStatus> {
  const signedIn = Boolean(window.ytcfg?.get('LOGGED_IN'));
  if (!signedIn) return { signed_in: false, name: '' };
  const raw = await musicRequest('account/account_menu', {}, 10000);
  return { signed_in: true, name: accountName(raw) };
}
