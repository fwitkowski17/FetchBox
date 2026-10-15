<script lang="ts">
  // How speeds are written in the app (cards, side panel, limits shown on cards and in menus).
  // The limit fields keep the unit picked in their own field.
  import { i18n, t } from '../i18n';
  import { SPEED_DISPLAYS, SPEED_UNIT_LABEL, type SpeedDisplay } from '../util/format';

  let { id = 'speed-unit' }: { id?: string } = $props();

  function label(d: SpeedDisplay): string {
    if (d === 'auto') return t('settings.speedUnit.auto');
    if (d === 'bits') return t('settings.speedUnit.bits');
    return SPEED_UNIT_LABEL[d];
  }
</script>

<div class="field">
  <label for={id}>{t('settings.speedUnit')}</label>
  <select
    {id}
    class="select"
    value={i18n.speedUnit}
    onchange={(e) => i18n.setSpeedUnit(e.currentTarget.value as SpeedDisplay)}
  >
    {#each SPEED_DISPLAYS as d (d)}
      <option value={d}>{label(d)}</option>
    {/each}
  </select>
  <p class="hint">{t('settings.speedUnit.hint', { example: i18n.speed(1_500_000) })}</p>
</div>
