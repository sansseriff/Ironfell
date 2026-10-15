import { mount } from 'svelte'
import './app.css'
import './ui.css'
import App from './App.svelte'

import { startTheme } from './runtime/theme.svelte'

startTheme()

const app = mount(App, {
  target: document.getElementById('app')!,
})

export default app



