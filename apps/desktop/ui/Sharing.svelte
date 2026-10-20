<script lang="ts">
  import { onMount } from 'svelte';
  import QRCode from 'qrcode';
  import { call, native } from './api';
  import type {
    ControlMode,
    ControlParticipant,
    ControlPermissions,
  } from '@ytm/protocol';
  let { mode }: { mode: ControlMode } = $props();
  type Status = {
    session_id: string;
    mode: ControlMode | null;
    links: string[];
    participants: ControlParticipant[];
    paired: { id: string; name: string }[];
    relay_status: string;
    discovery_error: string;
  };
  let status = $state<Status>({
    session_id: '',
    mode: null,
    links: [],
    participants: [],
    paired: [],
    relay_status: '',
    discovery_error: '',
  });
  let relay = $state(false);
  let busy = $state(false);
  let error = $state('');
  let selected = $state('');
  let qr = $state('');
  const permissionLabels: Record<keyof ControlPermissions, string> = {
    transport: 'Play / pause',
    skip: 'Skip',
    seek: 'Seek',
    queue: 'Edit queue',
    replace_queue: 'Replace / clear queue',
    volume: 'Host volume',
    catalog: 'Public search',
  };
  const defaultPermissions: ControlPermissions = {
    transport: true,
    skip: true,
    seek: true,
    queue: true,
    replace_queue: false,
    volume: false,
    catalog: true,
  };
  async function refresh() {
    if (!native) return;
    status = await call<Status>('control_status');
    if (!status.links.includes(selected)) selected = status.links[0] || '';
  }
  onMount(() => {
    let disposed = false,
      fetching = false;
    async function poll() {
      if (disposed || fetching) return;
      fetching = true;
      try {
        await refresh();
      } catch (e) {
        error = String(e);
      } finally {
        fetching = false;
      }
    }
    void poll();
    const timer = setInterval(poll, 1500);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  });
  $effect(() => {
    const link = selected;
    let cancelled = false;
    qr = '';
    if (link)
      void QRCode.toDataURL(link, { width: 240, margin: 2 }).then((value) => {
        if (!cancelled) qr = value;
      });
    return () => {
      cancelled = true;
    };
  });
  async function run(command: string, args?: Record<string, unknown>) {
    busy = true;
    error = '';
    try {
      await call(command, args);
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  function permission(
    peer: ControlParticipant,
    key: keyof ControlPermissions,
    checked: boolean,
  ) {
    void run('control_participant', {
      id: peer.id,
      permissions: { ...peer.permissions, [key]: checked },
    });
  }
</script>

<section class="sharing">
  <div class="eyebrow">ONE HOST. SHARED CONTROLS.</div>
  <h1>
    {mode === 'personal' ? 'Remote control' : 'Group control'}<span class="mint"
      >.</span
    >
  </h1>
  <p>
    {mode === 'personal'
      ? 'Pair your own devices with this player. Approved devices can reconnect until you revoke them.'
      : 'Invite guests to control this player. Every new group session requires host approval.'}
  </p>
  <p>
    Music plays on this host’s selected audio output. Everyone else sends
    commands.
  </p>
  {#if error}<p class="failure" role="alert">{error}</p>{/if}
  {#if status.discovery_error}<p class="failure" role="status">
      {status.discovery_error}
    </p>{/if}
  {#if !status.session_id}
    <label class="option"
      ><input type="checkbox" bind:checked={relay} /> Also connect through the configured
      Cloudflare relay</label
    >
    <button
      class="primary"
      disabled={busy || !native}
      onclick={() => run('control_start', { mode, relay })}
      >Start {mode === 'personal' ? 'personal remote' : 'group'} sharing</button
    >
  {:else}
    <h2>This device is the host</h2>
    <p>
      {status.mode === 'personal' ? 'Personal remote' : 'Group control'} session ·
      {status.relay_status || 'LAN only'}
    </p>
    <button disabled={busy} onclick={() => run('control_end')}
      >End sharing and disconnect everyone</button
    >
    <h2>Invite a controller</h2>
    <p>
      This host appears under Nearby devices in the app's player device picker.
      Each new device still needs your approval. QR codes and manual addresses
      are available if your network blocks discovery.
    </p>
    <label
      >Connection address<select bind:value={selected}
        >{#each status.links as link}<option value={link}
            >{link.split('/#')[0].split('/controller/')[0]}</option
          >{/each}</select
      ></label
    >
    {#if qr}<img
        src={qr}
        alt="Scan to request admission to this host"
        width="240"
        height="240"
      />{/if}
    <input aria-label="Invitation link" readonly value={selected} />
    <div class="actions">
      <button
        onclick={() =>
          navigator.clipboard
            .writeText(selected)
            .catch((e) => (error = String(e)))}>Copy invitation link</button
      ><button disabled={busy} onclick={() => run('control_rotate')}
        >Rotate invitation</button
      >
    </div>
    <p>
      Open or scan the link on a trusted network, then approve the device below.
      Leave your listening room before switching modes.
    </p>
    <h2>Participants</h2>
    {#if !status.participants.length}<p>Waiting for admission requests.</p>{/if}
    {#each status.participants as peer (peer.id)}
      <article>
        <h3>{peer.name}</h3>
        <p>
          {peer.approved
            ? peer.connected
              ? 'Connected'
              : 'Disconnected'
            : 'Awaiting approval'}
        </p>
        {#if !peer.approved}<button
            disabled={busy}
            onclick={() =>
              run('control_participant', {
                id: peer.id,
                permissions: defaultPermissions,
              })}>Approve device</button
          >
        {:else}<div class="permissions">
            {#each Object.entries(permissionLabels) as [key, label]}<label
                ><input
                  type="checkbox"
                  disabled={busy}
                  checked={peer.permissions[key as keyof ControlPermissions]}
                  onchange={(e) =>
                    permission(
                      peer,
                      key as keyof ControlPermissions,
                      e.currentTarget.checked,
                    )}
                />{label}</label
              >{/each}
          </div>{/if}
        <button
          disabled={busy}
          onclick={() =>
            run('control_participant', { id: peer.id, permissions: null })}
          >{peer.approved ? 'Remove and revoke' : 'Reject request'}</button
        >
      </article>
    {/each}
  {/if}
  <h2>Paired personal devices</h2>
  {#if !status.paired.length}<p>No saved devices.</p>{/if}
  {#each status.paired as device}<article>
      <strong>{device.name}</strong><button
        disabled={busy}
        onclick={() =>
          run('control_participant', { id: device.id, permissions: null })}
        >Revoke device</button
      >
    </article>{/each}
</section>

<style>
  .sharing {
    max-width: 800px;
  }
  h1 {
    font-size: 36px;
    margin: 12px 0;
  }
  h2 {
    margin-top: 30px;
  }
  p {
    color: var(--muted, #a4b2aa);
    line-height: 1.6;
  }
  button,
  input,
  select {
    font: inherit;
    color: inherit;
    background: var(--surface, #1c2922);
    border: 1px solid #3c5145;
    border-radius: 10px;
    padding: 11px;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  input:not([type='checkbox']),
  select {
    display: block;
    width: 100%;
    margin: 10px 0;
    min-width: 0;
  }
  label.option {
    display: block;
    margin: 24px 0;
  }
  .primary {
    background: #b5edcf;
    color: #12271b;
  }
  .actions {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
  }
  img {
    display: block;
    margin: 20px 0;
    border-radius: 12px;
  }
  article {
    padding: 18px;
    border: 1px solid #304337;
    border-radius: 14px;
    margin: 12px 0;
  }
  article > button {
    margin: 8px;
  }
  .permissions {
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
    margin: 18px 0;
  }
  .permissions label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .failure {
    color: #f9b6a8;
  }
</style>
