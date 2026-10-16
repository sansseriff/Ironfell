import { mount } from 'svelte'
import './app.css'
import './ui.css'
import App from './App.svelte'

import { isNativeHost } from './runtime/native_host'

if (isNativeHost()) document.documentElement.dataset.native = 'true'

import { startTheme } from './runtime/theme.svelte'

startTheme()

const app = mount(App, {
  target: document.getElementById('app')!,
})

export default app



