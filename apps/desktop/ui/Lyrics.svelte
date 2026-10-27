<script lang="ts">
  import { untrack } from 'svelte';
  import { LoaderCircle, MicVocal } from '@lucide/svelte';
  import { call, type Snapshot } from './api';
  import { parseLrc, type Line } from './lyrics';
  import Toggle from './Toggle.svelte';

  let { data, position }: { data: Snapshot; position: number } = $props();
  type LyricsResult = {
    syncedLyrics: string | null;
    plainLyrics: string | null;
    instrumental: boolean;
  };
  const track = $derived(data.player.track);
  const hasTrack = $derived(Boolean(track?.video_id) && !data.player.ad);
  const key = $derived(
    JSON.stringify([
      data.remote?.active ? data.remote.snapshot?.session_id : 'local',
      data.account.signed_in,
      data.account.name,
      track?.video_id,
      track?.title,
      track?.artist,
      Math.round(data.player.duration),
    ]),
  );
  let automatic = $state(false);
  let lines = $state<Line[]>([]);
  let plain = $state('');
  let status = $state('Play a song, then look up its lyrics.');
  let loading = $state(false);
  let loaded = $state(false);
  let generation = 0;
  // Lyrics stay in this window's memory, never in the saved account profile.
  const cache = new Map<string, LyricsResult>();
  const activeLine = $derived(
    lines.reduce((last, line, i) => (line.time <= position ? i : last), -1),
  );

  function display(result: LyricsResult) {
    lines = parseLrc(result.syncedLyrics ?? '');
    plain = result.plainLyrics ?? '';
    loaded = true;
    status = result.instrumental
      ? 'Just the music. This track is instrumental.'
      : lines.length || plain
        ? ''
        : 'No lyrics found for this song.';
  }

  async function lookup(refresh = false) {
    if (!hasTrack || loading) return;
    const requestedKey = key;
    const request = ++generation;
    const saved = !refresh && cache.get(requestedKey);
    if (saved) {
      display(saved);
      return;
    }
    loading = true;
    status = 'Finding the words…';
    try {
      const result = await call<LyricsResult>('get_lyrics');
      if (request !== generation || requestedKey !== key) return;
      cache.delete(requestedKey);
      cache.set(requestedKey, result);
      if (cache.size > 20) cache.delete(cache.keys().next().value!);
      display(result);
    } catch (error) {
      if (request === generation && requestedKey === key)
        status = String(error).replace(/^Error: /, '');
    } finally {
      if (request === generation) loading = false;
    }
  }

  $effect(() => {
    key;
    const enabled = automatic;
    const available = hasTrack;
    untrack(() => {
      generation++;
      loading = false;
      lines = [];
      plain = '';
      loaded = false;
      status = available
        ? 'Look up lyrics for the current song.'
        : 'Play a song, then look up its lyrics.';
      if (available) {
        const saved = cache.get(key);
        if (saved) display(saved);
        else if (enabled) void lookup();
      }
    });
    return () => {
      generation++;
    };
  });
</script>

<main class="lyrics-panel">
  <div class="eyebrow"><MicVocal size={15} /> THE WORDS BEHIND THE MUSIC</div>
  <h1>{track?.title ?? 'Every song has a story.'}</h1>
  <p class="muted">{track?.artist ?? 'Choose something you love.'}</p>
  <div class="lyrics-actions">
    <button
      class="secondary"
      disabled={!hasTrack || loading}
      onclick={() => void lookup(true)}
    >
      {#if loading}<LoaderCircle class="spin" size={16} />{:else}<MicVocal
          size={16}
        />{/if}
      {loaded ? 'Refresh lyrics' : 'Find lyrics'}
    </button>
    <label class="automatic-lyrics"
      ><Toggle
        label="Automatically find lyrics"
        bind:checked={automatic}
      />Automatically find lyrics</label
    >
  </div>
  {#if status}<p class="lyrics-status" role="status">{status}</p>{/if}
  <div class="lyrics-lines" aria-busy={loading}>
    {#each lines as line, i}<p class:current={i === activeLine}>
        {line.text || '♪'}
      </p>{/each}
    {#if !lines.length}<p class="plain-lyrics">{plain}</p>{/if}
  </div>
  <small class="muted"
    >Lyrics provided by LRCLIB · {automatic
      ? 'Automatic lookup while this window is open'
      : 'Fetched only when you ask'}</small
  >
</main>

<style>
  .lyrics-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 16px;
    margin-top: 22px;
  }
  .automatic-lyrics {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--muted);
    font-size: 13px;
  }
</style>
