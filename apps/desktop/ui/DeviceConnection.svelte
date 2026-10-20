<script lang="ts">
  import { onMount } from 'svelte';
  import { call, native, mobile } from './api';
  import {
    scan,
    cancel,
    Format,
    checkPermissions,
    requestPermissions,
    openAppSettings,
  } from '@tauri-apps/plugin-barcode-scanner';
  import { noDevice, type PlaybackDevice } from './playback-device';
  import { listen } from '@tauri-apps/api/event';
  type NearbyDevice = {
    id: string;
    name: string;
    address: string;
    mode: 'personal' | 'group';
  };
  let nearby = $state<NearbyDevice[]>([]);
  let discovering = $state(false),
    discoveryError = $state('');
  let mounted = true;
  async function discover() {
    if (!native || discovering) return;
    discovering = true;
    discoveryError = '';
    nearby = [];
    try {
      const result = await call<NearbyDevice[]>('remote_discover');
      if (mounted && Array.isArray(result)) nearby = result;
    } catch (e) {
      if (mounted) discoveryError = String(e);
    } finally {
      if (mounted) discovering = false;
    }
  }
  let {
    onChange = (_status: PlaybackDevice) => {},
    onConnected = () => {},
  }: { onChange?: (status: PlaybackDevice) => void; onConnected?: () => void } =
    $props();
  let data = $state<PlaybackDevice>({ ...noDevice });
  let link = $state(''),
    error = $state(''),
    busy = $state(false);
  let scanning = $state(false),
    cameraDenied = $state(false);
  async function scanHost() {
    if (busy || scanning) return;
    error = '';
    cameraDenied = false;
    scanning = true;
    try {
      let permission = await checkPermissions();
      if (permission !== 'granted') permission = await requestPermissions();
      if (permission !== 'granted') {
        cameraDenied = true;
        error =
          'Camera access is required to scan the host QR code. Enable it in app settings, or enter the host address below.';
        return;
      }
      document.documentElement.classList.add('host-qr-scanning');
      const result = await scan({ formats: [Format.QRCode], windowed: true });
      link = result.content.trim();
      await run(() => call('remote_connect', { invitation: link }));
    } catch (e) {
      if (!/cancel/i.test(String(e)))
        error = `Could not scan the host QR code: ${String(e)}`;
    } finally {
      document.documentElement.classList.remove('host-qr-scanning');
      scanning = false;
    }
  }
  async function refresh() {
    if (!native) return;
    data = await call<PlaybackDevice>('remote_status');
    onChange(data);
  }
  onMount(() => {
    mounted = true;
    let disposeNearby = () => {};
    if (native)
      void listen<NearbyDevice[]>('nearby-devices', (event) => {
        if (mounted && Array.isArray(event.payload)) nearby = event.payload;
      })
        .then((dispose) => {
          if (mounted) disposeNearby = dispose;
          else dispose();
        })
        .catch(() => {});
    void discover();
    let active = true,
      polling = false;
    async function poll() {
      if (!active || polling || busy || scanning) return;
      polling = true;
      try {
        await refresh();
      } catch (e) {
        error = String(e);
      } finally {
        polling = false;
      }
    }
    void poll();
    const timer = setInterval(poll, 1000);
    const discoveryTimer = setInterval(() => {
      if (active) void discover();
    }, 20000);
    return () => {
      active = false;
      mounted = false;
      disposeNearby();
      clearInterval(timer);
      clearInterval(discoveryTimer);
      document.documentElement.classList.remove('host-qr-scanning');
      if (scanning) void cancel().catch(() => {});
    };
  });
  async function run(task: () => Promise<unknown>) {
    busy = true;
    error = '';
    try {
      await task();
      await refresh();
      if (data.active) onConnected();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if scanning}
  <div
    class="qr-overlay"
    role="dialog"
    aria-label="Scan host QR code"
    aria-modal="true"
  >
    <p>Point your camera at the QR code on the host.</p>
    <button onclick={() => cancel().catch((e) => (error = String(e)))}
      >Cancel scan</button
    >
  </div>
{/if}

<section class="remote-client">
  <h3>Nearby devices</h3>
  <button disabled={discovering || !native} onclick={discover}
    >{discovering ? 'Looking for devices…' : 'Refresh nearby devices'}</button
  >
  {#if discoveryError}<p role="status">{discoveryError}</p>{/if}
  {#each nearby as host (host.id)}
    <button
      class="nearby-device"
      disabled={busy || scanning || data.snapshot?.session_id === host.id}
      onclick={() =>
        run(async () => {
          if (data.active) await call('remote_disconnect');
          await call('remote_connect', { invitation: host.address });
        })}
    >
      <strong>{host.name}</strong><span
        >{data.snapshot?.session_id === host.id
          ? 'Selected'
          : data.active
            ? 'Disconnect and connect'
            : 'Connect'} · {host.mode === 'personal'
          ? 'Personal remote'
          : 'Group sharing'}</span
      >
    </button>
  {:else}
    <p>
      {discovering
        ? 'Looking for YTM Listen hosts on your local network…'
        : 'No nearby sharing hosts found. Enable sharing on the other device and use the same LAN.'}
    </p>
  {/each}
  <h3>Connect a device</h3>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if !data.active}
    {#if data.devices?.length}
      <h3>Your paired devices</h3>
      {#each data.devices as device (device.address)}
        <button
          disabled={busy || scanning}
          onclick={() =>
            run(() => call('remote_connect', { invitation: device.address }))}
          >{device.name}<span> · Connect</span></button
        >
      {/each}
    {/if}
    <p>
      Scan the host's QR code, enter its LAN address, or paste its invitation.
      Your player and queue will control that device after approval.
    </p>
    {#if mobile && native}<button disabled={busy || scanning} onclick={scanHost}
        >{scanning ? 'Scanning…' : 'Scan host QR code'}</button
      >{/if}
    {#if cameraDenied}<button
        onclick={() => openAppSettings().catch((e) => (error = String(e)))}
        >Open camera settings</button
      >{/if}
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void run(() => call('remote_connect', { invitation: link }));
      }}
    >
      <label
        >Host address or invitation<input
          type="text"
          bind:value={link}
          placeholder="192.168.1.10:9863 or full invitation link"
          autocapitalize="none"
          spellcheck="false"
          required
        /></label
      >
      <button disabled={busy || scanning || !native}>Connect to host</button>
    </form>
  {:else}
    <h3>Target host: {data.target}</h3>
    <p role="status">{data.message}</p>
    <button disabled={busy} onclick={() => run(() => call('remote_disconnect'))}
      >Disconnect from host</button
    >
  {/if}
</section>

<style>
  :global(html.host-qr-scanning),
  :global(html.host-qr-scanning body) {
    background: transparent !important;
  }
  :global(html.host-qr-scanning #app) {
    visibility: hidden;
  }
  .qr-overlay {
    position: fixed;
    inset: 0;
    z-index: 10000;
    visibility: visible;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    align-items: center;
    padding: max(30px, env(safe-area-inset-top)) 20px
      max(30px, env(safe-area-inset-bottom));
    pointer-events: none;
  }
  .qr-overlay p,
  .qr-overlay button {
    background: #111614;
    color: white;
    padding: 16px;
    border-radius: 12px;
  }
  .qr-overlay button {
    pointer-events: auto;
  }
  .remote-client {
    padding: 22px;
    border: 1px solid #3c5145;
    border-radius: 16px;
    margin-bottom: 30px;
    max-width: 800px;
  }
  p {
    color: var(--muted, #a4b2aa);
  }
  p {
    line-height: 1.6;
  }
  input:not([type='range']) {
    width: 100%;
  }
  button,
  input {
    font: inherit;
    border-radius: 10px;
    padding: 10px;
    border: 1px solid #3c5145;
    background: var(--surface, #1c2922);
    color: inherit;
  }
  label {
    display: block;
    line-height: 2;
  }
  form button {
    margin: 12px 0;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
  }
  .nearby-device {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 5px;
    width: 100%;
    margin: 8px 0;
    text-align: left;
  }
  .nearby-device span {
    font-size: 12px;
    color: var(--muted);
  }
  [role='alert'] {
    color: #f9b6a8;
  }
</style>
