<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
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
  import {
    call,
    defaults,
    initial,
    native,
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
    | 'hotkeys'
    | 'general'
    | 'about';
  let tab = $state<Tab>(panel === 'party' ? 'party' : 'overview');
  let musicView = $state<'home' | 'search' | 'library' | 'queue'>('home');
  $effect(() => {
    if (
      tab === 'overview' ||
      tab === 'search' ||
      tab === 'library' ||
      tab === 'queue'
    )
      musicView = tab === 'overview' ? 'home' : tab;
  });
  let data = $state<Snapshot>(structuredClone(initial));
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
    { id: 'discord', label: 'Discord presence', icon: MessageCircle },
    { id: 'party', label: 'Listen together', icon: Users },
    { id: 'hotkeys', label: 'Keyboard shortcuts', icon: Keyboard },
    { id: 'general', label: 'Preferences', icon: SlidersHorizontal },
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
    const unchanged = JSON.stringify(draft) === JSON.stringify(data.settings);
    data = next;
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
    return () => {
      disposed = true;
      dispose();
      clearInterval(timer);
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
      data = { ...data, settings: structuredClone($state.snapshot(draft)) };
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
      data = { ...data, settings: { ...data.settings, window_blur: enabled } };
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
  function logout() {
    void action(async () => {
      await call('logout_account');
      accountOpen = false;
      notify('Signing out of YouTube Music…');
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
      disabled={!track || locked || data.player.ad}
      onclick={() => command({ type: 'previous' })}
      ><SkipBack size={18} fill="currentColor" /></button
    >
    <button
      class="play-button"
      aria-label={data.player.playing ? 'Pause' : 'Play'}
      disabled={!track || locked}
      onclick={() => command({ type: 'toggle' })}
      >{#if data.player.playing}<Pause
          size={19}
          fill="currentColor"
        />{:else}<Play size={19} fill="currentColor" />{/if}</button
    >
    <button
      class="icon"
      aria-label="Next track"
      disabled={!track || locked || data.player.ad}
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
    {locked}
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
  <div class="app-shell">
    <aside class="sidebar">
      <a href="#overview" class="brand" onclick={() => (tab = 'overview')}
        ><span class="brand-mark"><AudioLines size={25} strokeWidth={2} /></span
        ><span
          >ytm<span class="brand-light">listen</span><span class="brand-dot"
            >.</span
          ></span
        ></a
      >
      <div class="workspace-label">YOUR MUSIC, AT HOME</div>
      <nav aria-label="Main navigation">
        {#each nav as item}<button
            class:active={tab === item.id}
            onclick={() => (tab = item.id)}
            ><item.icon
              size={18}
              strokeWidth={1.7}
            />{item.label}{#if item.id === 'party'}<span class="nav-new"
                >NEW</span
              >{/if}</button
          >{/each}
      </nav>
      <div class="sidebar-bottom">
        <div class="quiet-card">
          <span class="quiet-icon"><Headphones size={21} /></span><strong
            >A space for your music.</strong
          >
          <p>No extra noise.<br />Just a better way to listen.</p>
        </div>
        <button class="about-link" onclick={() => (tab = 'about')}
          ><CircleHelp size={16} /> About YTM Listen <span>0.3.2</span></button
        >
        <div class="build-label">
          <span class="status-dot"></span>{native
            ? 'DESKTOP COMPANION'
            : 'INTERFACE PREVIEW'}
        </div>
      </div>
    </aside>
    <div class="main-column">
      <header class="topbar">
        <div class="breadcrumb">
          <span
            >{tab === 'general'
              ? 'Preferences'
              : tab === 'about'
                ? 'About'
                : nav.find((n) => n.id === tab)?.label}</span
          >
        </div>
        <AccountMenu
          account={data.account}
          onAccount={() => (accountOpen = true)}
          onLogout={logout}
        />
      </header>
      <main class="content">
        {#if !native}<div class="preview-note">
            <Disc3 size={14} /> Interface preview. Playback and integrations run in
            the desktop app.
          </div>{/if}
        <div
          hidden={!(
            tab === 'overview' ||
            tab === 'search' ||
            tab === 'library' ||
            tab === 'queue'
          )}
        >
          <Music
            {data}
            view={musicView}
            active={tab === 'overview' ||
              tab === 'search' ||
              tab === 'library' ||
              tab === 'queue'}
            onError={(message) => (error = message)}
            onAccount={() => (accountOpen = true)}
          />
        </div>
        {#if tab === 'discord'}
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
          {#if panel === 'settings'}
            <section class="card appearance-card">
              <h3>Appearance</h3>
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
          <section class="card">
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
          <section class="card">
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
        {:else if tab === 'about'}
          <div class="page-title">
            <div>
              <div class="eyebrow">MADE FOR THE WAY YOU LISTEN</div>
              <h1>A little closer to the music<span class="mint">.</span></h1>
              <p>YTM Listen · Version 0.3.2</p>
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
      <div class="now-playing">
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
          {#if track}<button
              class="icon"
              aria-label="Like current song"
              onclick={() => command({ type: 'like' })}
              ><Heart size={17} /></button
            >{/if}
        </div>
        <div class="now-controls">
          {@render transport()}{@render progress()}
        </div>
        <div class="now-extras">
          {#if draft.incognito}<EyeOff size={17} />{/if}<button
            class="icon"
            aria-label="Open mini-player"
            onclick={() => action(() => call('open_panel', { panel: 'mini' }))}
            ><Disc3 size={18} /></button
          ><button
            class="icon"
            aria-label={data.player.muted ? 'Unmute' : 'Mute'}
            aria-pressed={data.player.muted}
            onclick={() => command({ type: 'mute', muted: !data.player.muted })}
            >{#if data.player.muted || data.player.volume === 0}<VolumeX
                size={17}
              />{:else}<Volume2 size={17} />{/if}</button
          ><input
            aria-label="Volume"
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
          : 'Bring your library along.'}
      </h2>
      <p>
        {data.account.signed_in
          ? 'Your music and playlists use this YouTube Music account.'
          : 'Sign in to access the music and playlists saved to your YouTube account.'}
        Search and public playback are available without signing in, subject to YouTube
        availability.
      </p>
      {#if data.account_message}<div class="info-line">
          {data.account_message}
        </div>{/if}
      <div class="button-row">
        {#if data.account.signed_in}<button class="primary" onclick={logout}
            >Sign out</button
          >{:else}<button
            class="primary"
            onclick={() => action(() => call('open_account'))}
            ><UserRound size={16} />Sign in with Google</button
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
        Google sign-in opens in a separate account window. No credentials are
        entered into this application interface. If Google rejects the embedded
        browser, that remains a compatibility blocker; repeatedly signing in
        will not resolve it.
      </p>
    </div>
  </div>
{/if}
