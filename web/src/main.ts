import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { i18n } from './lib/i18n';
import { accent } from './lib/stores/accent.svelte';

const target = document.getElementById('app');
if (!target) {
  throw new Error('#app element not found');
}

// The dictionary is loaded before the first render, so there is never a flash of the wrong language.
await i18n.init();
accent.init();

const app = mount(App, { target });

export default app;
