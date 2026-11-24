<script setup>
// The small menu a chat's link shows on right-click (or on click, when Settings > Rooms > Browser
// says "Ask each time"): Castle's browser, the member's own browser, or Copy link. "Remember my
// choice" saves the way, so the next click goes straight there.
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { browserOpen, browserOutside, getSettings, saveSettings } from '../api/castle.js'
import { LINK_MENU, linkMenus } from './links.js'

const shown = ref(null) // { url, x, y }
const remember = ref(false)
const problem = ref('')
const WIDTH = 320
const HEIGHT = 230

function place({ url, x, y }) {
  problem.value = ''
  remember.value = false
  shown.value = {
    url,
    x: Math.max(8, Math.min(x, window.innerWidth - WIDTH - 8)),
    y: Math.max(8, Math.min(y, window.innerHeight - HEIGHT - 8)),
  }
}
const onMenu = (event) => place(event.detail)
const close = () => (shown.value = null)
const onKey = (event) => event.key === 'Escape' && close()

async function keepChoice(way) {
  if (!remember.value) return
  const { settings } = await getSettings()
  await saveSettings({ ...settings, linkOpen: way })
}

async function go(way) {
  const url = shown.value?.url
  if (!url) return
  try {
    if (way === 'castle') await browserOpen(url)
    else await browserOutside(url)
    await keepChoice(way)
    close()
  } catch (reason) {
    problem.value = String(reason)
  }
}

async function copy() {
  try {
    await navigator.clipboard.writeText(shown.value?.url ?? '')
    close()
  } catch {
    problem.value = 'The link could not be copied.'
  }
}

onMounted(() => {
  linkMenus.addEventListener(LINK_MENU, onMenu)
  window.addEventListener('keydown', onKey)
})
onBeforeUnmount(() => {
  linkMenus.removeEventListener(LINK_MENU, onMenu)
  window.removeEventListener('keydown', onKey)
})
</script>

<template>
  <div v-if="shown" class="veil" @click.self="close" @contextmenu.prevent="close">
    <div class="menu" role="menu" :aria-label="`Open ${shown.url}`" :style="{ left: `${shown.x}px`, top: `${shown.y}px` }">
      <span class="title">OPEN THIS LINK</span>
      <button role="menuitem" class="choice" @click="go('castle')">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M3 9h18" /></svg>
        <span><span>In Castle's browser</span><small>a new tab in the BROWSER window</small></span>
      </button>
      <button role="menuitem" class="choice" @click="go('system')">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
        <span><span>In your browser</span><small>the one your computer uses</small></span>
      </button>
      <button role="menuitem" class="choice plain" @click="copy">Copy link</button>
      <label class="remember">
        Remember my choice
        <input v-model="remember" type="checkbox" />
      </label>
      <p v-if="problem" class="problem" role="alert">{{ problem }}</p>
    </div>
  </div>
</template>

<style scoped>
.veil {
  position: fixed;
  inset: 0;
  z-index: 9000;
}
.menu {
  position: fixed;
  width: 320px;
  box-sizing: border-box;
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  background: var(--panel-background, rgba(8, 16, 44, 0.98));
  backdrop-filter: blur(var(--glass-blur, 14px));
  border: 1px solid var(--line);
  border-radius: 12px;
  box-shadow: 0 18px 48px rgba(0, 0, 0, 0.55);
  color: var(--text);
}
.title {
  font-size: 11px;
  letter-spacing: 0.18em;
  color: var(--text-dim);
  padding: 8px 10px 6px;
}
.choice {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: inherit;
  font: inherit;
  font-size: 14px;
  text-align: left;
  cursor: pointer;
}
.choice svg {
  color: var(--accent);
  flex-shrink: 0;
}
.choice > span {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.choice small {
  font-size: 12px;
  color: var(--text-dim);
}
.choice.plain {
  padding-left: 40px;
}
.choice:hover,
.choice:focus-visible {
  background: var(--hover);
  outline: none;
}
.remember {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 4px;
  padding: 10px;
  border-top: 1px solid var(--line);
  font-size: 13px;
  cursor: pointer;
}
.remember input {
  accent-color: var(--control-accent, var(--accent));
  width: 18px;
  height: 18px;
}
.problem {
  margin: 4px 10px;
  font-size: 12px;
  color: var(--bad);
}
</style>
