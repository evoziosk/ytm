<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { Search, LoaderCircle, Play, AudioLines, X } from '@lucide/svelte';
  import {
    call,
    initial,
    native,
    snapshot,
    subscribe,
    type Snapshot,
  } from './api';
  import type { CatalogPage, MusicItem } from '@ytm/protocol';

  let data = $state<Snapshot>(structuredClone(initial));
  let query = $state('');
  let results = $state<MusicItem[]>([]);
  let selected = $state(0);
  let searching = $state(false);
  let playing = $state(false);
  let failure = $state('');
  let input = $state<HTMLInputElement>();
  let list = $state<HTMLDivElement>();
  let timer: ReturnType<typeof setTimeout>;
  let generation = 0;
  let inFlight = false;
  let queued: { query: string; generation: number } | null = null;
  let disposed = false;
  const locked = $derived(data.party.role === 'member');

  async function focus() {
    await tick();
    input?.focus();
    input?.select();
  }
  async function dismiss() {
    if (native) await getCurrentWindow().hide();
  }
  onMount(() => {
    let unsubscribe = () => {};
    let unfocus = () => {};
    void focus();
    if (native)
      void (async () => {
        unsubscribe = await subscribe((next) => {
          data = next;
        });
        if (disposed) {
          unsubscribe();
          return;
        }
        data = await snapshot();
        if (disposed) {
          unsubscribe();
          return;
        }
        unfocus = await getCurrentWindow().onFocusChanged(({ payload }) => {
          if (payload) void focus();
        });
        if (disposed) {
          unsubscribe();
          unfocus();
        }
      })().catch((error) => {
        if (!disposed) failure = String(error);
      });
    return () => {
      disposed = true;
      generation++;
      clearTimeout(timer);
      queued = null;
      unsubscribe();
      unfocus();
    };
  });

  function schedule() {
    clearTimeout(timer);
    const request = { query: query.trim(), generation: ++generation };
    queued = null;
    results = [];
    selected = 0;
    failure = '';
    searching = Boolean(request.query);
    if (!request.query) return;
    timer = setTimeout(() => {
      queued = request;
      void search();
    }, 250);
  }
  async function search() {
    if (inFlight || !queued || disposed) return;
    const request = queued;
    queued = null;
    inFlight = true;
    try {
      const page = await call<CatalogPage>('music_query', {
        request: {
          kind: 'search',
          query: request.query,
          browse_id: '',
          continuation: '',
        },
      });
      if (request.generation === generation && !disposed)
        results = page.items.filter((item) => item.video_id);
    } catch (error) {
      if (request.generation === generation && !disposed)
        failure = String(error).replace(/^Error: /, '');
    } finally {
      inFlight = false;
      if (request.generation === generation && !disposed) searching = false;
      if (queued) void search();
    }
  }
  async function choose(index: number) {
    if (playing || !results[index]) return;
    if (locked) {
      failure =
        'Playback follows your party host. Leave the party to choose a song.';
      return;
    }
    playing = true;
    failure = '';
    const songs = $state.snapshot(results);
    try {
      await call('play_items', { items: songs, index, playlistId: '' });
      await dismiss();
    } catch (error) {
      failure = String(error).replace(/^Error: /, '');
    } finally {
      playing = false;
    }
  }
  function keydown(event: KeyboardEvent) {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey)
      return;
    if (event.key === 'Enter') {
      event.preventDefault();
      if (!searching) void choose(selected);
    }
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    event.preventDefault();
    if (!results.length) return;
    selected =
      (selected + (event.key === 'ArrowDown' ? 1 : -1) + results.length) %
      results.length;
    void tick().then(() =>
      list
        ?.querySelector<HTMLElement>(`[data-index="${selected}"]`)
        ?.scrollIntoView?.({ block: 'nearest' }),
    );
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === 'Escape' && !event.isComposing) {
      event.preventDefault();
      void dismiss();
    }
  }}
/>

