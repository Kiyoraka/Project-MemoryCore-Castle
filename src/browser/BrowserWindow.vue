<script setup>
// The BROWSER window's own page (browser.rs, webview `browser-ui`): the tabs and the toolbar,
// exactly BAR (94 px) tall above the web page, or the whole window with the new-tab page when the
// tab in front has no page yet. The web pages themselves are separate webviews below it.
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  browserClose,
  browserGo,
  browserOpen,
  browserOutside,
  browserPutInChat,
  browserState,
  browserStep,
  browserSwitch,
  devServers,
  getSettings,
  onBrowserState,
} from '../api/castle.js'
import { isMac } from '../platform.js'
import { newTabShortcuts, shownAddress } from './browserview.js'

const tabs = ref({ list: [], active: null })
const typed = ref('')
const problem = ref('')
const servers = ref([])
const newTab = ref('shortcuts')
const field = ref(null)
let stop = null

const current = computed(() => tabs.value.list.find((t) => t.id === tabs.value.active) ?? null)
const page = computed(() => current.value?.url ?? '')
const shortcuts = computed(() => newTabShortcuts(servers.value, newTab.value))
const secure = computed(() => page.value.startsWith('https://'))

watch(page, (url) => (typed.value = url), { immediate: true })

async function act(work) {
  problem.value = ''
  try {
    await work()
  } catch (reason) {
    problem.value = String(reason)
  }
}

const go = () => act(() => browserGo(typed.value))
const openShortcut = (url) => act(() => browserGo(url))
const step = (name) => act(() => browserStep(name))
const putInChat = () => act(() => browserPutInChat(page.value))
const outside = () => act(() => browserOutside(page.value))
const pick = (id) => act(() => browserSwitch(id))
const close = (id) => act(() => browserClose(id))
const add = () => act(() => browserOpen(null))

async function refreshNewTab() {
  try {
    servers.value = await devServers()
  } catch {
    servers.value = []
  }
}

function onKey(event) {
  const mod = isMac ? event.metaKey : event.ctrlKey
  if (!mod) return
  const key = event.key.toLowerCase()
  if (key === 't') add()
  else if (key === 'w' && current.value) close(current.value.id)
  else if (key === 'l') field.value?.select()
  else return
  event.preventDefault()
}

watch(
  () => current.value && !current.value.url,
  async (blank) => {
    if (!blank) return
    if (newTab.value === 'artifacts') return openShortcut(shortcuts.value[0]?.url)
    await refreshNewTab()
    await nextTick()
    field.value?.focus()
  },
)

onMounted(async () => {
  document.title = 'MemoryCore · Browser'
  window.addEventListener('keydown', onKey)
  stop = await onBrowserState((next) => (tabs.value = next))
  try {
    newTab.value = (await getSettings()).settings?.browserNewTab ?? 'shortcuts'
  } catch {
    // the default shortcuts
  }
  tabs.value = await browserState()
  if (current.value && !current.value.url) refreshNewTab()
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey)
  stop?.()
})
</script>

<template>
  <div class="browser">
    <div class="bar">
      <div class="strip" role="tablist" aria-label="Tabs">
        <div v-for="t in tabs.list" :key="t.id" class="tab" :class="{ on: t.id === tabs.active }" role="tab" :aria-selected="t.id === tabs.active" :title="t.url || t.title" @click="pick(t.id)" @auxclick.middle="close(t.id)">
          <span class="dot" :class="{ local: t.url?.startsWith('http://'), blank: !t.url }" aria-hidden="true"></span>
          <span class="name">{{ t.title }}</span>
          <button class="x" :aria-label="`Close ${t.title}`" @click.stop="close(t.id)">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
          </button>
        </div>
        <button class="icon add" aria-label="New tab" :title="`New tab (${isMac ? '⌘' : 'Ctrl+'}T)`" @click="add">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
        </button>
        <span class="safe" title="Web pages here get no Castle permissions: they cannot read your memory, chats or files.">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l8 3v6c0 4.5-3.4 8.4-8 9-4.6-.6-8-4.5-8-9V6z" /></svg>
          This window can't reach Castle or your memory
        </span>
      </div>
      <div class="tools">
        <button class="icon" aria-label="Back" :disabled="!page" @click="step('back')">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 18l-6-6 6-6" /></svg>
        </button>
        <button class="icon" aria-label="Forward" :disabled="!page" @click="step('forward')">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 18l6-6-6-6" /></svg>
        </button>
        <button class="icon" aria-label="Reload" :disabled="!page" @click="step('reload')">
          <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 4v7h-7" /></svg>
        </button>
        <form class="address" @submit.prevent="go">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" :stroke="secure ? 'var(--good)' : 'var(--warn)'" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" /></svg>
          <label class="hidden" for="browser-address">Address</label>
          <input id="browser-address" ref="field" v-model="typed" placeholder="Type an address or search" spellcheck="false" autocomplete="off" @focus="$event.target.select()" />
        </form>
        <button class="pill" :disabled="!page" title="Put this page's address in HOME's chat" @click="putInChat">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z" /></svg>
          Put in chat
        </button>
        <button class="pill" :disabled="!page" @click="outside">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
          Open in your browser
        </button>
      </div>
    </div>

    <main v-if="current && !current.url" class="new-tab">
      <span class="label">NEW TAB</span>
      <div v-if="shortcuts.length" class="grid">
        <button v-for="s in shortcuts" :key="s.url" class="shortcut" @click="openShortcut(s.url)">
          <span class="letter" aria-hidden="true">{{ s.letter }}</span>{{ s.name }}
        </button>
      </div>
      <span class="hint">{{ shortcuts.length ? 'Your Artifacts and the dev servers running now' : 'Type an address or something to search for' }}</span>
    </main>
    <p v-if="problem" class="problem" role="alert">{{ problem }}</p>
  </div>
