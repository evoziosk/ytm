<script lang="ts">
  import type { PlaybackState } from '@ytm/protocol';
  import { send } from './api';
  import { formatTime, seekResult, type SeekAttempt } from './seek';

  let {
    player,
    position,
    clock,
    locked,
    onError,
  }: {
    player: PlaybackState;
    position: number;
    clock: number;
    locked: boolean;
    onError: (message: string) => void;
  } = $props();
  let draft = $state<number | null>(null);
  let pending = $state<SeekAttempt | null>(null);
  let generation = 0;
  const disabled = $derived(
    !player.track || !player.duration || locked || player.ad,
  );
  const displayed = $derived(draft ?? pending?.position ?? position);
  $effect(() => {
    if (disabled) {
      draft = null;
      pending = null;
      return;
    }
    if (!pending) return;
    const result = seekResult(pending, player, clock);
    if (result === 'pending') return;
    pending = null;
    if (result === 'timeout')
      onError('The player did not confirm the seek. Try again.');
  });
  async function commit(value: number) {
    if (disabled || !Number.isFinite(value)) return;
    const request = ++generation;
    const attempt = {
      position: Math.min(player.duration, Math.max(0, value)),
      trackId: player.track!.video_id,
      sentAt: Date.now(),
    };
    pending = attempt;
    draft = null;
    try {
      await send({ type: 'seek', position: attempt.position });
    } catch (error) {
      if (request !== generation || !pending) return;
      pending = null;
      onError(String(error).replace(/^Error: /, ''));
    }
  }
</script>

<div class="progress" aria-busy={Boolean(pending)}>
  <span>{formatTime(displayed)}</span><input
    aria-label="Track position"
    aria-valuetext={`${formatTime(displayed)} of ${formatTime(player.duration)}${pending ? ', seeking' : ''}`}
    type="range"
    min="0"
    max={player.duration || 1}
    step="0.1"
    value={displayed}
    {disabled}
    onpointercancel={() => (draft = null)}
    oninput={(event) => (draft = Number(event.currentTarget.value))}
    onchange={(event) => void commit(Number(event.currentTarget.value))}
  /><span>{formatTime(player.duration)}</span>
</div>