<main class="quick-search" aria-label="Quick music search">
  <div class="quick-input-row">
    {#if searching || playing}<LoaderCircle
        size={23}
        class="spin"
      />{:else}<Search size={23} />{/if}
    <input
      bind:this={input}
      bind:value={query}
      oninput={(event) => {
        query = event.currentTarget.value;
        schedule();
      }}
      onkeydown={keydown}
      aria-label="Search for a song"
      role="combobox"
      aria-autocomplete="list"
      aria-controls="quick-results"
      aria-expanded={results.length > 0}
      aria-activedescendant={results[selected]
        ? `quick-result-${selected}`
        : undefined}
      autocomplete="off"
      spellcheck="false"
      maxlength="512"
      placeholder="Search for a song…"
    />
    <button
      class="quick-close"
      aria-label="Close quick search"
      onclick={() => void dismiss()}><X size={17} /></button
    >
  </div>
  {#if failure}<div class="quick-error" role="alert">{failure}</div>{/if}
  {#if results.length}
    <div
      class="quick-results"
      id="quick-results"
      role="listbox"
      aria-label="Music results"
      bind:this={list}
      aria-busy={playing}
    >
      {#each results as item, index (item.key)}
        <button
          class="quick-result"
          class:chosen={selected === index}
          role="option"
          aria-selected={selected === index}
          id={`quick-result-${index}`}
          data-index={index}
          tabindex="-1"
          disabled={playing}
          onpointermove={() => (selected = index)}
          onclick={() => void choose(index)}
        >
          <span class="quick-art"
            >{#if item.artwork}<img
                src={item.artwork}
                alt=""
                loading="lazy"
                referrerpolicy="no-referrer"
              />{:else}<AudioLines size={19} />{/if}</span
          >
          <span class="quick-description"
            ><strong>{item.title}</strong><small
              >{item.artist || item.subtitle}</small
            ></span
          >
          {#if item.kind === 'video'}<span class="quick-video"
              >Video fallback</span
            >{/if}
          <Play size={14} />
        </button>
      {/each}
    </div>
  {:else}
    <div class="quick-empty" role="status">
      <AudioLines size={30} strokeWidth={1.4} />
      <strong
        >{searching
          ? 'Finding your music…'
          : query.trim()
            ? 'No songs found'
            : 'A song away from a better moment.'}</strong
      >
      <p>
        {searching
          ? 'Song recordings come first.'
          : query.trim()
            ? 'Try a song title or artist name.'
            : 'Type a title or artist. Keep your hands on the keyboard.'}
      </p>
    </div>
  {/if}
  <footer>
    <span
      >{playing
        ? 'Starting playback…'
        : locked
          ? 'Following your party host'
          : '↑ ↓ choose · Enter play · Esc close'}</span
    ><span>{data.settings.hotkey_search || 'YTM Listen'}</span>
  </footer>
</main>

<style>
  .quick-search {
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: rgba(18, 27, 22, 0.35);
    backdrop-filter: blur(24px) saturate(125%);
    border: 1px solid #b0e7ce40;
    border-radius: 12px;
  }
  .quick-input-row {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 20px 22px;
    border-bottom: 1px solid #ffffff0d;
    color: #b0e7ce;
  }
  .quick-input-row input {
    flex: 1;
    min-width: 0;
    font-size: 20px;
    color: #eef5ef;
    background: none;
    border: 0;
    outline: 0;
    padding: 0;
  }
  .quick-input-row input::placeholder {
    color: #82978a;
  }
  .quick-close {
    display: grid;
    place-items: center;
    color: #91a196;
    border-radius: 5px;
    padding: 5px;
  }
  .quick-close:hover {
    background: #ffffff0d;
  }
  .quick-results {
    flex: 1;
    overflow-y: auto;
    padding: 7px;
    min-height: 0;
  }
  .quick-result {
    display: flex;
    align-items: center;
    gap: 13px;
    padding: 10px 12px;
    width: 100%;
    text-align: left;
    border-radius: 7px;
    border: 1px solid transparent;
  }
  .quick-result.chosen {
    background: #b0e7ce12;
    border-color: #b0e7ce22;
  }
  .quick-art {
    height: 38px;
    width: 38px;
    flex: 0 0 38px;
    display: grid;
    place-items: center;
    border-radius: 5px;
    background: #284331;
    overflow: hidden;
    color: #b0e7ce;
  }
  .quick-art img {
    height: 100%;
    width: 100%;
    object-fit: cover;
  }
  .quick-description {
    flex: 1;
    min-width: 0;
  }
  .quick-description strong,
  .quick-description small {
    display: block;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .quick-description strong {
    font-size: 13px;
    font-weight: 550;
    color: #e1eee5;
  }
  .quick-description small {
    font-size: 11px;
    color: #8ea496;
    margin-top: 3px;
  }
  .quick-result > :global(svg) {
    color: #b0e7ce;
    opacity: 0;
  }
  .quick-result.chosen > :global(svg) {
    opacity: 1;
  }
  .quick-video {
    color: #9cad9f;
    font-size: 9px;
    max-width: 60px;
  }
  .quick-empty {
    flex: 1;
    display: flex;
    justify-content: center;
    align-items: center;
    flex-direction: column;
    gap: 14px;
    padding: 25px;
    text-align: center;
    color: #8ea496;
  }
  .quick-empty strong {
    color: #dce9e0;
    font-weight: 500;
    font-size: 15px;
  }
  .quick-empty p {
    font-size: 12px;
    max-width: 290px;
  }
  .quick-error {
    padding: 10px 22px;
    background: #c1736b14;
    color: #e9ada4;
    font-size: 12px;
  }
  footer {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    padding: 12px 20px;
    border-top: 1px solid #ffffff0d;
    color: #84978a;
    font-size: 10px;
  }
</style>
