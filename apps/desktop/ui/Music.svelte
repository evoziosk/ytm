<script lang="ts">
  import { untrack, tick, onMount } from 'svelte';
  import {
    Search,
    Play,
    AudioLines,
    ArrowLeft,
    ArrowRight,
    Disc3,
    Library,
    ListMusic,
    LoaderCircle,
    RefreshCw,
    UserRound,
    ArrowUp,
    ArrowDown,
    Trash2,
    Shuffle,
    Repeat,
    Repeat1,
  } from '@lucide/svelte';
  import { call, native, mobile, type Snapshot } from './api';
  import { allowed, noDevice, queueEdit } from './playback-device';
  import type { CatalogKind, CatalogPage, MusicItem } from '@ytm/protocol';
  let {
    data,
    view = 'home',
    active = true,
    onError,
    onAccount,
    remoteQueueTotal = 0,
    onLoadDeviceQueue = async () => {},
    onRefreshDeviceQueue = async () => {},
    query = $bindable(''),
    headerSearch = false,
  }: {
    data: Snapshot;
    view?: 'home' | 'search' | 'library' | 'queue';
    active?: boolean;
    onError: (error: string) => void;
    onAccount: () => void;
    remoteQueueTotal?: number;
    onLoadDeviceQueue?: () => Promise<void>;
    onRefreshDeviceQueue?: () => Promise<void>;
    query?: string;
    headerSearch?: boolean;
  } = $props();
  let resultQuery = $state('');
  let scroll = 0;
  let wasActive = true;
  onMount(() => {
    const trackScroll = () => {
      if (active)
        scroll = document.scrollingElement?.scrollTop ?? window.scrollY;
    };
    window.addEventListener('scroll', trackScroll, { passive: true });
    return () => window.removeEventListener('scroll', trackScroll);
  });
  let page = $state<CatalogPage | null>(null);
  let loading = $state(false);
  let failure = $state('');
  let activeKind = $state<CatalogKind>('home');
  let browseId = $state('');
  let collectionTitle = $state('');
  let filter = $state('all');
  let moreLoading = $state(false);
  let moreFailure = $state('');
  let rendered = $state(100);
  type Frame = {
    query: string;
    searched: string;
    page: CatalogPage | null;
    kind: CatalogKind;
    browse: string;
    title: string;
    filter: string;
    scroll: number;
    rendered: number;
    failure: string;
  };
  const cache = new Map<string, { frame: Frame; history: Frame[] }>();
  let history = $state<Frame[]>([]);
  let accountKey = '';
  function frame(): Frame {
    return {
      query,
      searched: resultQuery,
      page: $state.snapshot(page),
      kind: activeKind,
      browse: browseId,
      title: collectionTitle,
      filter,
      scroll,
      rendered,
      failure,
    };
  }
  function restore(saved: Frame) {
    query = saved.query;
    resultQuery = saved.searched;
    page = saved.page;
    activeKind = saved.kind;
    browseId = saved.browse;
    collectionTitle = saved.title;
    filter = saved.filter;
    rendered = saved.rendered;
    failure = saved.failure;
    loading = false;
    moreLoading = false;
    moreFailure = '';
    scroll = saved.scroll;
    void tick().then(() => {
      if (document.scrollingElement)
        document.scrollingElement.scrollTop = saved.scroll;
    });
  }
  function back() {
    const saved = history.at(-1);
    if (saved) {
      requestVersion++;
      history = history.slice(0, -1);
      restore(saved);
    }
  }
  let requestVersion = 0;
  let previousView = '';
  const items = $derived(
    view === 'queue' && activeKind !== 'search'
      ? data.queue
      : (page?.items ?? []),
  );
  const visibleItems = $derived(
    items.filter((item) => filter === 'all' || item.kind === filter),
  );
  const playable = $derived(visibleItems.filter((item) => item.video_id));
  const homeLanding = $derived(
    view === 'home' && activeKind === 'home' && !browseId,
  );
  const featured = $derived(items.find((item) => item.artwork) ?? items[0]);
  const device = $derived(data.remote ?? noDevice);
  const canQueue = $derived(
    data.party.role !== 'member' && allowed(device, 'queue'),
  );
  const canPlay = $derived(canQueue && allowed(device, 'transport'));
  const title = $derived(
    activeKind === 'search'
      ? `Results for "${resultQuery}"`
      : view === 'queue'
        ? 'Up next'
        : collectionTitle ||
          (view === 'library'
            ? 'Your library'
            : view === 'search'
              ? query.trim()
                ? `Results for “${query.trim()}”`
                : 'Find your next favorite'
              : 'Your music, right here'),
  );
  async function load(kind: CatalogKind, id = '', name = '', remember = true) {
    if (
      remember &&
      (page || browseId || (view === 'queue' && data.queue.length))
    ) {
      const previous = frame();
      if (activeKind === 'search') previous.query = resultQuery;
      history = [...history, previous].slice(-30);
    }
    const version = ++requestVersion;
    loading = true;
    failure = '';
    moreFailure = '';
    moreLoading = false;
    rendered = 100;
    filter = 'all';
    activeKind = kind;
    if (kind === 'search') resultQuery = query.trim();
    browseId = id;
    collectionTitle = name;
    try {
      const response = await call<CatalogPage>('music_query', {
        request: { kind, query: query.trim(), browse_id: id, continuation: '' },
      });
      if (version === requestVersion) {
        page = response;
        scroll = 0;
        void tick().then(() => {
          if (version === requestVersion && active && document.scrollingElement)
            document.scrollingElement.scrollTop = 0;
        });
      }
    } catch (error) {
      if (version === requestVersion) {
        page = null;
        failure = String(error).replace(/^Error: /, '');
      }
    } finally {
      if (version === requestVersion) loading = false;
    }
  }
  $effect(() => {
    const target = view;
    const shown = active;
    const account = `${data.account.signed_in}:${data.account.name}:${device.active ? (device.snapshot?.session_id ?? 'pending') : 'local'}`;
    untrack(() => {
      if (target === previousView && account === accountKey) {
        if (!shown)
          cache.set(target, {
            frame: frame(),
            history: $state.snapshot(history),
          });
        else if (!wasActive) {
          const saved = cache.get(target);
          if (saved) restore(saved.frame);
        }
        wasActive = shown;
        return;
      }
      wasActive = shown;
      if (account !== accountKey) {
        cache.clear();
        accountKey = account;
      } else if (previousView)
        cache.set(previousView, {
          frame: frame(),
          history: $state.snapshot(history),
        });
      previousView = target;
      requestVersion++;
      history = [];
      const saved = cache.get(target);
      if (saved) {
        history = saved.history;
        restore(saved.frame);
        return;
      }
      restore({
        query: '',
        searched: '',
        page: null,
        kind: target === 'library' ? 'library' : 'home',
        browse: '',
        title: '',
        filter: 'all',
        scroll: 0,
        rendered: 100,
        failure: '',
      });
      if (target === 'home' || target === 'library')
        void load(target, '', '', false);
    });
  });
  async function loadMore() {
    if (!page?.continuation || moreLoading) return;
    const version = requestVersion;
    const continuation = page.continuation;
    moreLoading = true;
    moreFailure = '';
    try {
      const response = await call<CatalogPage>('music_query', {
        request: {
          kind: activeKind,
          query: resultQuery,
          browse_id: browseId,
          continuation,
        },
      });
      if (version !== requestVersion || !page) return;
      const seen = new Set(page.items.map((item) => item.key));
      const items = [
        ...page.items,
        ...response.items.filter(
          (item) => !seen.has(item.key) && Boolean(seen.add(item.key)),
        ),
      ];
      page = {
        ...page,
        items: items.slice(0, 5000),
        continuation:
          items.length >= 5000 || response.continuation === continuation
            ? ''
            : response.continuation,
        message:
          items.length >= 5000
            ? '5,000 results loaded. Search more specifically to find other music.'
            : page.message,
      };
      rendered += 100;
    } catch (error) {
      if (version === requestVersion)
        moreFailure = String(error).replace(/^Error: /, '');
    } finally {
      if (version === requestVersion) moreLoading = false;
    }
  }
  async function queueAction(action: Record<string, unknown>) {
    try {
      if (device.active) {
        if (!device.snapshot)
          throw new Error('Waiting for the playback device');
        await call('remote_request', {
          command: {
            type: 'edit_queue',
            revision: device.snapshot.queue_revision,
            edit: queueEdit(
              data.queue,
              action,
              device.active ? remoteQueueTotal : data.queue.length,
            ),
          },
        });
        return;
      }
      await call('queue_action', { action });
    } catch (error) {
      onError(String(error));
    }
  }
  function search(event: SubmitEvent) {
    event.preventDefault();
    if (query.trim()) void load('search');
  }
  export function searchFromHeader() {
    if (query.trim()) void load('search');
  }
  async function select(item: MusicItem) {
    if (!item.video_id) {
      if (item.browse_id) await load('browse', item.browse_id, item.title);
      return;
    }
    try {
      if (view === 'queue' && activeKind !== 'search') {
        await queueAction({
          type: 'select',
          index: data.queue.findIndex((song) => song.key === item.key),
        });
        return;
      }
      await call('play_items', {
        items: $state.snapshot(playable),
        index: playable.findIndex((song) => song.key === item.key),
        playlistId: page?.playlist_id || '',
      });
    } catch (error) {
      onError(String(error));
    }
  }
  async function addTrack(item: MusicItem, placement: 'now' | 'next' | 'end') {
    try {
      await call('queue_track', {
        track: {
          video_id: item.video_id,
          title: item.title,
          artist: item.artist || item.subtitle,
          artwork: item.artwork,
        },
        placement,
      });
    } catch (error) {
      onError(String(error));
    }
  }
  async function playAll() {
    try {
      if (device.active && view === 'queue' && activeKind !== 'search') {
        if (playable[0])
          await queueAction({
            type: 'select',
            index: data.queue.findIndex((item) => item.key === playable[0].key),
          });
        return;
      }
      if (playable.length)
        await call('play_items', {
          items: $state.snapshot(playable),
          index: 0,
          playlistId:
            view === 'queue' && activeKind !== 'search'
              ? data.queue_playlist_id
              : page?.playlist_id || '',
        });
    } catch (error) {
      onError(String(error));
    }
  }
