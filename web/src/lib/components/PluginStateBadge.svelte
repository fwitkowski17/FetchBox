<script lang="ts">
  import type { PluginState } from '../api/types';
  import { t } from '../i18n';
  import { stateTone, type Tone } from '../util/plugins';

  let { state }: { state: PluginState } = $props();

  const STYLE: Record<Tone, string> = {
    good: 'bg-success-soft text-success',
    busy: 'bg-accent-soft text-accent',
    bad: 'bg-danger-soft text-danger',
    warn: 'bg-warning-soft text-warning',
    muted: 'bg-surface-2 text-muted',
  };
  const tone = $derived(stateTone(state));
</script>

<span
  data-state={state}
  class={[
    'inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-semibold whitespace-nowrap',
    STYLE[tone],
  ]}
>
  <span class={['size-1.5 rounded-full bg-current', tone === 'busy' && 'animate-pulse']}></span>
  {t(`ext.state.${state}`)}
</span>
