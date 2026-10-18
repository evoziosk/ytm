// Keep the fragile, site-specific selectors together.
export const selectors = {
  player: '#movie_player',
  video: 'video',
  ad: '#movie_player.ad-showing, #movie_player.ad-interrupting',
  like: 'ytmusic-player-bar .like[aria-pressed], ytmusic-player-bar like-button-renderer #button-shape-like button',
  dislike:
    'ytmusic-player-bar .dislike[aria-pressed], ytmusic-player-bar like-button-renderer #button-shape-dislike button',
} as const;
