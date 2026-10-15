<script lang="ts">
  // Theme, accent colour and language: shared by Settings and the first-run setup.
  import { t } from '../i18n';
  import AccentPicker from './AccentPicker.svelte';
  import LanguageSelect from './LanguageSelect.svelte';
  import SpeedUnitSelect from './SpeedUnitSelect.svelte';
  import ThemePicker from './ThemePicker.svelte';
</script>

<ThemePicker />
<p class="hint">{t('settings.theme.hint')}</p>

<div class="field">
  <span class="label">{t('settings.accent')}</span>
  <AccentPicker />
</div>

<LanguageSelect />

<SpeedUnitSelect />
