import { mount } from 'svelte'
import './app.css'
import App from './App.svelte'

import { UIState } from './ui-state.svelte'

const app = mount(App, {
  target: document.getElementById('app')!,
})

export default app



