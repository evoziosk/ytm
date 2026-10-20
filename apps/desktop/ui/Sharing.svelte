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
  <header class="sharing-intro">
    <div class="eyebrow">ONE HOST. SHARED CONTROLS.</div>
    <h1>
      {mode === 'personal' ? 'Remote control' : 'Group control'}<span
        class="mint">.</span
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
  </header>
  {#if error}<p class="failure" role="alert">{error}</p>{/if}
  {#if status.discovery_error}<p class="failure" role="status">
      {status.discovery_error}
    </p>{/if}
  <div class="sharing-session">
    <div class="eyebrow">
      {status.session_id ? 'YOUR CONNECTION' : 'GET CONNECTED'}
    </div>
    {#if !status.session_id}
      <h2>Your music. Within reach.</h2>
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
        {status.mode === 'personal' ? 'Personal remote' : 'Group control'} session
        ·
        {status.relay_status || 'LAN only'}
      </p>
      <button disabled={busy} onclick={() => run('control_end')}
        >End sharing and disconnect everyone</button
      >
      <h2>Invite a controller</h2>
      <p>
        This host appears under Nearby devices in the app's player device
        picker. Each new device still needs your approval. QR codes and manual
        addresses are available if your network blocks discovery.
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
        Open or scan the link on a trusted network, then approve the device
        below. Leave your listening room before switching modes.
      </p>
      <h2>Participants</h2>
      {#if !status.participants.length}<p>
          Waiting for admission requests.
        </p>{/if}
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
  </div>
  <div class="saved-devices">
    <div class="eyebrow">YOUR DEVICES</div>
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
  </div>
</section>

<style>
  .sharing {
    display: grid;
    grid-template-columns: minmax(0, 1.5fr) minmax(240px, 1fr);
    gap: 24px;
    margin-top: 26px;
    align-items: start;
  }
  .sharing-intro {
    grid-column: 1 / -1;
    max-width: 700px;
    margin-bottom: 10px;
  }
  .sharing-session,
  .saved-devices {
    min-width: 0;
    padding: 28px;
    border: 1px solid var(--line);
    border-radius: 24px;
    background: linear-gradient(135deg, #34273855, #24203080);
    box-shadow: var(--glass-shadow);
    backdrop-filter: blur(28px);
  }
  .sharing-session > .eyebrow,
  .saved-devices > .eyebrow {
    margin-bottom: 20px;
  }
  .saved-devices h2,
  .sharing-session > h2:first-of-type {
    margin-top: 0;
  }
  .saved-devices p {
    margin-top: 16px;
  }
  .sharing-intro p + p {
    margin-top: 8px;
    font-size: 12px;
  }
  .sharing-session > p {
    margin: 12px 0 20px;
  }
  .sharing > .failure {
    grid-column: 1 / -1;
  }
  @media (max-width: 1100px) {
    .sharing {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  @media (max-width: 700px) {
    .sharing-session,
    .saved-devices {
      padding: 22px;
    }
  }
  h1 {
    font-size: 36px;
    margin: 12px 0;
  }
  h2 {
    margin-top: 28px;
    margin-bottom: 14px;
    font-size: 20px;
    font-weight: 500;
    letter-spacing: -0.5px;
  }
  p {
    color: var(--muted, #b2a4a9);
    line-height: 1.6;
  }
  button,
  input,
  select {
    font: inherit;
    color: inherit;
    background: var(--surface, #221c29);
    border: 1px solid #4c3c51;
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
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 24px 0;
    padding: 18px;
    border-radius: 14px;
    background: #c4a2ce08;
    font-size: 12px;
    line-height: 1.8;
  }
  input[type='checkbox'] {
    accent-color: var(--accent);
    width: 16px;
    height: 16px;
    flex-shrink: 0;
  }
  .primary {
    background: #e0c2cc;
    color: #1c1722;
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
    padding: 10px;
    background: white;
    max-width: 100%;
    height: auto;
  }
  article {
    padding: 18px;
    border: 1px solid #393043;
    border-radius: 14px;
    margin: 12px 0;
    background: #c4a2ce06;
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
