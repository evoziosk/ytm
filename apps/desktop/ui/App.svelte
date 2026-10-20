<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { version as appVersion } from '../package.json';
  import {
    AudioLines,
    ArrowUpRight,
    ArrowRight,
    Check,
    CheckCheck,
    ChevronRight,
    CircleHelp,
    Copy,
    Crown,
    Disc3,
    MonitorSmartphone,
    EyeOff,
    Headphones,
    Heart,
    Keyboard,
    LayoutDashboard,
    Search,
    Library,
    ListMusic,
    UserRound,
    LoaderCircle,
    MessageCircle,
    MicVocal,
    Music2,
    Pause,
    Play,
    Plus,
    Radio,
    ShieldCheck,
    SkipBack,
    SkipForward,
    SlidersHorizontal,
    Users,
    Volume2,
    VolumeX,
    X,
  } from '@lucide/svelte';
  import Toggle from './Toggle.svelte';
  import Music from './Music.svelte';
  import SeekBar from './SeekBar.svelte';
  import AccountMenu from './AccountMenu.svelte';
  import Sharing from './Sharing.svelte';
  import AudioSharing from './AudioSharing.svelte';
  import DeviceConnection from './DeviceConnection.svelte';
  import {
    playbackView,
    allowed,
    noDevice,
    queueEdit,
    type PlaybackDevice,
    type DeviceQueue,
  } from './playback-device';
  import { listen } from '@tauri-apps/api/event';
  import {
    call,
    defaults,
    initial,
    native,
    mobile,
    send,
    snapshot,
    subscribe,
    type Settings,
    type Snapshot,
  } from './api';
  import { parseLrc, type Line } from './lyrics';
  import type { PlayerCommand } from '@ytm/protocol';

  const panel = new URLSearchParams(location.search).get('panel') ?? 'settings';
  type Tab =
    | 'overview'
    | 'search'
    | 'library'
    | 'queue'
    | 'discord'
    | 'party'
    | 'remote'
    | 'group'
    | 'hotkeys'
    | 'general'
    | 'audio'
    | 'about';
  let tab = $state<Tab>(panel === 'party' ? 'party' : 'overview');
  let musicView = $state<'home' | 'search' | 'library' | 'queue'>('home');
  let searchQuery = $state('');
  let musicComponent: Music | undefined = $state();
  async function searchCatalog(event: SubmitEvent) {
    event.preventDefault();
    const submitted = searchQuery.trim();
    if (!submitted) return;
    tab = 'search';
    await tick();
    searchQuery = submitted;
    await tick();
    musicComponent?.searchFromHeader();
  }
  $effect(() => {
    if (
      tab === 'overview' ||
      tab === 'search' ||
      tab === 'library' ||
      tab === 'queue'
    )
      musicView = tab === 'overview' ? 'home' : tab;
  });
  let localData = $state<Snapshot>(structuredClone(initial));
  let device = $state<PlaybackDevice>({ ...noDevice });
  let deviceQueue = $state<DeviceQueue | null>(null);
  let devicePickerOpen = $state(false);
  let devicePolling = false;
  let queueLoading = $state(false);
  const data = $derived(playbackView(localData, device, deviceQueue));
  const remoteReady = $derived(
    !device.active ||
      Boolean(
        device.snapshot &&
        device.participant?.approved &&
        device.message === 'Connected',
      ),
  );
  async function loadDeviceQueue(more = false) {
    if (!device.active || !device.snapshot || !remoteReady || queueLoading)
      return;
    const session = device.snapshot.session_id;
    const revision = device.snapshot.queue_revision;
    const offset =
      more && deviceQueue?.revision === revision
        ? deviceQueue.entries.length
        : 0;
    queueLoading = true;
    try {
      const result = await call<DeviceQueue>('remote_request', {
        command: { type: 'queue', offset, limit: 50 },
      });
      if (
        device.active &&
        device.snapshot?.session_id === session &&
        device.snapshot.queue_revision === revision &&
        result.type === 'queue' &&
        result.revision === revision
      ) {
        deviceQueue = {
          ...result,
          entries:
            offset && deviceQueue?.revision === revision
              ? [...deviceQueue.entries, ...result.entries]
              : result.entries,
        };
      }
    } catch (e) {
      if (device.active) error = String(e);
    } finally {
      queueLoading = false;
    }
  }
  function acceptDevice(status: PlaybackDevice) {
    if (!status || typeof status.active !== 'boolean') return;
    if (
      device.snapshot?.session_id !== status.snapshot?.session_id ||
      !status.active ||
      !status.snapshot
    )
      deviceQueue = null;
    device = status;
  }
  async function refreshDevice() {
    if (!native || devicePolling) return;
    devicePolling = true;
    try {
      acceptDevice(await call<PlaybackDevice>('remote_status'));
    } finally {
      devicePolling = false;
    }
  }
  let requestedQueueKey = '';
  $effect(() => {
    if (!device.active || !device.snapshot) {
      requestedQueueKey = '';
      return;
    }
    const key = `${device.snapshot.session_id}:${device.snapshot.queue_revision}`;
    if (remoteReady && !queueLoading && key !== requestedQueueKey) {
      requestedQueueKey = key;
      void loadDeviceQueue();
    }
  });
  let draft = $state<Settings>(structuredClone(defaults));
  let ready = $state(false);
  let busy = $state(false);
  let appearanceBusy = $state(false);
  let error = $state('');
  let toast = $state('');
  let accountOpen = $state(false);

  let room = $state('');
  let clock = $state(Date.now());
  let lyrics = $state<Line[]>([]);
  let plainLyrics = $state('');
  let lyricsStatus = $state('Play a song, then look up its lyrics.');
  let lyricsLoading = $state(false);
  let lastLyricsTrack = '';
  let lastPendingRoom: string | null = null;
  const dirty = $derived(
    ready && JSON.stringify(draft) !== JSON.stringify(data.settings),
  );
  const track = $derived(data.player.track);
  const blurred = $derived(panel === 'settings' && data.settings.window_blur);
  $effect(() => {
    document.documentElement.classList.toggle('app-blur', blurred);
  });
  onDestroy(() => document.documentElement.classList.remove('app-blur'));
  const locked = $derived(data.party.role === 'member');
  const elapsed = $derived(
    data.player.playing && !data.player.buffering
      ? Math.max(0, (clock - data.player.ts) / 1000) * data.player.playback_rate
      : 0,
  );
  const position = $derived(
    Math.min(data.player.duration || Infinity, data.player.position + elapsed),
  );
  const activeLine = $derived(
    lyrics.reduce((last, line, i) => (line.time <= position ? i : last), -1),
  );
  const nav = [
    { id: 'overview', label: 'Home', icon: LayoutDashboard },
    { id: 'search', label: 'Search', icon: Search },
    { id: 'library', label: 'Your library', icon: Library },
    { id: 'queue', label: 'Play queue', icon: ListMusic },
    ...(!mobile
      ? [
          {
            id: 'discord',
            label: 'Discord presence',
            icon: MessageCircle,
          } as const,
        ]
      : []),
    { id: 'party', label: 'Listen together', icon: Users },
    { id: 'remote', label: 'Remote control', icon: Radio },
    { id: 'group', label: 'Group control', icon: Headphones },
    ...(!mobile
      ? [
          {
            id: 'hotkeys',
            label: 'Keyboard shortcuts',
            icon: Keyboard,
          } as const,
        ]
      : []),
    { id: 'general', label: 'Preferences', icon: SlidersHorizontal },
    ...(!mobile
      ? [{ id: 'audio', label: 'Virtual microphone', icon: MicVocal } as const]
      : []),
  ] as const;
  const shortcuts: {
    key: keyof Pick<
      Settings,
      | 'hotkey_toggle'
      | 'hotkey_next'
      | 'hotkey_previous'
      | 'hotkey_like'
      | 'hotkey_mini'
      | 'hotkey_search'
    >;
    title: string;
    description: string;
  }[] = [
    {
      key: 'hotkey_toggle',
      title: 'Play / pause',
      description: 'A little pause, whenever you need it.',
    },
    {
      key: 'hotkey_next',
      title: 'Next track',
      description: 'On to the next good thing.',
    },
    {
      key: 'hotkey_previous',
      title: 'Previous track',
      description: 'That one deserves another listen.',
    },
    {
      key: 'hotkey_like',
      title: 'Like this track',
      description: 'Keep your favorites close.',
    },
    {
      key: 'hotkey_search',
      title: 'Quick search overlay',
      description:
        'Find and play a song from any app using only your keyboard.',
    },
    {
      key: 'hotkey_mini',
      title: 'Toggle mini-player',
      description: 'Your music, in a smaller space.',
    },
  ];
  function accept(next: Snapshot) {
    const unchanged =
      JSON.stringify(draft) === JSON.stringify(localData.settings);
    localData = next;
    if (!ready || unchanged) draft = structuredClone(next.settings);
    ready = true;
    if (next.pending_room && next.pending_room !== lastPendingRoom) {
      room = next.pending_room;
      tab = 'party';
    }
    lastPendingRoom = next.pending_room;
    if (next.player.track?.video_id !== lastLyricsTrack) {
      lyrics = [];
      plainLyrics = '';
      lyricsStatus = 'Look up lyrics for the current song.';
    }
  }
  onMount(() => {
    let dispose = () => {};
    let disposed = false;
    void (async () => {
      dispose = await subscribe(accept);
      if (disposed) {
        dispose();
        return;
      }
      accept(await snapshot());
    })().catch((e) => (error = String(e)));
    const timer = setInterval(() => (clock = Date.now()), 250);
    let disposeDevice = () => {};
    if (native) {
      void refreshDevice().catch(() => {});
      void listen<PlaybackDevice>('remote-state', (event) => {
        if (!disposed) acceptDevice(event.payload);
      })
        .then((dispose) => {
          if (disposed) dispose();
          else disposeDevice = dispose;
        })
        .catch(() => {});
    }
    const remoteTimer = native
      ? setInterval(() => {
          if (!disposed) void refreshDevice().catch(() => {});
        }, 1500)
      : undefined;
    return () => {
      disposed = true;
      dispose();
      clearInterval(timer);
      clearInterval(remoteTimer);
      disposeDevice();
    };
  });
  async function action(fn: () => Promise<unknown>) {
    error = '';
    try {
      await fn();
    } catch (e) {
      error = String(e).replace(/^Error: /, '');
    }
  }
  function notify(message: string) {
    toast = message;
    setTimeout(() => {
      if (toast === message) toast = '';
    }, 3500);
  }
  async function save(): Promise<boolean> {
    busy = true;
    error = '';
    try {
      await call('save_settings', { settings: $state.snapshot(draft) });
      localData = {
        ...localData,
        settings: structuredClone($state.snapshot(draft)),
      };
      notify('Preferences saved');
      return true;
    } catch (e) {
      error = String(e).replace(/^Error: /, '');
      return false;
    } finally {
      busy = false;
    }
  }
  async function setWindowBlur(enabled: boolean) {
    if (appearanceBusy || busy) return;
    appearanceBusy = true;
    error = '';
    try {
      if (native) await call('set_window_blur', { enabled });
      localData = {
        ...localData,
        settings: { ...localData.settings, window_blur: enabled },
      };
      draft.window_blur = enabled;
    } catch (e) {
      error = String(e).replace(/^Error: /, '');
    } finally {
      appearanceBusy = false;
    }
  }
  async function party(hosting: boolean) {
    if (dirty && !(await save())) return;
    let code = room.trim();
    if (code.includes('://')) {
      try {
        code = new URL(code).pathname.split('/').filter(Boolean).at(-1) ?? '';
      } catch {
        /* Rust validates the code. */
      }
    }
    busy = true;
    await action(() => call('party_connect', { code, hosting }));
    busy = false;
  }
  function command(command: PlayerCommand) {
    void action(() => send(command));
  }
  function playFromHomeQueue(key: string) {
    if (locked || !allowed(device, 'queue') || !allowed(device, 'transport'))
      return;
    const index = data.queue.findIndex((item) => item.key === key);
    if (index < 0) return;
    void action(async () => {
      if (device.active) {
        if (!device.snapshot) return;
        await call('remote_request', {
          command: {
            type: 'edit_queue',
            revision: device.snapshot.queue_revision,
            edit: queueEdit(data.queue, { type: 'select', index }),
          },
        });
      } else {
        await call('queue_action', { action: { type: 'select', index } });
      }
    });
  }
  function logout() {
    void action(async () => {
      await call('logout_account');
      accountOpen = false;
      notify(
        mobile
          ? 'Google library disconnected.'
          : 'Signing out of YouTube Music…',
      );
    });
  }
  const time = (seconds: number) =>
    `${Math.floor(Math.max(0, seconds) / 60)}:${Math.floor(
      Math.max(0, seconds) % 60,
    )
      .toString()
      .padStart(2, '0')}`;
  const listText = (text: string) =>
    text
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean);
  async function loadLyrics() {
    lyricsLoading = true;
    lyricsStatus = 'Finding the words…';
    const requested = track?.video_id;
    lastLyricsTrack = requested ?? '';
    try {
      const result = await call<{
        syncedLyrics: string | null;
        plainLyrics: string | null;
        instrumental: boolean;
      }>('get_lyrics');
      if (requested !== data.player.track?.video_id) return;
      lyrics = parseLrc(result.syncedLyrics ?? '');
      plainLyrics = result.plainLyrics ?? '';
      lyricsStatus = result.instrumental
        ? 'Just the music. This track is instrumental.'
        : lyrics.length || plainLyrics
          ? ''
          : 'No lyrics found for this song.';
    } catch (e) {
      lyricsStatus = String(e).replace(/^Error: /, '');
    } finally {
      lyricsLoading = false;
    }
  }