</template>

<style scoped>
/* the browser window has no App.vue around it: its own colours, Castle's defaults */
.browser {
  --text: #eef3ff;
  --text-dim: #a6b8de;
  --accent: #8fb8ff;
  --line: rgba(150, 190, 255, 0.34);
  --hover: rgba(150, 190, 255, 0.14);
  --good: #8fe3b8;
  --warn: #ffd59a;
  --bad: #ff9aa8;
  position: fixed;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: #050b1f;
  color: var(--text);
  font-family: -apple-system, 'Segoe UI', system-ui, sans-serif;
  user-select: none;
}
.bar {
  height: 94px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
}
.strip {
  height: 42px;
  display: flex;
  align-items: flex-end;
  gap: 4px;
  padding: 0 12px;
  background: rgba(6, 12, 34, 0.96);
  overflow: hidden;
}
.tab {
  height: 36px;
  width: 220px;
  min-width: 90px;
  flex-shrink: 1;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 6px 0 12px;
  border-radius: 8px 8px 0 0;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
}
.tab:hover {
  background: rgba(150, 190, 255, 0.08);
}
.tab.on {
  background: rgba(10, 20, 52, 0.92);
  color: var(--text);
  box-shadow: inset 0 2px 0 var(--accent);
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 4px;
  flex-shrink: 0;
  background: var(--accent);
}
.dot.local {
  background: var(--good);
}
.dot.blank {
  background: var(--text-dim);
}
.name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.x {
  width: 22px;
  height: 22px;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: var(--text-dim);
  cursor: pointer;
}
.x:hover {
  background: var(--hover);
  color: var(--text);
}
.safe {
  margin-left: auto;
  align-self: center;
  display: flex;
  align-items: center;
  gap: 6px;
  padding-left: 12px;
  font-size: 12px;
  color: var(--text-dim);
  white-space: nowrap;
}
.safe svg {
  color: var(--good);
}
.tools {
  height: 52px;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 12px;
  background: rgba(10, 20, 52, 0.92);
  border-bottom: 1px solid rgba(150, 190, 255, 0.18);
}
.icon {
  width: 36px;
  height: 36px;
  flex-shrink: 0;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text);
  cursor: pointer;
}
.icon.add {
  width: 32px;
  height: 32px;
  margin-bottom: 2px;
  color: var(--text-dim);
}
.icon:hover:not(:disabled) {
  background: var(--hover);
}
.icon:disabled,
.pill:disabled {
  opacity: 0.4;
  cursor: default;
}
.address {
  flex: 1;
  min-width: 120px;
  height: 36px;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 14px;
  border-radius: 18px;
  background: rgba(5, 11, 31, 0.85);
  border: 1px solid var(--line);
}
.address:focus-within {
  border-color: var(--accent);
}
.address input {
  flex: 1;
  min-width: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 14px;
  user-select: text;
}
.hidden {
  position: absolute;
  left: -9999px;
}
.pill {
  height: 36px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 14px;
  border-radius: 18px;
  border: 1px solid var(--line);
  background: rgba(150, 190, 255, 0.08);
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
  white-space: nowrap;
}
.pill:hover:not(:disabled) {
  background: rgba(150, 190, 255, 0.18);
}
.new-tab {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 28px;
  padding: 24px;
}
.label {
  font-size: 13px;
  letter-spacing: 0.22em;
  color: var(--text-dim);
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
  gap: 16px;
  width: min(640px, 100%);
}
.shortcut {
  height: 96px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  border-radius: 12px;
  border: 1px solid var(--line);
  background: rgba(12, 24, 60, 0.62);
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
.shortcut:hover {
  background: rgba(150, 190, 255, 0.18);
}
.letter {
  width: 32px;
  height: 32px;
  display: grid;
  place-items: center;
  border-radius: 8px;
  background: rgba(143, 184, 255, 0.22);
  color: var(--accent);
  font-weight: 700;
}
.hint {
  font-size: 12px;
  color: var(--text-dim);
}
.problem {
  position: fixed;
  left: 16px;
  bottom: 12px;
  margin: 0;
  padding: 8px 12px;
  border-radius: 8px;
  background: rgba(5, 11, 31, 0.92);
  border: 1px solid var(--bad);
  color: var(--bad);
  font-size: 12px;
}
@media (max-width: 860px) {
  .safe {
    display: none;
  }
}
</style>
