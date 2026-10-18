import type { CatalogPage, CatalogRequest, MusicItem } from '@ytm/protocol';

type Node = Record<string, any>;
const text = (value: any): string =>
  typeof value === 'string'
    ? value
    : (value?.simpleText ??
      value?.runs?.map((run: any) => run.text ?? '').join('') ??
      '');
function thumbnails(value: any, depth = 0): string {
  if (!value || typeof value !== 'object' || depth > 8) return '';
  if (Array.isArray(value.thumbnails)) {
    const url = value.thumbnails.at(-1)?.url ?? '';
    return url.startsWith('https://')
      ? url
      : url.startsWith('//')
        ? `https:${url}`
        : '';
  }
  for (const nested of Object.values(value)) {
    const found = thumbnails(nested, depth + 1);
    if (found) return found;
  }
  return '';
}
function item(renderer: Node): MusicItem | null {
  const columns = (renderer.flexColumns ?? []).map(
    (column: Node) => column.musicResponsiveListItemFlexColumnRenderer?.text,
  );
  const heading = renderer.title ?? columns[0];
  const title = text(heading).trim();
  if (!title) return null;
  const endpoint =
    renderer.navigationEndpoint ??
    heading?.runs?.find((run: Node) => run.navigationEndpoint)
      ?.navigationEndpoint ??
    {};
  const overlay =
    renderer.overlay?.musicItemThumbnailOverlayRenderer?.content
      ?.musicPlayButtonRenderer?.playNavigationEndpoint ?? {};
  const browse = endpoint.browseEndpoint?.browseId ?? '';
  const video = browse
    ? ''
    : (renderer.videoId ??
      renderer.playlistItemData?.videoId ??
      endpoint.watchEndpoint?.videoId ??
      overlay.watchEndpoint?.videoId ??
      '');
  const subtitle =
    text(renderer.subtitle ?? columns[1]) ||
    (renderer.longBylineText ? text(renderer.longBylineText) : '');
  const pageType =
    endpoint.browseEndpoint?.browseEndpointContextSupportedConfigs
      ?.browseEndpointContextMusicConfig?.pageType ?? '';
  const kind = video
    ? ((endpoint.watchEndpoint ?? overlay.watchEndpoint)
        ?.watchEndpointMusicSupportedConfigs?.watchEndpointMusicConfig
        ?.musicVideoType ??
        renderer.musicVideoType ??
        'MUSIC_VIDEO_TYPE_ATV') === 'MUSIC_VIDEO_TYPE_ATV'
      ? 'song'
      : 'video'
    : pageType.includes('ARTIST') || browse.startsWith('UC')
      ? 'artist'
      : pageType.includes('ALBUM') || browse.startsWith('MPRE')
        ? 'album'
        : 'playlist';
  if (!video && !browse) return null;
  return {
    key: video || browse,
    title: title.slice(0, 512),
    subtitle: subtitle.slice(0, 512),
    artist:
      (renderer.subtitle ?? columns[1] ?? renderer.longBylineText)?.runs
        ?.filter((run: Node) =>
          run.navigationEndpoint?.browseEndpoint?.browseId?.startsWith('UC'),
        )
        .map((run: Node) => run.text)
        .join(' & ')
        .slice(0, 512) ||
      subtitle
        .split(/\s*\u2022\s*/)
        .filter(
          (part: string) =>
            !/^(song|video|album|single|ep)$/i.test(part.trim()),
        )[0]
        ?.slice(0, 512) ||
      '',
    artwork: thumbnails(renderer.thumbnail ?? renderer.thumbnailRenderer).slice(
      0,
      2048,
    ),
    kind,
    video_id: /^[\w-]{11}$/.test(video) ? video : '',
    browse_id: /^[\w-]{1,256}$/.test(browse) ? browse : '',
  };
}
export function parseCatalog(
  raw: unknown,
  title: string,
  signedIn: boolean,
): CatalogPage {
  const items: MusicItem[] = [];
  const seen = new Set<string>();
  let visited = 0;
  function visit(node: any, depth = 0) {
    if (
      !node ||
      typeof node !== 'object' ||
      depth > 60 ||
      visited++ > 100000 ||
      items.length >= 500
    )
      return;
    for (const key of [
      'musicResponsiveListItemRenderer',
      'musicTwoRowItemRenderer',
      'playlistPanelVideoRenderer',
    ]) {
      if (node[key]) {
        const parsed = item(node[key]);
        if (parsed && !seen.has(parsed.key)) {
          seen.add(parsed.key);
          items.push(parsed);
        }
        return;
      }
    }
    for (const value of Object.values(node)) visit(value, depth + 1);
  }
  visit(raw);
  return {
    title,
    items,
    signed_in: signedIn,
    message:
      items.length >= 500
        ? 'This page reached the display limit. Use a more specific search if results are missing.'
        : items.length
          ? ''
          : 'No music was returned for this view.',
    continuation: continuation(raw),
    playlist_id: '',
  };
}
// Read the current list's continuation, never a nested Home carousel's token.
export function continuation(raw: unknown): string {
  let token = '';
  let visited = 0;
  const visit = (node: any, depth = 0) => {
    if (
      !node ||
      typeof node !== 'object' ||
      token ||
      depth > 50 ||
      visited++ > 100000
    )
      return;
    if (node.musicCarouselShelfRenderer) return;
    for (const name of [
      'sectionListRenderer',
      'musicShelfRenderer',
      'musicPlaylistShelfRenderer',
      'gridRenderer',
      'sectionListContinuation',
      'musicShelfContinuation',
      'musicPlaylistShelfContinuation',
      'gridContinuation',
    ]) {
      const list = node[name];
      const next = list?.continuations?.find(
        (entry: any) =>
          entry.nextContinuationData || entry.reloadContinuationData,
      );
      const value =
        next?.nextContinuationData?.continuation ??
        next?.reloadContinuationData?.continuation;
      if (typeof value === 'string' && value.length <= 24000) {
        token = value;
        return;
      }
    }
    const value =
      node.continuationItemRenderer?.continuationEndpoint?.continuationCommand
        ?.token;
    if (typeof value === 'string' && value.length <= 24000) {
      token = value;
      return;
    }
    for (const value of Object.values(node)) visit(value, depth + 1);
  };
  visit(raw);
  return token;
}
function cursor(endpoint: 'search' | 'browse', token: string): string {
  return token ? JSON.stringify({ endpoint, token }) : '';
}
function config(key: string): any {
  return (window as any).ytcfg?.get?.(key);
}
async function authorization(): Promise<string | null> {
  // Use this WebView's own website session. Credentials never leave this page for Rust or the UI.
  const cookies = new Map(
    document.cookie.split(';').map((value) => {
      const split = value.indexOf('=');
      return [value.slice(0, split).trim(), value.slice(split + 1)];
    }),
  );
  const sid = cookies.get('__Secure-3PAPISID') ?? cookies.get('SAPISID');
  if (!sid) return null;
  const timestamp = Math.floor(Date.now() / 1000);
  const bytes = new TextEncoder().encode(
    `${timestamp} ${sid} ${location.origin}`,
  );
  const hash = [...new Uint8Array(await crypto.subtle.digest('SHA-1', bytes))]
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
  return `SAPISIDHASH ${timestamp}_${hash}`;
}
export async function queryCatalog(
  request: CatalogRequest,
): Promise<CatalogPage> {
  const context = config('INNERTUBE_CONTEXT');
  if (!context?.client)
    throw new Error(
      'The music service is still loading. Try again in a moment.',
    );
  const signedIn = Boolean(config('LOGGED_IN'));
  if (request.kind === 'library' && !signedIn)
    throw new Error(
      'Sign in to load your YouTube Music library. You can still search and play without signing in.',
    );
  if (request.continuation) {
    let next: { endpoint: 'search' | 'browse'; token: string };
    try {
      next = JSON.parse(request.continuation);
    } catch {
      throw new Error('Invalid next page. Refresh this view.');
    }
    if (
      !['search', 'browse'].includes(next.endpoint) ||
      typeof next.token !== 'string' ||
      next.token.length > 24000
    )
      throw new Error('Invalid next page. Refresh this view.');
    const raw = await musicRequest(next.endpoint, { continuation: next.token });
    const page = parseCatalog(raw, request.query || 'More music', signedIn);
    page.continuation = cursor(next.endpoint, page.continuation);
    page.playlist_id = request.browse_id.startsWith('VL')
      ? request.browse_id.slice(2)
      : '';
    return page;
  }
  const endpoint = request.kind === 'search' ? 'search' : 'browse';
  const body: Node = {};
  if (request.kind === 'search') body.query = request.query;
  else
    body.browseId =
      request.kind === 'home'
        ? 'FEmusic_home'
        : request.kind === 'library'
          ? 'FEmusic_library_landing'
          : request.browse_id;
  const raw = await musicRequest(
    endpoint,
    body,
    request.kind === 'search' ? 8000 : 15000,
  );
  if (request.kind === 'search') {
    const params = songSearchParams(raw);
    const songRaw = await musicRequest(
      'search',
      { query: request.query, params },
      8000,
    );
    return songFirstResults(raw, songRaw, request.query, signedIn);
  }
  const page = parseCatalog(
    raw,
    request.kind === 'library'
      ? 'Your library'
      : request.kind === 'home'
        ? 'Made for this moment'
        : 'Explore',
    signedIn,
  );
  page.continuation = cursor('browse', page.continuation);
  page.playlist_id = request.browse_id.startsWith('VL')
    ? request.browse_id.slice(2)
    : '';
  return page;
}

