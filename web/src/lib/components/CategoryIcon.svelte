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
</script>

<span class="cat {category}" class:tile style:--box={`${size + 18}px`}>
  <Icon name={category} {size} />
</span>

<style>
  .cat {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--c);
  }

  .tile {
    width: var(--box);
    height: var(--box);
    border-radius: 12px;
    background: color-mix(in srgb, var(--c) 15%, transparent);
  }

  .photo {
    --c: var(--cat-photo);
  }
  .music {
    --c: var(--cat-music);
  }
  .video {
    --c: var(--cat-video);
  }
  .program {
    --c: var(--cat-program);
  }
  .archive {
    --c: var(--cat-archive);
  }
  .other {
    --c: var(--cat-other);
  }
</style>
