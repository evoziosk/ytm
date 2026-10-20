// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { queryCatalog } from './catalog';
afterEach(() => vi.unstubAllGlobals());
it('uses an anonymous context and no host credentials for remote search and collection browsing', async () => {
  (window as any).ytcfg = {
    get: (key: string) =>
      key === 'INNERTUBE_CONTEXT'
        ? {
            client: { clientVersion: '1.0', visitorData: 'PRIVATE' },
            user: { onBehalfOfUser: 'PRIVATE' },
          }
        : 'PRIVATE',
  };
  const fetch = vi.fn().mockResolvedValue({ ok: true, json: async () => ({}) });
  vi.stubGlobal('fetch', fetch);
  for (const kind of ['public_search', 'public_browse'] as const) {
    await queryCatalog({
      kind,
      query: 'music',
      browse_id: 'MPREpublic',
      continuation: 'PRIVATE',
    });
    const [url, options] = fetch.mock.calls.at(-1)!;
    expect(url).toBe(
      `/youtubei/v1/${kind === 'public_search' ? 'search' : 'browse'}?prettyPrint=false`,
    );
    expect(options.credentials).toBe('omit');
    expect(options.headers).toEqual({ 'Content-Type': 'application/json' });
    expect(options.body).not.toContain('PRIVATE');
  }
  await expect(
    queryCatalog({
      kind: 'public_browse',
      query: '',
      browse_id: 'FEmusic_library_landing',
      continuation: '',
    }),
  ).rejects.toThrow('Invalid public collection');
  delete (window as any).ytcfg;
});
