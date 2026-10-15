<script lang="ts">
  import { t } from '../i18n';
  import { accent } from '../stores/accent.svelte';
  import { ACCENTS, swatch } from '../util/accent';
  import Icon from './Icon.svelte';
</script>

<div class="flex flex-wrap gap-3" role="radiogroup" aria-label={t('settings.accent')}>
  {#each ACCENTS as a (a.id)}
    {@const selected = accent.id === a.id}
    <label
      class="relative grid size-10 cursor-pointer place-items-center rounded-full has-focus-visible:outline-2 has-focus-visible:outline-offset-2 has-focus-visible:outline-text"
      title={t(`accent.${a.id}`)}
    >
      <input
        type="radio"
        name="accent"
        value={a.id}
        class="absolute inset-0 size-full cursor-pointer opacity-0"
        checked={selected}
        aria-label={t(`accent.${a.id}`)}
        onchange={() => accent.set(a.id)}
      />
      <span
        class={[
          'grid size-8 place-items-center rounded-full text-white shadow-sm ring-offset-2 ring-offset-surface transition-all',
          selected ? 'ring-2 ring-text' : 'ring-0',
        ]}
        style:background={swatch(a)}
      >
        {#if selected}<Icon name="check" size={16} />{/if}
      </span>
    </label>
  {/each}
</div>