</script>

<div class="music-page" class:home-landing={homeLanding}>
  {#if !headerSearch}
    <div class="music-search-row">
      <form class="music-search" onsubmit={search}>
        <Search size={19} /><input
          aria-label="Search music"
          bind:value={query}
          placeholder="Search songs, artists, albums…"
          autocomplete="off"
        /><button
          type="submit"
          disabled={loading || !query.trim()}
          aria-label="Search"
          >{#if loading}<LoaderCircle
              class="spin"
              size={17}
            />{:else}<ArrowRight size={18} />{/if}</button
        >
      </form>
    </div>
  {/if}
  <div class="music-page-heading">
    <div>
      <div class="eyebrow">
        {view === 'library'
          ? 'ALL YOUR FAVORITES, TOGETHER'
          : view === 'queue'
            ? 'KEEP THE GOOD THINGS COMING'
            : 'FIND YOUR FREQUENCY'}
      </div>
      <h1>{homeLanding ? 'Discover' : title}<span class="mint">.</span></h1>
      <p>
        {view === 'queue'
          ? 'The soundtrack you have lined up.'
          : view === 'library'
            ? mobile
              ? 'Your YouTube playlists, connected through Google.'
              : 'Albums, playlists, and music saved to your YouTube account.'
            : 'Search, discover, and play. Without leaving your listening space.'}
      </p>
    </div>
    <div class="music-heading-actions">
      {#if view === 'queue' && device.active}<button
          class="icon"
          aria-label="Refresh host queue"
          onclick={() => onRefreshDeviceQueue()}><RefreshCw size={17} /></button
        >{/if}
      {#if history.length}<button
          class="icon"
          aria-label="Back to music"
          onclick={back}><ArrowLeft size={19} /></button
        >{/if}{#if view !== 'queue'}<button
          class="icon"
          disabled={loading}
          aria-label="Refresh music"
          onclick={() => load(activeKind, browseId, collectionTitle, false)}
          ><RefreshCw size={17} /></button
        >{/if}
    </div>
  </div>
  {#if homeLanding}
    <section class="discovery-feature" aria-label="Featured music">
      <div class="discovery-feature-heading">
        <h2>In the spotlight</h2>
        <span>Something worth a listen</span>
      </div>
      <div class="discovery-feature-body">
        <button
          class="spotlight-art"
          disabled={!featured || (!canPlay && Boolean(featured.video_id))}
          aria-label={featured
            ? `Explore ${featured.title}`
            : 'Featured music will appear here'}
          onclick={() => featured && select(featured)}
        >
          {#if featured?.artwork}<img
              src={featured.artwork}
              alt=""
              referrerpolicy="no-referrer"
            />{:else}<div class="spotlight-record">
              <Disc3 size={110} strokeWidth={0.65} />
            </div>{/if}
          <span class="spotlight-shade"></span>
          <span class="spotlight-copy"
            ><small
              >{featured?.kind === 'album'
                ? 'ALBUM SPOTLIGHT'
                : featured?.kind === 'playlist'
                  ? 'PLAYLIST SPOTLIGHT'
                  : 'YOUR DAILY SOUNDTRACK'}</small
            ><strong>{featured?.title ?? 'Find your frequency.'}</strong><span
              >{featured?.artist ||
                featured?.subtitle ||
                'A whole world of music. Make it yours.'}</span
            ></span
          >
          {#if featured}<span class="spotlight-play"
              >{#if featured.video_id}<Play
                  size={21}
                  fill="currentColor"
                />{:else}<ArrowRight size={21} />{/if}</span
            >{/if}
        </button>
        <div class="spotlight-picks">
          <div class="eyebrow">ON YOUR RADAR</div>
          {#each items.slice(0, 4) as item, index (item.key)}
            <button
              class="spotlight-pick"
              disabled={!canPlay && Boolean(item.video_id)}
              onclick={() => select(item)}
              ><span class="spotlight-index"
                >{String(index + 1).padStart(2, '0')}</span
              ><span class="track-description"
                ><strong>{item.title}</strong><small
                  >{item.artist || item.subtitle || item.kind}</small
                ></span
              ><Play size={13} fill="currentColor" /></button
            >
          {:else}
            <div class="spotlight-welcome">
              <AudioLines size={24} strokeWidth={1.2} />
              <h3>A sound for every mood.</h3>
              <p>
                Search for a favorite artist or discover an album to get
                started.
              </p>
            </div>
          {/each}
        </div>
      </div>
    </section>
    {#if items.length}<div class="discovery-section-heading">
        <h2>Made for your rotation</h2>
        <span>{items.length} picks to explore</span>
      </div>{/if}
  {/if}
  {#if view === 'queue' && activeKind !== 'search' && data.queue.length}
    <div
      class="queue-options button-row"
      role="group"
      aria-label="Queue controls"
    >
      <button
        class="secondary small"
        aria-pressed={data.shuffle}
        disabled={data.party.role === 'member' || device.active}
        onclick={() => queueAction({ type: 'shuffle', enabled: !data.shuffle })}
        ><Shuffle size={15} />Shuffle {data.shuffle ? 'on' : 'off'}</button
      >
      <button
        class="secondary small"
        disabled={data.party.role === 'member' || device.active}
        onclick={() =>
          queueAction({
            type: 'repeat',
            mode:
              data.repeat === 'off'
                ? 'all'
                : data.repeat === 'all'
                  ? 'one'
                  : 'off',
          })}
        >{#if data.repeat === 'one'}<Repeat1 size={15} />{:else}<Repeat
            size={15}
          />{/if}Repeat {data.repeat}</button
      >
      <button
        class="secondary small"
        disabled={!canQueue ||
          !allowed(device, 'replace_queue') ||
          data.queue.length < 2}
        onclick={() => queueAction({ type: 'clear' })}
        ><Trash2 size={15} />Clear other tracks</button
      >
    </div>
  {/if}
  {#if data.account_message}<div class="music-session">
      <span class="status-dot" class:dim={!page?.signed_in}
      ></span>{data.account_message}
    </div>{/if}
  {#if page?.message && items.length}<p class="music-session">
      {page.message}
    </p>{/if}
  {#if !loading && !failure && items.length}
    <div class="music-filters" role="group" aria-label="Filter music">
      <div>
        {#each ['all', 'song', 'album', 'playlist', 'artist', ...(items.some((item) => item.kind === 'video') ? ['video'] : [])] as kind}<button
            class:chosen={filter === kind}
            aria-pressed={filter === kind}
            onclick={() => (filter = kind)}
            >{kind === 'all'
              ? 'All'
              : `${kind[0].toUpperCase()}${kind.slice(1)}s`}</button
          >{/each}
      </div>
      <span
        >{visibleItems.length}
        {visibleItems.length === 1 ? 'result' : 'results'}</span
      >
      {#if playable.length}<button
          class="primary small"
          disabled={!canPlay}
          onclick={playAll}
          ><Play size={13} fill="currentColor" />{device.active &&
          playable.length > 50 &&
          view !== 'queue'
            ? 'Play first 50'
            : page?.continuation
              ? 'Play loaded'
              : 'Play all'}</button
        >{/if}
    </div>
  {/if}
  {#if failure}
    <div class="music-empty">
      <span class="empty-glyph"
        >{#if view === 'library'}<Library size={34} />{:else}<AudioLines
            size={34}
          />{/if}</span
      >
      <h2>
        {view === 'library'
          ? 'Bring your library along.'
          : 'The music service needs a moment.'}
      </h2>
      <p>{failure}</p>
      <div class="button-row">
        <button
          class="primary"
          onclick={() => load(activeKind, browseId, collectionTitle, false)}
          ><RefreshCw size={15} />Try again</button
        ><button class="secondary" onclick={onAccount}
          ><UserRound size={15} />Account & connection</button
        >
      </div>
    </div>
  {:else if loading}
    <div class="catalog-loading" role="status">
      <LoaderCircle class="spin" size={20} /><span>Finding your music…</span>
    </div>
    <div class="music-grid skeleton-grid">
      {#each Array(8) as _}<div class="skeleton-card">
          <div></div>
          <span></span><small></small>
        </div>{/each}
    </div>
  {:else if !visibleItems.length}
    <div class="music-empty">
      <span class="empty-glyph"
        >{#if view === 'queue'}<ListMusic size={34} />{:else}<Search
            size={34}
          />{/if}</span
      >
      <h2>
        {filter !== 'all'
          ? `No ${filter}s on this page.`
          : view === 'queue'
            ? 'Make room for a good song.'
            : view === 'search' && !page
              ? 'What are you in the mood for?'
              : 'Nothing here just yet.'}
      </h2>
      <p>
        {filter !== 'all'
          ? 'Choose another filter to see the music already loaded.'
          : view === 'queue'
            ? 'Choose a song from search or play an album. Your queue will appear here.'
            : page?.message ||
              'Search for a song, an artist, or an album above.'}
      </p>
      {#if filter !== 'all'}<button
          class="secondary"
          onclick={() => (filter = 'all')}>Show all</button
        >{/if}
    </div>
  {:else}
    {#if filter === 'song' || view === 'queue' || (browseId && visibleItems.every((item) => item.video_id))}
      <div class="track-list">
        {#each visibleItems.slice(0, rendered) as item, index (item.key)}<div
            class="queue-track"
          >
            <button
              class="track-row"
              class:current={view === 'queue' && activeKind !== 'search'
                ? data.queue[data.queue_index]?.key === item.key
                : data.player.track?.video_id === item.video_id}
              disabled={!canPlay}
              onclick={() => select(item)}
              ><span class="track-number"
                >{#if view === 'queue' && activeKind !== 'search' ? data.queue[data.queue_index]?.key === item.key : data.player.track?.video_id === item.video_id}<AudioLines
                    size={16}
                  />{:else}{index + 1}{/if}</span
              ><span class="track-art"
                >{#if item.artwork}<img
                    src={item.artwork}
                    alt=""
                    loading="lazy"
                    referrerpolicy="no-referrer"
                  />{:else}{@render MusicFallback()}{/if}</span
              ><span class="track-description"
                ><strong>{item.title}</strong><small>{item.subtitle}</small
                ></span
              ><Play size={15} /></button
            >
            {#if item.video_id && view !== 'queue'}
              <div class="queue-edit">
                <button
                  class="secondary"
                  disabled={!canPlay}
                  onclick={() => addTrack(item, 'now')}>Play now</button
                >
                <button
                  class="secondary"
                  disabled={!canPlay}
                  onclick={() => addTrack(item, 'next')}>Play next</button
                >
                <button
                  class="secondary"
                  disabled={!canPlay}
                  onclick={() => addTrack(item, 'end')}>Add to queue</button
                >
              </div>
            {/if}
            {#if view === 'queue' && activeKind !== 'search'}
              {@const queueIndex = data.queue.findIndex(
                (song) => song.key === item.key,
              )}
              <div class="queue-edit">
                <button
                  class="icon"
                  aria-label={`Move ${item.title} up`}
                  disabled={!canQueue || queueIndex === 0}
                  onclick={() =>
                    queueAction({
                      type: 'move',
                      from: queueIndex,
                      to: queueIndex - 1,
                    })}><ArrowUp size={15} /></button
                >
                <button
                  class="icon"
                  aria-label={`Move ${item.title} down`}
                  disabled={!canQueue ||
                    queueIndex === data.queue.length - 1 ||
                    (device.active &&
                      data.queue.length < remoteQueueTotal &&
                      queueIndex === data.queue.length - 2)}
                  onclick={() =>
                    queueAction({
                      type: 'move',
                      from: queueIndex,
                      to: queueIndex + 1,
                    })}><ArrowDown size={15} /></button
                >
                <button
                  class="icon"
                  aria-label={`Remove ${item.title}`}
                  disabled={!canQueue || queueIndex === data.queue_index}
                  onclick={() =>
                    queueAction({ type: 'remove', index: queueIndex })}
                  ><Trash2 size={15} /></button
                >
              </div>
            {/if}
          </div>{/each}
      </div>
    {:else}
      <div class="music-grid">
        {#each visibleItems.slice(0, rendered) as item (item.key)}<button
            class="music-card"
            disabled={!canPlay && Boolean(item.video_id)}
            onclick={() => select(item)}
            ><span class="music-cover" class:artist={item.kind === 'artist'}
              >{#if item.artwork}<img
                  src={item.artwork}
                  alt=""
                  loading="lazy"
                  referrerpolicy="no-referrer"
                />{:else}<Disc3 size={45} strokeWidth={1} />{/if}<span
                class="cover-play"
                >{#if item.video_id}<Play
                    size={20}
                    fill="currentColor"
                  />{:else}<ArrowRight size={20} />{/if}</span
              ></span
            ><span class="music-kind">{item.kind}</span><strong
              >{item.title}</strong
            ><small
              >{item.subtitle ||
                (item.video_id ? 'Play song' : 'Explore collection')}</small
            ></button
          >{/each}
      </div>
    {/if}
  {/if}
  {#if view === 'queue' && device.active && data.queue.length < remoteQueueTotal}<button
      class="secondary"
      onclick={() => onLoadDeviceQueue()}>Load more queue</button
    >{/if}
  {#if !loading && !failure}
    <div class="catalog-pagination">
      {#if visibleItems.length > rendered}<button
          class="secondary"
          onclick={() => (rendered += 100)}>Show more loaded music</button
        >{/if}
      {#if page?.continuation && !(view === 'queue' && activeKind !== 'search')}<button
          class="secondary"
          disabled={moreLoading}
          onclick={loadMore}
          >{#if moreLoading}<LoaderCircle
              size={16}
              class="spin"
            />{/if}{moreFailure ? 'Retry loading more' : 'Load more'}</button
        >{/if}
      {#if moreFailure}<p role="alert">{moreFailure}</p>{/if}
    </div>
  {/if}
  {#if !native}<p class="muted small-text">
      Music results require the running desktop app; this preview contains no
      simulated catalog.
    </p>{/if}
</div>

{#snippet MusicFallback()}<AudioLines size={19} />{/snippet}
