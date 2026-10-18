import { expect, it } from 'vitest';
import {
  parseCatalog,
  songFirstResults,
  songSearchParams,
  continuation,
} from './catalog';
it('extracts songs and collections without confusing artist menu endpoints with track IDs', () => {
  const song = {
    musicResponsiveListItemRenderer: {
      playlistItemData: { videoId: 'dQw4w9WgXcQ' },
      flexColumns: [
        {
          musicResponsiveListItemFlexColumnRenderer: {
            text: { runs: [{ text: 'Song' }] },
          },
        },
        {
          musicResponsiveListItemFlexColumnRenderer: {
            text: { runs: [{ text: 'Artist' }] },
          },
        },
      ],
      thumbnail: {
        musicThumbnailRenderer: {
          thumbnail: { thumbnails: [{ url: 'https://example.com/art' }] },
        },
      },
    },
  };
  const album = {
    musicTwoRowItemRenderer: {
      title: { runs: [{ text: 'Album' }] },
      navigationEndpoint: { browseEndpoint: { browseId: 'MPREabc' } },
      overlay: {
        musicItemThumbnailOverlayRenderer: {
          content: {
            musicPlayButtonRenderer: {
              playNavigationEndpoint: {
                watchEndpoint: { videoId: 'Rgrt_8mXrK8' },
              },
            },
          },
        },
      },
    },
  };
  const parsed = parseCatalog(
    { contents: [song, song, album] },
    'Results',
    false,
  );
  expect(parsed.items).toHaveLength(2);
  expect(parsed.items[0]).toMatchObject({
    title: 'Song',
    subtitle: 'Artist',
    video_id: 'dQw4w9WgXcQ',
  });
  expect(parsed.items[1]).toMatchObject({
    kind: 'album',
    browse_id: 'MPREabc',
    video_id: '',
  });
});

const result = (title: string, id: string, musicVideoType: string) => ({
  musicResponsiveListItemRenderer: {
    title: { runs: [{ text: title }] },
    navigationEndpoint: {
      watchEndpoint: {
        videoId: id,
        watchEndpointMusicSupportedConfigs: {
          watchEndpointMusicConfig: { musicVideoType },
        },
      },
    },
  },
});
it('prefers song recordings and separates music videos instead of labelling them songs', () => {
  const video = result(
    'Get Lucky (Official Music Video)',
    '5NV6Rdv1a3I',
    'MUSIC_VIDEO_TYPE_OMV',
  );
  const song = result('Get Lucky', 'Rgrt_8mXrK8', 'MUSIC_VIDEO_TYPE_ATV');
  expect(parseCatalog(video, '', false).items[0].kind).toBe('video');
  const preferred = songFirstResults(
    { items: [video] },
    { items: [song] },
    'Query',
    false,
  );
  expect(preferred.items.map((item) => item.video_id)).toEqual(['Rgrt_8mXrK8']);
  expect(preferred.items[0].kind).toBe('song');
  const fallback = songFirstResults({ items: [video] }, {}, 'Query', false);
  expect(fallback.items[0].kind).toBe('video');
  expect(fallback.message).toContain('fallback');
});
it('keeps exact video-only recordings despite unrelated song suggestions and preserves live versions', () => {
  const video = result(
    'Rare Live Session',
    '5NV6Rdv1a3I',
    'MUSIC_VIDEO_TYPE_OMV',
  );
  const unrelated = result(
    'Different Song',
    'Rgrt_8mXrK8',
    'MUSIC_VIDEO_TYPE_ATV',
  );
  const page = songFirstResults(
    { items: [video] },
    { items: [unrelated] },
    'Rare Live Session',
    false,
  );
  expect(page.items[0].video_id).toBe('5NV6Rdv1a3I');
  expect(page.items).toHaveLength(2);
  const studio = result('Rare Session', 'dQw4w9WgXcQ', 'MUSIC_VIDEO_TYPE_ATV');
  expect(
    songFirstResults(
      { items: [video] },
      { items: [studio] },
      'Rare Session',
      false,
    ).items,
  ).toHaveLength(2);
});
it('reads list and action continuations without mistaking carousel pagination for a page', () => {
  const next = { nextContinuationData: { continuation: 'next-page-token' } };
  expect(
    parseCatalog(
      {
        continuationContents: {
          musicPlaylistShelfContinuation: {
            contents: [],
            continuations: [next],
          },
        },
      },
      '',
      true,
    ).continuation,
  ).toBe('next-page-token');
  expect(
    continuation({
      musicCarouselShelfRenderer: {
        musicShelfRenderer: { continuations: [next] },
      },
    }),
  ).toBe('');
  expect(
    continuation({
      onResponseReceivedActions: [
        {
          appendContinuationItemsAction: {
            continuationItems: [
              {
                continuationItemRenderer: {
                  continuationEndpoint: {
                    continuationCommand: { token: 'action-token' },
                  },
                },
              },
            ],
          },
        },
      ],
    }),
  ).toBe('action-token');
});
it('uses the website Songs filter endpoint without depending on its displayed language', () => {
  const params = 'EgWKAQIIAWoSEAQQAxAFEAkQChAQEBUQDhAR';
  expect(
    songSearchParams({
      chipCloudChipRenderer: {
        text: { simpleText: 'آواز' },
        navigationEndpoint: { searchEndpoint: { params } },
      },
    }),
  ).toBe(params);
});
it('extracts an artist from bullet-separated metadata when artist links are absent', () => {
  const raw = result('Song', 'dQw4w9WgXcQ', 'MUSIC_VIDEO_TYPE_ATV');
  Object.assign(raw.musicResponsiveListItemRenderer, {
    subtitle: { simpleText: 'Song \u2022 Artist \u2022 Album \u2022 3:20' },
  });
  expect(parseCatalog(raw, '', false).items[0].artist).toBe('Artist');
});
