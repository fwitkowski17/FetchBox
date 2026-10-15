// Is the screen wide enough for the permanent side panel? (Below that it becomes a drawer.)
// Keep the breakpoint in sync with the `64rem` media queries in Sidebar.svelte and App.svelte.
const DESKTOP = '(min-width: 64rem)';

class Media {
  desktop = $state(true);

  constructor() {
    const mq = window.matchMedia(DESKTOP);
    this.desktop = mq.matches;
    mq.addEventListener('change', (e) => (this.desktop = e.matches));
  }
}

export const media = new Media();
