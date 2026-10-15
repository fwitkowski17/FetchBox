<script lang="ts">
  // Theme and accent colour: the look of the app. Shared by Settings and the first-run setup.
  import { t } from '../i18n';
  import AccentPicker from './AccentPicker.svelte';
  import ThemePicker from './ThemePicker.svelte';
</script>

<ThemePicker />
<p class="hint">{t('settings.theme.hint')}</p>

<div class="field">
  <span class="label">{t('settings.accent')}</span>
  <AccentPicker />
</div>
