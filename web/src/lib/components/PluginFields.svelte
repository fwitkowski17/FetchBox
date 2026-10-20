<script lang="ts">
  // The fields of a plugin's settings or dialog, drawn from its declaration. Everything the plugin
  // wrote (labels, help, choices) is shown as text.
  import type { PluginField } from '../api/types';
  import { i18n, t } from '../i18n';
  import { SvelteSet } from 'svelte/reactivity';
  import { isShown, tr, type FieldError, type Values } from '../util/pluginUi';
  import Toggle from './Toggle.svelte';

  interface Props {
    fields: readonly PluginField[];
    values: Values;
    /** The plugin's own translations (`PluginView.i18n`). */
    texts: Record<string, Record<string, string>>;
    errors?: Record<string, FieldError>;
    /** Secrets that already have a stored value: the field says so instead of showing it. */
    stored?: ReadonlySet<string>;
    /** Secrets the user chose to remove. */
    clearing?: SvelteSet<string>;
    /** Prefix for element ids, unique on the page. */
    idPrefix: string;
    disabled?: boolean;
  }

  let {
    fields,
    values = $bindable(),
    texts,
    errors = {},
    stored = new Set<string>(),
    clearing,
    idPrefix,
    disabled = false,
  }: Props = $props();

  const L = (text: string | undefined) => tr(texts, i18n.locale, text ?? '');

  function errorText(e: FieldError): string {
    switch (e.code) {
      case 'required':
        return t('ext.field.required');
      case 'number':
        return t('ext.field.number');
      case 'min':
        return t('ext.field.min', { limit: e.limit });
      case 'max':
        return t('ext.field.max', { limit: e.limit });
    }
  }

  function text(key: string): string {
    const v = values[key];
    return typeof v === 'string' ? v : v === undefined ? '' : String(v);
  }
</script>

{#each fields as f (f.key)}
  {#if isShown(f, values)}
    {@const id = `${idPrefix}-${f.key}`}
    {@const error = errors[f.key]}
    {#if f.type === 'info'}
      <p class="hint [overflow-wrap:anywhere]" data-field={f.key}>{L(f.label || f.help)}</p>
    {:else if f.type === 'toggle'}
      <div data-field={f.key}>
        <Toggle
          bind:checked={() => values[f.key] === true, (v) => (values[f.key] = v)}
          hint={L(f.help)}
          {disabled}
        >
          {L(f.label)}
        </Toggle>
      </div>
    {:else}
      <div class="field" data-field={f.key}>
        <label for={id}>
          {L(f.label)}{#if f.required}<span class="text-danger" aria-hidden="true"> *</span>{/if}
        </label>
        {#if f.type === 'textarea'}
          <textarea
            {id}
            class="input min-h-24"
            value={text(f.key)}
            oninput={(e) => (values[f.key] = e.currentTarget.value)}
            placeholder={L(f.placeholder ?? '')}
            aria-invalid={error ? 'true' : undefined}
            {disabled}
          ></textarea>
        {:else if f.type === 'output'}
          <textarea
            {id}
            class="input min-h-24 font-mono text-sm"
            readonly
            value={text(f.key)}
            placeholder={L(f.placeholder ?? '')}
          ></textarea>
        {:else if f.type === 'select'}
          <select
            {id}
            class="select"
            value={text(f.key)}
            onchange={(e) => (values[f.key] = e.currentTarget.value)}
            aria-invalid={error ? 'true' : undefined}
            {disabled}
          >
            {#if !f.required && !f.options?.some((o) => o.value === '')}
              <option value=""></option>
            {/if}
            {#each f.options ?? [] as o (o.value)}
              <option value={o.value}>{L(o.label)}</option>
            {/each}
          </select>
        {:else if f.type === 'number'}
          <div class="flex items-center gap-2">
            <input
              {id}
              class="input min-w-0 flex-1"
              type="number"
              inputmode="decimal"
              min={f.min ?? undefined}
              max={f.max ?? undefined}
              step={f.step ?? 'any'}
              value={text(f.key)}
              oninput={(e) => (values[f.key] = e.currentTarget.value)}
              placeholder={L(f.placeholder ?? '')}
              aria-invalid={error ? 'true' : undefined}
              {disabled}
            />
            {#if f.unit}<span class="text-sm text-muted">{f.unit}</span>{/if}
          </div>
        {:else if f.type === 'secret'}
          <div class="flex items-center gap-2">
            <input
              {id}
              class="input min-w-0 flex-1"
              type="password"
              autocomplete="new-password"
              value={text(f.key)}
              oninput={(e) => {
                values[f.key] = e.currentTarget.value;
                if (e.currentTarget.value !== '') clearing?.delete(f.key);
              }}
              placeholder={stored.has(f.key) && !clearing?.has(f.key)
                ? t('ext.field.secretStored')
                : L(f.placeholder ?? '')}
              aria-invalid={error ? 'true' : undefined}
              {disabled}
            />
            {#if clearing && stored.has(f.key)}
              <button
                type="button"
                class="btn"
                aria-pressed={clearing.has(f.key)}
                onclick={() => {
                  if (clearing.has(f.key)) clearing.delete(f.key);
                  else {
                    clearing.add(f.key);
                    values[f.key] = '';
                  }
                }}
                {disabled}
              >
                {clearing.has(f.key) ? t('ext.field.keepSecret') : t('ext.field.removeSecret')}
              </button>
            {/if}
          </div>
        {:else}
          <input
            {id}
            class="input"
            type="text"
            value={text(f.key)}
            oninput={(e) => (values[f.key] = e.currentTarget.value)}
            placeholder={L(f.placeholder ?? '')}
            aria-invalid={error ? 'true' : undefined}
            {disabled}
          />
        {/if}
        {#if f.help}<p class="hint [overflow-wrap:anywhere]">{L(f.help)}</p>{/if}
        {#if error}<p class="text-sm text-danger" role="alert">{errorText(error)}</p>{/if}
      </div>
    {/if}
  {/if}
{/each}
