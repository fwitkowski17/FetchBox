<script lang="ts">
  import { LOCALES, i18n, t } from '../i18n';

  let { id = 'language' }: { id?: string } = $props();
</script>

<div class="field">
  <label for={id}>{t('settings.language')}</label>
  <select
    {id}
    class="select"
    value={i18n.pref}
    onchange={(e) => void i18n.setPref(e.currentTarget.value)}
  >
    <option value="auto">{t('settings.language.auto')}</option>
    {#each LOCALES as l (l.code)}
      <option value={l.code} lang={l.code}>{l.name}</option>
    {/each}
  </select>
</div>
