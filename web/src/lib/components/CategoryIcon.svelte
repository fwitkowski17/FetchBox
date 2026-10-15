<script lang="ts">
  import type { Category } from '../api/types';
  import Icon from './Icon.svelte';

  interface Props {
    category: Category;
    size?: number;
    /** Draw the tinted square behind the icon. */
    tile?: boolean;
  }

  let { category, size = 22, tile = false }: Props = $props();

  // Full class names (not built from strings) so Tailwind can see them.
  const COLOR: Record<Category, string> = {
    photo: 'text-cat-photo bg-cat-photo/15',
    music: 'text-cat-music bg-cat-music/15',
    video: 'text-cat-video bg-cat-video/15',
    program: 'text-cat-program bg-cat-program/15',
    archive: 'text-cat-archive bg-cat-archive/15',
    other: 'text-cat-other bg-cat-other/15',
  };
</script>
<span
  class={[
    'inline-flex shrink-0 items-center justify-center',
    COLOR[category],
    tile ? 'rounded-xl' : 'bg-transparent',
  ]}
  style:width={tile ? `${size + 20}px` : undefined}
  style:height={tile ? `${size + 20}px` : undefined}
>
  <Icon name={category} {size} />
</span>
