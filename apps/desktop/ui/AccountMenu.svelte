<script lang="ts">
  import {
    UserRound,
    ChevronDown,
    SlidersHorizontal,
    LogOut,
  } from '@lucide/svelte';
  import type { AccountStatus } from '@ytm/protocol';
  let {
    account,
    onAccount,
    onLogout,
  }: { account: AccountStatus; onAccount: () => void; onLogout: () => void } =
    $props();
  let menu: HTMLDetailsElement;
  function choose(action: () => void) {
    menu.open = false;
    action();
  }
</script>

<details class="account-menu" bind:this={menu}>
  <summary
    class="music-link"
    aria-label={account.signed_in
      ? `${account.name || 'Signed in'} account settings`
      : 'Account settings'}
  >
    <UserRound size={14} /><span
      >{account.signed_in ? account.name || 'Signed in' : 'Account'}</span
    ><ChevronDown size={13} />
  </summary>
  <div class="account-dropdown">
    <button onclick={() => choose(onAccount)}
      ><SlidersHorizontal size={15} />Account settings</button
    >
    {#if account.signed_in}<button onclick={() => choose(onLogout)}
        ><LogOut size={15} />Sign out</button
      >{/if}
  </div>
</details>
