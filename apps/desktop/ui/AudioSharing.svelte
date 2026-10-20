<script lang="ts">
  import { onMount } from 'svelte';
  import { Mic, MicOff, AudioLines, Square } from '@lucide/svelte';
  import { call, native } from './api';
  import Toggle from './Toggle.svelte';
  import {
    initialAudio,
    type AudioRoutingStatus,
    type AudioDevices,
  } from './audio-routing';

  let {
    visible = false,
    onOpen = () => {},
  }: { visible?: boolean; onOpen?: () => void } = $props();
  let status = $state(structuredClone(initialAudio));
  let preferences = $state(structuredClone(initialAudio.preferences));
  let devices = $state<AudioDevices>({ outputs: [], microphones: [] });
  let ready = $state(false);
  let busy = $state(false);
  let error = $state('');
  let interrupted = $state(false);
  let revision = 0;
  const microphoneLive = $derived(
    status.active && status.microphone_enabled && !status.microphone_muted,
  );

  async function refreshDevices() {
    try {
      devices = await call<AudioDevices>('audio_devices');
    } catch (e) {
      error = String(e);
    }
  }
  async function openDriverHelp(event: MouseEvent) {
    if (!native) return;
    event.preventDefault();
    try {
      await call('plugin:opener|open_url', {
        url: 'https://vb-audio.com/Cable/',
        with: null,
      });
    } catch (e) {
      error = String(e);
    }
  }
  onMount(() => {
    if (!native) {
      status.message = 'Virtual microphone runs in the Windows desktop app.';
      return;
    }
    let disposed = false,
      polling = false;
    async function poll() {
      if (disposed || polling || busy) return;
      polling = true;
      const expected = revision;
      try {
        const next = await call<AudioRoutingStatus>('audio_status');
        if (disposed || expected !== revision) return;
        if (status.active && !next.active) interrupted = true;
        status = next;
        if (!ready) {
          preferences = structuredClone(next.preferences);
          ready = true;
        }
      } catch (e) {
        if (!disposed) error = String(e);
      } finally {
        polling = false;
      }
    }
    void poll().then(() => {
      if (!disposed && status.supported) void refreshDevices();
    });
    const timer = setInterval(poll, 250);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  });
  async function apply(
    start = false,
    microphone = status.microphone_enabled,
    muted = status.microphone_muted,
    confirmMicrophone = false,
  ) {
    if (busy) return;
    busy = true;
    error = '';
    interrupted = false;
    revision++;
    try {
      status = await call<AudioRoutingStatus>('audio_apply', {
        start,
        routing: {
          preferences: $state.snapshot(preferences),
          microphone_enabled: start ? false : microphone,
          microphone_muted: start ? false : muted,
          confirm_microphone: confirmMicrophone,
        },
      });
    } catch (e) {
      error = String(e);
      try {
        status = await call<AudioRoutingStatus>('audio_status');
      } catch {
        /* Retain the last confirmed state. */
      }
    } finally {
      busy = false;
    }
  }
  async function stop() {
    busy = true;
    error = '';
    interrupted = false;
    revision++;
    try {
      status = await call<AudioRoutingStatus>('audio_stop');
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if !visible && status.active}
  <section class="audio-banner" aria-label="Audio sharing active">
    <button class="text-button" onclick={onOpen}
      ><AudioLines size={17} />Sharing music{microphoneLive
        ? ' + microphone'
        : ''}</button
    >
    {#if status.microphone_enabled}<button
        class="secondary"
        disabled={busy}
        onclick={() => apply(false, true, !status.microphone_muted, true)}
        >{#if microphoneLive}<Mic size={15} />Mute mic{:else}<MicOff
            size={15}
          />Unmute mic{/if}</button
      >{/if}
    <button class="secondary" disabled={busy} onclick={stop}
      ><Square size={14} />Stop sharing</button
    >
    {#if error}<span role="alert">{error}</span>{/if}
  </section>
{/if}
{#if !visible && !status.active && interrupted}
  <section class="audio-banner" aria-label="Audio sharing stopped">
    <span role="alert">{status.message}</span>
    <button class="secondary" onclick={onOpen}>Audio settings</button>
  </section>
{/if}
<div hidden={!visible} class="audio-sharing">
  <div class="page-title">
    <div>
      <div class="eyebrow">YOUR MUSIC. YOUR VOICE.</div>
      <h1>Virtual microphone<span class="mint">.</span></h1>
      <p>
        Send this computer’s music and, if enabled, your microphone to another
        app.
      </p>
    </div>
  </div>
  <section class="card">
    <div class="setting-row">
      <div>
        <h3>Share music</h3>
        <p role="status">{status.message}</p>
      </div>
      {#if status.active}<button class="primary" disabled={busy} onclick={stop}
          >Stop sharing</button
        >
      {:else}<button
          class="primary"
          disabled={busy ||
            !ready ||
            !status.supported ||
            !preferences.output_id}
          onclick={() => apply(true)}>Start sharing</button
        >{/if}
    </div>
    {#if error}<p class="failure" role="alert">{error}</p>{/if}
    {#if status.supported}
      <label class="audio-field"
        >Virtual output<select
          aria-label="Virtual output"
          bind:value={preferences.output_id}
          disabled={busy || status.active}
          ><option value="">Choose a virtual cable</option
          >{#each devices.outputs as device}<option value={device.id}
              >{device.name}</option
            >{/each}</select
        ></label
      >
      <button class="text-button" disabled={busy} onclick={refreshDevices}
        >Refresh devices</button
      >
      {#if !devices.outputs.length}<p class="muted">
          No supported virtual cable was found. Install <a
            href="https://vb-audio.com/Cable/"
            onclick={openDriverHelp}
            target="_blank"
            rel="noreferrer">VB-CABLE from VB-Audio</a
          >, restart Windows if requested, then refresh. VB-CABLE is
          donationware.
        </p>{/if}
      <p class="muted">
        Select the cable’s playback input here, then its recording output as the
        input device in Discord, OBS, or your calling app. Windows defaults stay
        unchanged.
      </p>
      <label class="audio-field"
        >Shared music volume · {Math.round(preferences.music_gain * 100)}%<input
          aria-label="Shared music volume"
          type="range"
          min="0"
          max="1"
          step="0.01"
          bind:value={preferences.music_gain}
          disabled={busy}
          onchange={() => status.active && apply()}
        /></label
      >
      <label class="audio-field"
        >Music level<meter
          aria-label="Music level"
          min="0"
          max="1"
          value={status.music_peak}
        ></meter></label
      >
    {/if}
  </section>
  {#if status.supported}
    <section class="card">
      <div class="setting-row">
        <div>
          <h3>Include microphone</h3>
          <p>
            Starts off every time you open the app. Enable it only when you want
            to share your voice.
          </p>
        </div>
        <Toggle
          label="Include microphone"
          bind:checked={
            () => status.microphone_enabled,
            (value) => void apply(false, value, false, true)
          }
          disabled={busy ||
            !status.active ||
            (!status.microphone_enabled && !preferences.microphone_id)}
        />
      </div>
      <label class="audio-field"
        >Microphone<select
          aria-label="Microphone device"
          bind:value={preferences.microphone_id}
          disabled={busy || status.microphone_enabled}
          ><option value="">Choose your microphone</option
          >{#each devices.microphones as device}<option value={device.id}
              >{device.name}</option
            >{/each}</select
        ></label
      >
      <div class="setting-row">
        <strong
          >{microphoneLive ? 'Microphone is live' : 'Microphone is off'}</strong
        ><button
          class="secondary"
          disabled={busy || !status.active || !status.microphone_enabled}
          onclick={() => apply(false, true, !status.microphone_muted, true)}
          >{status.microphone_muted
            ? 'Unmute microphone'
            : 'Mute microphone'}</button
        >
      </div>
      <label class="audio-field"
        >Microphone gain · {Math.round(
          preferences.microphone_gain * 100,
        )}%<input
          aria-label="Microphone gain"
          type="range"
          min="0"
          max="2"
          step="0.01"
          bind:value={preferences.microphone_gain}
          disabled={busy}
          onchange={() => status.active && apply()}
        /></label
      >
      <label class="audio-field"
        >Microphone level<meter
          aria-label="Microphone level"
          min="0"
          max="1"
          value={status.microphone_peak}
        ></meter></label
      >
      <p class="muted">
        Your microphone is not played through your speakers. Headphones prevent
        music from being picked up acoustically by a live microphone.
      </p>
    </section>
    <section class="card">
      <h3>Output quality</h3>
      <label class="audio-field"
        >Combined level<meter
          aria-label="Combined output level"
          min="0"
          max="1"
          value={status.output_peak}
        ></meter></label
      >
      <p role="status">
        {status.limited
          ? 'Peak protection is reducing the level. Lower the music or microphone gain.'
          : '48 kHz stereo PCM mixing with peak protection.'}
      </p>
      <p class="muted">
        Uses the music quality available to your account without another lossy
        encoding step. Player volume and mute can affect shared music. Calling
        apps may compress, downmix, or suppress music; disable their noise
        suppression for music sharing.
      </p>
      {#if status.active}<p class="muted small-text">
          Buffered music: {status.buffered_ms} ms · Buffer interruptions: {status.underruns}
          · Dropped frames: {status.dropped_frames}
        </p>{/if}
    </section>
  {/if}
</div>

<style>
  .audio-banner {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    padding: 12px 16px;
    border: 1px solid #d2a8b655;
    background: #d2a8b612;
    border-radius: 14px;
    margin-bottom: 20px;
  }
  .audio-field {
    display: grid;
    gap: 10px;
    margin: 20px 0;
    font-size: 13px;
    font-weight: 600;
  }
  .audio-field select {
    width: 100%;
    padding: 12px;
    background: var(--surface, #182125);
    color: inherit;
    border: 1px solid #ffffff24;
    border-radius: 10px;
  }
  .audio-field input,
  meter {
    width: 100%;
    accent-color: #d2a8b6;
  }
  meter {
    height: 12px;
  }
  meter::-webkit-meter-bar {
    background: #ffffff12;
    border: 0;
    border-radius: 8px;
  }
  meter::-webkit-meter-optimum-value {
    background: #d2a8b6;
    border-radius: 8px;
  }
  a {
    color: #d2a8b6;
  }
</style>