export async function musicRequest(
  endpoint: 'search' | 'browse' | 'account/account_menu',
  body: Node,
  timeout = 15000,
): Promise<Node> {
  const context = config('INNERTUBE_CONTEXT');
  if (!context?.client) throw new Error('The music service is still loading.');
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    'X-Youtube-Client-Name': String(
      config('INNERTUBE_CONTEXT_CLIENT_NAME') ?? 67,
    ),
    'X-Youtube-Client-Version': String(context.client.clientVersion),
    'X-Goog-AuthUser': String(config('SESSION_INDEX') ?? 0),
  };
  if (config('LOGGED_IN')) {
    const auth = await authorization();
    if (auth) headers.Authorization = auth;
  }
  const response = await fetch(`/youtubei/v1/${endpoint}?prettyPrint=false`, {
    method: 'POST',
    credentials: 'include',
    headers,
    body: JSON.stringify({ context: structuredClone(context), ...body }),
    signal: AbortSignal.timeout(timeout),
  });
  if (!response.ok)
    throw new Error(
      `Music service returned ${response.status}. ${response.status === 401 || response.status === 403 ? 'Your account session may need sign-in.' : 'Please try again.'}`,
    );
  const raw = await response.json();
  if (raw.error)
    throw new Error('The music service could not complete this request.');
  return raw;
}

