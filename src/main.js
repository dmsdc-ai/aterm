import { mount } from 'svelte'
import './app.css'
import App from './App.svelte'

let app = null

try {
  app = mount(App, {
    target: document.getElementById('app'),
  })

  // Clear auto-reload timeout and remove loading spinner
  if (window.__atermClearMountTimeout) window.__atermClearMountTimeout()
  const loader = document.getElementById('loader')
  if (loader) loader.remove()
} catch (e) {
  console.error('[aterm] Svelte mount failed:', e)
  const errEl = document.getElementById('loader-error')
  if (errEl) {
    errEl.style.display = 'block'
    errEl.textContent = 'Mount error: ' + e.message
  }
}

export default app
