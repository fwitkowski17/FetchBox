<script lang="ts">
  // Everything about how the app looks and reads, as in Settings: theme, accent colour, language and
  // the unit speeds are written in. (The first-run setup asks for the language and the look in
  // separate steps and leaves the speed unit at its default.)
  import LanguageSelect from './LanguageSelect.svelte';
  import LookFields from './LookFields.svelte';
  import SpeedUnitSelect from './SpeedUnitSelect.svelte';
</script>

<LookFields />

<LanguageSelect />

<SpeedUnitSelect />