</script>

{#snippet artwork(size = '')}
  <div class="artwork {size}">
    {#if track?.artwork}<img
        src={track.artwork}
        alt="Album artwork"
        referrerpolicy="no-referrer"
      />{:else}<AudioLines
        size={size === 'large' ? 42 : 22}
        strokeWidth={1.5}
      />{/if}
  </div>
{/snippet}
{#snippet transport()}
  <div class="transport">
    <button
      class="icon"
      aria-label="Previous track"
      disabled={!track || locked || data.player.ad || !allowed(device, 'skip')}
      onclick={() => command({ type: 'previous' })}
      ><SkipBack size={18} fill="currentColor" /></button
    >
    <button
      class="play-button"
      aria-label={data.player.playing ? 'Pause' : 'Play'}
      disabled={!track || locked || !allowed(device, 'transport')}
      onclick={() => command({ type: 'toggle' })}
      >{#if data.player.playing}<Pause
          size={19}
          fill="currentColor"
        />{:else}<Play size={19} fill="currentColor" />{/if}</button
    >
    <button
      class="icon"
      aria-label="Next track"
      disabled={!track || locked || data.player.ad || !allowed(device, 'skip')}
      onclick={() => command({ type: 'next' })}
      ><SkipForward size={18} fill="currentColor" /></button
    >
  </div>
{/snippet}
{#snippet progress()}
  <SeekBar
    player={data.player}
    {position}
    {clock}
    locked={locked || !allowed(device, 'seek')}
    onError={(message) => (error = message)}
  />
{/snippet}

{#if panel === 'mini'}
  <main class="mini">
    <div class="mini-drag" data-tauri-drag-region>
      <span data-tauri-drag-region><AudioLines size={13} /> YTM LISTEN</span
      ><button
        class="icon"
        aria-label="Hide mini-player"
        onclick={() => action(() => getCurrentWindow().hide())}
        ><X size={15} /></button
      >
    </div>
    <div class="mini-track">
      {@render artwork()}
      <div class="truncate">
        <strong>{track?.title ?? 'Make yourself at home'}</strong>
        <p>
          {data.player.ad
            ? 'Advertisement - your music resumes shortly'
            : (track?.artist ?? 'Choose a song in YTM Listen')}
        </p>
      </div>
      <button
        class="icon"
        aria-label="Open music"
        onclick={() => action(() => call('show_music'))}
        ><ArrowUpRight size={19} /></button
      >
    </div>
    {@render transport()}{@render progress()}
    {#if locked}<small class="mini-follow">Following your party host</small
      >{/if}
  </main>
{:else if panel === 'lyrics'}
  <main class="lyrics-panel">
    <div class="eyebrow"><MicVocal size={15} /> THE WORDS BEHIND THE MUSIC</div>
    <h1>{track?.title ?? 'Every song has a story.'}</h1>
    <p class="muted">{track?.artist ?? 'Choose something you love.'}</p>
    <button
      class="secondary"
      disabled={!track || lyricsLoading}
      onclick={loadLyrics}
      >{#if lyricsLoading}<LoaderCircle
          class="spin"
          size={16}
        />{:else}<MicVocal size={16} />{/if}Find lyrics</button
    >
    {#if lyricsStatus}<p class="lyrics-status">{lyricsStatus}</p>{/if}
    <div class="lyrics-lines">
      {#each lyrics as line, i}<p class:current={i === activeLine}>
          {line.text || '♪'}
        </p>{/each}{#if !lyrics.length}<p class="plain-lyrics">
          {plainLyrics}
        </p>{/if}
    </div>
    <small class="muted"
      >Lyrics provided by LRCLIB · Fetched only when you ask</small
    >
  </main>
{:else}
  <div class="app-shell" class:mobile-app={mobile}>
    <aside class="sidebar">
      <a href="#overview" class="brand" onclick={() => (tab = 'overview')}
        ><span class="brand-mark"><AudioLines size={25} strokeWidth={2} /></span
        ><span
          >ytm<span class="brand-light">listen</span><span class="brand-dot"
            >.</span
          ></span
        ></a
      >
      <div class="workspace-label">BROWSE MUSIC</div>
      <nav aria-label="Main navigation">
        {#each nav as item, index}
          {#if index === 4}<div class="nav-section-label">
              STAY CONNECTED
            </div>{/if}
          {#if item.id === 'hotkeys' || (mobile && item.id === 'general')}<div
              class="nav-section-label"
            >
              YOUR SETUP
            </div>{/if}
          <button class:active={tab === item.id} onclick={() => (tab = item.id)}
            ><item.icon size={18} strokeWidth={1.7} />{item.label}</button
          >{/each}
      </nav>
      <div class="sidebar-bottom">
        <div class="sidebar-track">
          {@render artwork()}
          <div class="truncate">
            <strong>{track?.title ?? 'Your listening space'}</strong>
            <p>{track?.artist ?? 'Make it your own.'}</p>
          </div>
          {#if track && !device.active}<button
              class="icon"
              aria-label="Like song from sidebar"
              onclick={() => command({ type: 'like' })}
              ><Heart size={15} /></button
            >{/if}
        </div>
        <div class="quiet-card">
          <span class="quiet-icon"><Headphones size={21} /></span><strong
            >A space for your music.</strong
          >
          <p>No extra noise.<br />Just a better way to listen.</p>
        </div>
        <button class="about-link" onclick={() => (tab = 'about')}
          ><CircleHelp size={16} /> About YTM Listen
          <span>{appVersion}</span></button
        >
        <div class="build-label">
          <span class="status-dot"></span>{native
            ? mobile
              ? 'YTM LISTEN'
              : 'DESKTOP COMPANION'
            : 'INTERFACE PREVIEW'}
        </div>
      </div>
    </aside>
    <div class="main-column">
      <header class="topbar">
        <div class="topbar-browse">
          <button
            class="icon back-home"
            aria-label="Back to Home"
            disabled={tab === 'overview'}
            onclick={() => (tab = 'overview')}><ArrowRight size={17} /></button
          >
          <form class="header-search" onsubmit={searchCatalog}>
            <Search size={15} /><input
              aria-label="Search music"
              bind:value={searchQuery}
              placeholder="Search your next favorite"
              autocomplete="off"
            /><button
              aria-label="Search"
              disabled={!searchQuery.trim()}
              type="submit"><ArrowRight size={14} /></button
            >
          </form>
        </div>
        <div class="discovery-tabs" role="group" aria-label="Browse music">
          <button
            class:chosen={tab === 'overview'}
            onclick={() => (tab = 'overview')}>Discover</button
          ><button
            class:chosen={tab === 'library'}
            onclick={() => (tab = 'library')}>Your library</button
          ><button
            class:chosen={tab === 'queue'}
            onclick={() => (tab = 'queue')}>Queue</button
          >
        </div>
        <AccountMenu
          account={data.account}
          onAccount={() => (accountOpen = true)}
          onLogout={logout}
        />
      </header>
      <main class="content">
        {#if !mobile && panel === 'settings'}
          <AudioSharing
            visible={tab === 'audio'}
            onOpen={() => (tab = 'audio')}
          />
        {/if}
        {#if device.active}<section
            class="remote-banner"
            aria-label="Playback device"
          >
            <button
              class="text-button"
              onclick={() => (devicePickerOpen = true)}
              ><MonitorSmartphone size={18} /> Playback device: {device.target}</button
            >
            <span role="status">{device.message}</span>
          </section>{/if}
        {#if !native}<div class="preview-note">
            <Disc3 size={14} /> Interface preview. Playback and integrations run in
            the desktop app.
          </div>{/if}
        <div
          class="music-workspace"
          hidden={!(
            tab === 'overview' ||
            tab === 'search' ||
            tab === 'library' ||
            tab === 'queue'
          )}
        >
          <Music
            bind:this={musicComponent}
            bind:query={searchQuery}
            headerSearch
            {data}
            remoteQueueTotal={deviceQueue?.total ?? 0}
            onLoadDeviceQueue={() => loadDeviceQueue(true)}
            onRefreshDeviceQueue={() => loadDeviceQueue()}
            view={musicView}
            active={tab === 'overview' ||
              tab === 'search' ||
              tab === 'library' ||
              tab === 'queue'}
            onError={(message) => (error = message)}
            onAccount={() => (accountOpen = true)}
          />
          <aside class="listening-rail" aria-label="Your listening space">
            <div class="rail-heading">
              <h2>Now playing</h2>
              <button
                class="icon"
                aria-label="Show playback queue"
                onclick={() => (tab = 'queue')}><ListMusic size={17} /></button
              >
            </div>
            <section class="listening-card">
              {#if track?.artwork}
                <div
                  class="listening-haze"
                  style:background-image={`url("${track.artwork}")`}
                ></div>
              {/if}
              <div class="listening-heading">
                <span class="eyebrow">NOW PLAYING</span><AudioLines size={17} />
              </div>
              {@render artwork('large')}
              <div class="listening-title">
                <h2>{track?.title ?? 'Set the mood.'}</h2>
                <p>
                  {data.player.ad
                    ? 'Advertisement'
                    : (track?.artist ?? 'Your next favorite is a play away.')}
                </p>
              </div>
              <button
                class="play-button listening-play"
                aria-label={data.player.playing
                  ? 'Pause now playing'
                  : 'Play now playing'}
                disabled={!track || locked || !allowed(device, 'transport')}
                onclick={() => command({ type: 'toggle' })}
                >{#if data.player.playing}<Pause
                    size={23}
                    fill="currentColor"
                  />{:else}<Play size={23} fill="currentColor" />{/if}</button
              >
              <div class="listening-caption">
                <Headphones size={13} /><span
                  >{data.player.playing
                    ? 'A little closer to the music.'
                    : 'Make yourself at home.'}</span
                >
              </div>
            </section>
            <section class="listening-queue" aria-label="Queue preview">
              <div class="section-heading">
                <h3>
                  Up next <span
                    >{device.active
                      ? (deviceQueue?.total ?? 0)
                      : data.queue.length}</span
                  >
                </h3>
                <button
                  class="icon"
                  aria-label="View full queue"
                  onclick={() => (tab = 'queue')}
                  ><ArrowUpRight size={17} /></button
                >
              </div>
              {#each data.queue.slice(data.queue_index + 1, data.queue_index + 5) as item}
                <button
                  class="listening-queue-row"
                  aria-label={`Play ${item.title} from Up next`}
                  disabled={locked ||
                    !allowed(device, 'queue') ||
                    !allowed(device, 'transport')}
                  onclick={() => playFromHomeQueue(item.key)}
                >
                  <span class="track-art"
                    >{#if item.artwork}<img
                        src={item.artwork}
                        alt=""
                        referrerpolicy="no-referrer"
                        loading="lazy"
                      />{:else}<Music2 size={18} />{/if}</span
                  >
                  <div class="track-description">
                    <strong>{item.title}</strong><small
                      >{item.artist || item.subtitle}</small
                    >
                  </div>
                  <Play class="queue-row-play" size={14} fill="currentColor" />
                </button>
              {:else}
                <div class="listening-queue-empty">
                  <ListMusic size={25} strokeWidth={1.3} />
                  <p>A little room for<br />what comes next.</p>
                </div>
              {/each}
            </section>
          </aside>
        </div>
        {#if tab === 'remote' || tab === 'group'}
          <button class="primary" onclick={() => (devicePickerOpen = true)}
            ><MonitorSmartphone size={18} />Choose a playback device</button
          >
          <Sharing mode={tab === 'remote' ? 'personal' : 'group'} />
        {:else if tab === 'discord'}
          <div class="page-title">
            <div>
              <div class="eyebrow">LET YOUR MUSIC SAY HELLO</div>
              <h1>Discord presence<span class="mint">.</span></h1>
              <p>A little window into what’s on repeat.</p>
            </div>
            <span class="pill"
              ><span
                class="status-dot"
                class:dim={data.discord === 'Not configured'}
              ></span>{data.discord}</span
            >
          </div>
          <div class="two-column">
            <section class="card">
              <div class="section-heading">
                <h3>Share your listening</h3>
                <Toggle
                  bind:checked={draft.discord_enabled}
                  label="Enable Discord presence"
                />
              </div>
              <p class="muted">
                Show the song, artist, album art, and progress on your Discord
                profile.
              </p>
              <label class="field"
                >Discord application ID<input
                  bind:value={draft.discord_app_id}
                  placeholder="Your application's numeric ID"
                  inputmode="numeric"
                /><small
                  >Create an application in the Discord Developer Portal, then
                  paste its ID here. No bot token is needed.</small
                ></label
              >
              <div class="info-line">
                <ShieldCheck size={17} /><span
                  >Your application ID is public. Your music account stays in
                  the player.</span
                >
              </div>
            </section>
            <section class="presence-preview">
              <div class="eyebrow">A LOOK AT YOUR PROFILE</div>
              <div class="discord-preview-card">
                <div class="profile-row">
                  <div class="profile-avatar">
                    <Headphones size={23} /><span></span>
                  </div>
                  <div>
                    <strong>{draft.display_name}</strong>
                    <p>Listening to YouTube Music</p>
                  </div>
                </div>
                <div class="preview-song">
                  {@render artwork()}
                  <div>
                    <strong>{track?.title ?? 'Your next favorite song'}</strong>
                    <p>{track?.artist ?? 'An artist you love'}</p>
                    <small>{track?.album ?? 'The soundtrack to your day'}</small
                    >
                  </div>
                </div>
                <div class="fake-progress">
                  <span
                    style:width={`${track && data.player.duration ? (position / data.player.duration) * 100 : 38}%`}
                  ></span>
                </div>
                <div class="preview-times">
                  <span>{time(position)}</span><span
                    >{time(data.player.duration)}</span
                  >
                </div>
                <div class="preview-button">Open in YouTube Music</div>
              </div>
              <small>Preview · Discord controls the final appearance</small>
            </section>
          </div>
          <section class="card">
            <h3>A little privacy, on your terms.</h3>
            <div class="setting-row">
              <div>
                <strong>Hide while paused</strong>
                <p>Clear your presence when the music stops.</p>
              </div>
              <Toggle
                bind:checked={draft.hide_paused}
                label="Hide presence when paused"
              />
            </div>
            <div class="setting-row">
              <div>
                <strong>Incognito listening</strong>
                <p>
                  Keep this session to yourself. Your listening room stays
                  connected.
                </p>
              </div>
              <Toggle
                bind:checked={draft.incognito}
                label="Incognito listening"
              />
            </div>
            <div class="setting-row">
              <div>
                <strong>Hide after inactivity</strong>
                <p>Minutes without playback. Use 0 to disable.</p>
              </div>
              <input
                class="number-input"
                type="number"
                min="0"
                max="1440"
                bind:value={draft.idle_minutes}
                aria-label="Idle minutes"
              />
            </div>
            <div class="input-grid">
              <label class="field"
                >Artists to keep private<textarea
                  rows="3"
                  value={draft.blocked_artists.join('\n')}
                  oninput={(e) =>
                    (draft.blocked_artists = listText(e.currentTarget.value))}
                  placeholder="One artist per line"></textarea></label
              ><label class="field"
                >Playlists to keep private<textarea
                  rows="3"
                  value={draft.blocked_playlists.join('\n')}
                  oninput={(e) =>
                    (draft.blocked_playlists = listText(e.currentTarget.value))}
                  placeholder="One YouTube playlist ID per line"
                ></textarea></label
              >
            </div>
          </section>
        {:else if tab === 'party'}
          <div class="page-title">
            <div>
              <div class="eyebrow">DIFFERENT PLACES. SAME FREQUENCY.</div>
              <h1>Listen together<span class="mint">.</span></h1>
              <p>Send an invite. Press play. Share the moment.</p>
            </div>
            <span class="pill"><Users size={14} />Up to 20 listeners</span>
          </div>
          {#if data.party.code}
            <section class="room-card">
              <div class="room-header">
                <span class="pill"
                  ><span class="status-dot" class:dim={!data.party.connected}
                  ></span>{data.party.connected
                    ? 'ROOM IS LIVE'
                    : data.party.reconnecting
                      ? 'RECONNECTING'
                      : 'ROOM CLOSED'}</span
                ><button
                  class="secondary small"
                  onclick={() => action(() => call('party_leave'))}
                  ><X size={15} />Leave room</button
                >
              </div>
              <span class="eyebrow">YOUR ROOM CODE</span>
              <h2 class="room-code">{data.party.code}</h2>
              <p class="muted">{data.party.message}</p>
              <div class="button-row">
                <button
                  class="primary"
                  onclick={() =>
                    action(async () => {
                      await navigator.clipboard.writeText(
                        data.party.invite_url,
                      );
                      notify('Invite link copied');
                    })}><Copy size={16} />Copy invite link</button
                ><span class="muted small-text"
                  >{data.party.role === 'host'
                    ? 'You set the soundtrack.'
                    : 'Playback follows your host.'}</span
                >
              </div>
            </section>
            <section class="card">
              <div class="section-heading">
                <h3>
                  In the room <span class="count"
                    >{data.party.members.length}</span
                  >
                </h3>
                <span class="muted small-text"
                  >{Math.round(Math.abs(data.party.drift_ms))} ms drift</span
                >
              </div>
              {#each data.party.members as member}<div class="member-row">
                  <span class="member-avatar"
                    >{member.name.slice(0, 1).toUpperCase()}</span
                  >
                  <div>
                    <strong
                      >{member.name}
                      {#if member.role === 'host'}<Crown
                          size={13}
                          class="mint"
                        />{/if}</strong
                    >
                    <p>
                      {member.role === 'host'
                        ? 'Host'
                        : member.state === 'synced'
                          ? 'In sync'
                          : member.state}
                    </p>
                  </div>
                  <span class="status-dot" class:dim={member.state !== 'synced'}
                  ></span>{#if data.party.role === 'host' && member.role !== 'host'}<button
                      class="text-button small"
                      onclick={() =>
                        action(() =>
                          call('party_transfer', { memberId: member.id }),
                        )}>Make host</button
                    >{/if}
                </div>{/each}
            </section>
          {:else}
            <section class="party-hero">
              <div class="party-orbits" aria-hidden="true">
                <div class="party-ring"></div>
                <span class="party-person a"><Headphones size={26} /></span
                ><span class="party-person b"><Music2 size={23} /></span><span
                  class="party-person c"><AudioLines size={23} /></span
                ><span class="party-heart"
                  ><Heart size={19} fill="currentColor" /></span
                >
              </div>
              <h2>Distance is just a detail.</h2>
              <p>
                Your friends stream from their own accounts.<br />We keep the
                song and the moment in sync.
              </p>
            </section>
            <div class="two-column">
              <section class="card">
                <span class="feature-icon green"><Radio size={22} /></span>
                <h3>Be the one with the good taste.</h3>
                <p class="muted">
                  Host a room and share the invite with your friends.
                </p>
                <button
                  class="primary full"
                  disabled={busy || !draft.relay_url}
                  onclick={() => party(true)}
                  ><Plus size={17} />Start a listening room</button
                >
              </section>
              <section class="card">
                <span class="feature-icon purple"><Headphones size={22} /></span
                >
                <h3>Got an invite?</h3>
                <p class="muted">Enter a room code or paste the invite link.</p>
                <div class="join-input">
                  <input
                    aria-label="Room code or invite link"
                    bind:value={room}
                    placeholder="e.g. HARMNY23"
                    onkeydown={(e) => {
                      if (e.key === 'Enter') void party(false);
                    }}
                  /><button
                    class="primary"
                    disabled={busy || !draft.relay_url || !room.trim()}
                    onclick={() => party(false)}
                    >Join <ArrowRight size={15} /></button
                  >
                </div>
              </section>
            </div>
          {/if}
          <section class="card">
            <div class="section-heading">
              <h3>Your connection</h3>
              <span class="pill subtle">SELF-HOSTED RELAY</span>
            </div>
            <div class="input-grid">
              <label class="field"
                >Display name<input
                  bind:value={draft.display_name}
                  maxlength="32"
                  placeholder="What should we call you?"
                /></label
              ><label class="field"
                >Relay URL<input
                  bind:value={draft.relay_url}
                  placeholder="https://your-relay.workers.dev"
                /></label
              >
            </div>
            <p class="muted small-text">
              {draft.relay_url
                ? 'Use the same relay as your friends. Changes are saved before connecting.'
                : 'Add your Cloudflare relay URL to start or join a room. Deployment instructions are in the README.'}
            </p>
            <div class="info-line">
              <ShieldCheck size={16} /><span
                >Only playback timing and room membership are shared. Your
                account and audio stay on your device.</span
              >
            </div>
          </section>
        {:else if tab === 'hotkeys'}
          <div class="page-title">
            <div>
              <div class="eyebrow">KEEP YOUR HANDS IN THE FLOW</div>
              <h1>Less clicking. More listening<span class="mint">.</span></h1>
              <p>
                Your music is always a few keys away, even when the app is
                tucked away.
              </p>
            </div>
            <span class="feature-icon green"><Keyboard size={25} /></span>
          </div>
          <div class="button-row quick-search-launch">
            <button
              class="secondary"
              onclick={() =>
                action(() => call('open_panel', { panel: 'search' }))}
              ><Search size={16} />Try quick search</button
            >
          </div>
          <section class="card shortcuts-card">
            {#each shortcuts as shortcut}<div class="setting-row">
                <div>
                  <strong>{shortcut.title}</strong>
                  <p>{shortcut.description}</p>
                </div>
                <input
                  aria-label={shortcut.title + ' shortcut'}
                  class="shortcut-input"
                  bind:value={draft[shortcut.key]}
                  placeholder="Not assigned"
                />
              </div>{/each}
            <div class="info-line">
              <Keyboard size={17} /><span
                >Use combinations like Ctrl+Shift+Space or Alt+M. Leave a field
                empty to disable it. Each combination must be unique.</span
              >
            </div>
          </section>
          <div class="tip">
            <Headphones size={18} /><span
              >Your keyboard’s media keys work through the system media
              controls, too.</span
            >
          </div>
        {:else if tab === 'general'}
          <div class="page-title">
            <div>
              <div class="eyebrow">THE LITTLE THINGS</div>
              <h1>Make yourself at home<span class="mint">.</span></h1>
              <p>A companion that fits the way you listen.</p>
            </div>
          </div>
          <div class="preferences-grid">
            {#if panel === 'settings' && !mobile}
              <section class="card appearance-card">
                <div class="section-heading">
                  <h3>Appearance</h3>
                  <span class="panel-kicker">YOUR SPACE</span>
                </div>
                <div class="appearance-preview" aria-hidden="true">
                  <div class="preview-window">
                    <div class="preview-window-nav"><i></i><i></i><i></i></div>
                    <div class="preview-window-body">
                      <span></span><span></span><span></span>
                    </div>
                    <div class="preview-window-player">
                      <span></span><Play size={12} fill="currentColor" />
                    </div>
                  </div>
                  <div class="appearance-swatch"></div>
                  <span>Rose after dark</span>
                </div>
                <div class="setting-row">
                  <div>
                    <strong>Window blur</strong>
                    <p>
                      A translucent background with a soft blur of your desktop.
                    </p>
                  </div>
                  <Toggle
                    bind:checked={
                      () => data.settings.window_blur,
                      (value) => void setWindowBlur(value)
                    }
                    disabled={appearanceBusy || busy}
                    label="Window blur"
                  />
                </div>
                <p class="muted small-text">
                  Applies immediately and is saved automatically.
                </p>
              </section>
            {/if}
            <section class="card">
              <h3>Streaming quality</h3>
              <p class="muted">
                Song versions and audio-only playback are preferred, including
                tracks paired with a music video. Video-only recordings remain
                available when no matching song version is found.
              </p>
              <div class="setting-row">
                <div>
                  <strong>High</strong>
                  <p>
                    Request YouTube Music’s highest audio quality available to
                    your account.
                  </p>
                </div>
                <span class="pill"
                  >{data.audio.requested_high
                    ? 'Requested'
                    : 'Waiting for player'}</span
                >
              </div>
              <p class="muted">
                {data.audio.premium
                  ? 'Premium account detected.'
                  : 'Premium quality requires a signed-in YouTube Music Premium account.'}
                {#if data.player.ad}Quality information resumes after the
                  advertisement.
                {:else if data.audio.bitrate}Current stream: {Math.round(
                    data.audio.bitrate / 1000,
                  )} kbps{data.audio.stream_quality === 'high'
                    ? ' · High quality'
                    : ''}.
                {:else}Current stream quality will appear when a song starts.{/if}
              </p>
            </section>
            <section class="card desktop-preferences" hidden={mobile}>
              <h3>Your desktop, your rhythm.</h3>
              <div class="setting-row">
                <div>
                  <strong>Keep listening when the window closes</strong>
                  <p>Stay in the system tray. Use the tray menu to quit.</p>
                </div>
                <Toggle
                  bind:checked={draft.close_to_tray}
                  label="Close to system tray"
                />
              </div>
              <div class="setting-row">
                <div>
                  <strong>A soundtrack from the start</strong>
                  <p>Launch YTM Listen when you sign in to your computer.</p>
                </div>
                <Toggle
                  bind:checked={draft.autostart}
                  label="Launch at startup"
                />
              </div>
            </section>
            <section class="card" hidden={mobile}>
              <div class="section-heading">
                <h3>A smaller way to stay close.</h3>
                <span class="feature-icon green"><Disc3 size={22} /></span>
              </div>
              <p class="muted">
                The always-on-top mini-player puts the essentials within reach,
                without taking over your space.
              </p>
              <button
                class="secondary"
                onclick={() =>
                  action(() => call('open_panel', { panel: 'mini' }))}
                >Open mini-player <ArrowUpRight size={16} /></button
              >
            </section>
            <section class="card">
              <h3>Find the words.</h3>
              <p class="muted">
                Look up lyrics for your current song. Synced lyrics highlight as
                you listen.
              </p>
              <button
                class="secondary"
                onclick={() =>
                  action(() => call('open_panel', { panel: 'lyrics' }))}
                >Open lyrics <MicVocal size={16} /></button
              >
            </section>
          </div>
        {:else if tab === 'about'}
          <div class="page-title">
            <div>
              <div class="eyebrow">MADE FOR THE WAY YOU LISTEN</div>
              <h1>A little closer to the music<span class="mint">.</span></h1>
              <p>YTM Listen · Version {appVersion}</p>
            </div>
            <span class="brand-mark big"><AudioLines size={34} /></span>
          </div>
          <section class="card">
            <h3>An open source desktop companion.</h3>
            <p class="muted">
              Search, library browsing, a play queue, and playback in one music
              interface, with Discord presence and rooms for listening together.
              Built with Tauri, Rust, and Svelte. Free to use and fork under the
              MIT license.
            </p>
            <div class="info-line">
              <ShieldCheck size={20} /><span
                >No account passwords pass through YTM Listen. Sign-in happens
                directly on Google's pages in the account window.</span
              >
            </div>
            <p class="muted small-text">
              Independent project. Not affiliated with or endorsed by YouTube,
              Google, or Discord. Playback availability and sign-in support
              depend on YouTube and your system WebView. Each listener needs
              their own account and access to the content.
            </p>
          </section>
          <section class="card">
            <h3>Getting settled in</h3>
            <ol class="onboarding">
              <li>Search for music and play it inside YTM Listen.</li>
              <li>Add your Discord application ID to share your listening.</li>
              <li>
                Deploy the included relay and set its URL to listen with
                friends.
              </li>
            </ol>
          </section>
        {/if}
        <footer class="content-footer">
          <span
            ><AudioLines size={13} />Good music. A little more connected.</span
          ><span>Made to stay out of your way.</span>
        </footer>
      </main>
      {#if dirty}<div class="save-bar">
          <span>You have unsaved changes.</span><button
            class="text-button"
            disabled={busy || appearanceBusy}
            onclick={() =>
              (draft = structuredClone($state.snapshot(data.settings)))}
            >Discard</button
          ><button
            class="primary small"
            disabled={busy || appearanceBusy}
            onclick={() => save()}
            >{#if busy}<LoaderCircle class="spin" size={16} />{:else}<Check
                size={16}
              />{/if}Save preferences</button
          >
        </div>{/if}
      <div class="now-playing" class:remote-playing={device.active}>
        <div class="now-track">
          {@render artwork()}
          <div class="truncate">
            <strong>{track?.title ?? 'A good song starts here.'}</strong>
            <p>
              {data.player.ad
                ? 'Advertisement - your music resumes shortly'
                : (track?.artist ?? 'Search for your next favorite above.')}
            </p>
          </div>
          {#if track && !device.active}<button
              class="icon"
              aria-label="Like current song"
              onclick={() => command({ type: 'like' })}
              ><Heart size={17} /></button
            >{/if}
        </div>
        <div class="now-controls">
          <button
            class="device-picker-button"
            class:connected={device.active}
            aria-label="Choose playback device"
            disabled={!native}
            onclick={() => (devicePickerOpen = true)}
            ><MonitorSmartphone size={16} /><span
              >{device.active
                ? device.target
                : mobile
                  ? 'This phone'
                  : 'This computer'}</span
            ></button
          >
          {@render transport()}{@render progress()}
        </div>
        <div class="now-extras">
          {#if draft.incognito}<EyeOff size={17} />{/if}{#if !mobile}<button
              class="icon"
              aria-label="Open mini-player"
              onclick={() =>
                action(() => call('open_panel', { panel: 'mini' }))}
              ><Disc3 size={18} /></button
            >{/if}{#if !device.active}<button
              class="icon"
              aria-label={data.player.muted ? 'Unmute' : 'Mute'}
              aria-pressed={data.player.muted}
              onclick={() =>
                command({ type: 'mute', muted: !data.player.muted })}
              >{#if data.player.muted || data.player.volume === 0}<VolumeX
                  size={17}
                />{:else}<Volume2 size={17} />{/if}</button
            >{/if}<input
            aria-label="Volume"
            disabled={!allowed(device, 'volume')}
            type="range"
            min="0"
            max="1"
            step="0.01"
            value={data.player.volume}
            onchange={(e) =>
              command({
                type: 'volume',
                volume: Number(e.currentTarget.value),
              })}
          />
        </div>
      </div>
    </div>
  </div>
{/if}
{#if error || data.playback_error}<div class="error-toast" role="alert">
    <span>{error || data.playback_error}</span>{#if data.playback_error}<button
        class="text-button"
        onclick={() => action(() => call('retry_playback'))}
        >Retry playback</button
      >{/if}<button
      class="icon"
      aria-label="Dismiss error"
      onclick={() => {
        error = '';
        if (data.playback_error)
          void action(() => call('dismiss_playback_error'));
      }}><X size={16} /></button
    >
  </div>{/if}
{#if toast}<div class="toast" role="status">
    <CheckCheck size={17} />{toast}
  </div>{/if}

{#if accountOpen}
  <div class="modal-backdrop" role="presentation">
    <div
      class="account-modal"
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-labelledby="account-title"
    >
      <div class="section-heading">
        <span class="feature-icon green"><UserRound size={24} /></span><button
          class="icon"
          aria-label="Close account panel"
          onclick={() => (accountOpen = false)}><X size={18} /></button
        >
      </div>
      <div class="eyebrow">YOUR MUSIC, YOUR ACCOUNT</div>
      <h2 id="account-title">
        {data.account.signed_in
          ? data.account.name || 'Your YouTube account'
          : mobile
            ? 'Connect your Google library.'
            : 'Bring your library along.'}
      </h2>
      <p>
        {#if mobile}
          Connect Google to browse your private YouTube playlists. Playback uses
          a separate session; connecting your library does not enable signed-in
          Music Premium playback.
        {:else}
          {data.account.signed_in
            ? 'Your music and playlists use this YouTube Music account.'
            : 'Sign in to access the music and playlists saved to your YouTube account.'}
          Search and public playback are available without signing in, subject to
          YouTube availability.
        {/if}
      </p>
      {#if data.account_message}<div class="info-line">
          {data.account_message}
        </div>{/if}
      <div class="button-row">
        {#if data.account.signed_in}<button class="primary" onclick={logout}
            >{mobile ? 'Disconnect Google' : 'Sign out'}</button
          >{:else}<button
            class="primary"
            onclick={() => action(() => call('open_account'))}
            ><UserRound size={16} />{mobile
              ? 'Connect Google library'
              : 'Sign in with Google'}</button
          >{/if}<button
          class="secondary"
          onclick={() =>
            action(async () => {
              await call('refresh_music');
              notify(
                'Music connection refreshed. Retry your library in a moment.',
              );
            })}>Refresh connection</button
        >
      </div>
      <p class="account-help">
        {#if mobile}
          Google’s native account chooser handles authorization. Google Play
          services is required. You can disconnect and revoke library access
          here at any time.
        {:else}
          Google sign-in opens in a separate account window. No credentials are
          entered into this application interface. If Google rejects the
          embedded browser, that remains a compatibility blocker; repeatedly
          signing in will not resolve it.
        {/if}
      </p>
    </div>
  </div>
{/if}

{#if devicePickerOpen}
  <div class="modal-backdrop" role="presentation">
    <div
      class="device-dialog card"
      role="dialog"
      aria-modal="true"
      aria-labelledby="device-title"
      tabindex="-1"
    >
      <button
        class="icon device-close"
        aria-label="Close device picker"
        onclick={() => (devicePickerOpen = false)}><X size={20} /></button
      >
      <h2 id="device-title">Play on a device</h2>
      <p>Your player follows the selected device. Audio plays there.</p>
      <button
        class="device-local secondary"
        aria-pressed={!device.active}
        onclick={() =>
          action(async () => {
            if (device.active) await call('remote_disconnect');
            acceptDevice({ ...noDevice });
            devicePickerOpen = false;
          })}
      >
        <MonitorSmartphone size={20} />{mobile ? 'This phone' : 'This computer'}
        {device.active ? '· Disconnect from host' : '· Selected'}
      </button>
      <DeviceConnection
        onChange={acceptDevice}
        onConnected={() => (devicePickerOpen = false)}
      />
    </div>
  </div>
{/if}
