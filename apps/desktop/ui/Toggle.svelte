<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    disabled = false,
  }: { checked?: boolean; label: string; disabled?: boolean } = $props();
</script>

<button
  type="button"
  class="switch"
  class:on={checked}
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => (checked = !checked)}><span></span></button
>
