import { mount } from 'svelte';
import '@fontsource-variable/dm-sans';
import '@fontsource-variable/manrope';
import App from './App.svelte';
import QuickSearch from './QuickSearch.svelte';
import './style.css';
import './music.css';
import './theme.css';
import './layout.css';
const quickSearch =
  new URLSearchParams(location.search).get('panel') === 'search';
document.documentElement.classList.toggle('quick-search-mode', quickSearch);
mount(quickSearch ? QuickSearch : App, {
  target: document.getElementById('app')!,
});