export function songSearchParams(raw: unknown): string {
  // Read the Songs chip's own endpoint. The prefix identifies its filter across UI languages.
  let found = '';
  let visited = 0;
  const visit = (node: any, depth = 0) => {
    if (
      !node ||
      typeof node !== 'object' ||
      found ||
      depth > 50 ||
      visited++ > 100000
    )
      return;
    const params =
      node.chipCloudChipRenderer?.navigationEndpoint?.searchEndpoint?.params;
    if (
      typeof params === 'string' &&
      params.startsWith('EgWKAQIIA') &&
      params.length < 512
    )
      found = params;
    for (const value of Object.values(node)) visit(value, depth + 1);
  };
  visit(raw);
  return found || 'EgWKAQIIAWoSEAQQAxAFEAkQChAQEBUQDhAR';
}

export function songFirstResults(
  mixedRaw: unknown,
  songRaw: unknown,
  title: string,
  signedIn: boolean,
): CatalogPage {
  const mixed = parseCatalog(mixedRaw, title, signedIn);
  const songs = parseCatalog(songRaw, title, signedIn).items.filter(
    (item) => item.kind === 'song' && item.video_id,
  );
  const preferred = [
    ...songs,
    ...mixed.items.filter((item) => item.kind === 'song' && item.video_id),
  ];
  const collections = mixed.items.filter((item) => !item.video_id);
  const fallback = mixed.items.filter(
    (item) =>
      item.kind === 'video' &&
      !preferred.some((song) => sameRecording(song, item)),
  );
  const seen = new Set<string>();
  const terms = normalize(title).split(' ').filter(Boolean);
  const score = (item: MusicItem) => {
    const value = normalize(`${item.title} ${item.artist}`);
    return terms.filter((term) => value.includes(term)).length;
  };
  const playback = [...preferred, ...fallback].filter(
    (item) => !seen.has(item.key) && Boolean(seen.add(item.key)),
  );
  playback.sort(
    (a, b) =>
      score(b) - score(a) ||
      Number(a.kind === 'video') - Number(b.kind === 'video'),
  );
  const items = [
    ...playback,
    ...collections.filter(
      (item) => !seen.has(item.key) && Boolean(seen.add(item.key)),
    ),
  ];
  return {
    ...mixed,
    items,
    continuation: cursor('search', continuation(songRaw)),
    message: fallback.length
      ? 'Song recordings are preferred. Video-only results remain available as a fallback.'
      : items.length
        ? ''
        : mixed.message,
  };
}

const normalize = (value: string) =>
  value
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, ' ')
    .trim();
function recordingTitle(item: MusicItem): string {
  return normalize(
    item.title.replace(
      /\b(official\s+(music\s+)?(video|audio|visuali[sz]er)|lyrics?\s+video|official)\b/gi,
      '',
    ),
  );
}
export function sameRecording(song: MusicItem, video: MusicItem): boolean {
  // Preserve live, remix, cover, acoustic, remaster and other version words.
  // Missing artist data never permits a different title to hide a video-only recording.
  const artist = (value: string) =>
    normalize(value.replace(/\s*(- Topic|VEVO)$/i, ''));
  return (
    recordingTitle(song) === recordingTitle(video) &&
    (!song.artist ||
      !video.artist ||
      artist(song.artist) === artist(video.artist))
  );
}
