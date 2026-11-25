<script setup>
// The browser, the Keep's way: the BROWSER window's own page (browser.rs, webview `browser-ui`), or
// the BROWSER room inside Castle (`embedded`, Settings > Rooms > BROWSER > Browser opens). Chips for
// the tabs (each with Open on the right and its x), the pages side by side as one group chip with its
// list, then the panes, each with its own toolbar (PaneBar.vue). This page measures the box under each
// pane's toolbar and the core puts that tab's web page there (browser_show); while a list or a sheet is
// open over the pages, or the room is not in front, every page hides.
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import {
  browserBeside,
  browserClose,
  browserDangerBack,
  browserDangerOpen,
  browserGo,
  browserImportBookmarks,
  browserImportHistory,
  browserImportPasswords,
  browserNewTab,
  browserOpen,
  browserPasswordGuide,
  browserPhoneView,
  browserShow,
  browserState,
  browserSwitch,
  browserTranslateSettings,
  browserUnpane,
  browserWelcome,
  browserWelcomeAnswer,
  browserWidths,
  devServers,
  getSettings,
  onBrowserState,
} from '../api/castle.js'
import { isMac } from '../platform.js'
import GlassSelect from '../components/GlassSelect.vue'
import DangerScreen from './DangerScreen.vue'
import PaneBar from './PaneBar.vue'
import PasswordsSheet from './PasswordsSheet.vue'
import { boundsOf, chipCount, chipTitle, columnsOf, dragWidths, groupOf, libraryTiles, newTabShortcuts, onScreen, signInHint, singleTabs } from './browserview.js'
import { STEPS, doneLine, offeredStep, stepNumber, stepWords } from './browser_welcome.js'

// inside Castle: a room of the main window; `visible` is false while another room or a sheet is in front
const props = defineProps({
  embedded: { type: Boolean, default: false },
  visible: { type: Boolean, default: true },
})

const tabs = ref({ list: [], active: null, panes: [], widths: [], phoneView: true })
const problem = ref('')
const servers = ref([])
const newTab = ref('shortcuts')
let stop = null

const current = computed(() => tabs.value.list.find((t) => t.id === tabs.value.active) ?? null)
const shortcuts = computed(() => newTabShortcuts(servers.value, newTab.value))
const hint = computed(() => signInHint(current.value?.url ?? ''))

async function act(work) {
  problem.value = ''
  try {
    await work()
  } catch (reason) {
    problem.value = String(reason)
  }
}

// ---------- the chips and the group ----------
const group = computed(() => groupOf(tabs.value))
const singles = computed(() => singleTabs(tabs.value))
const shownIds = computed(() => onScreen(tabs.value))
const shownTabs = computed(() => shownIds.value.map((id) => tabs.value.list.find((t) => t.id === id)).filter(Boolean))
const many = computed(() => shownTabs.value.length > 1)
const groupFocus = ref(null)
watch(
  () => tabs.value.active,
  (id) => {
    if (group.value?.ids.includes(id)) groupFocus.value = id
  },
)
const pick = (id) => act(() => browserSwitch(id))
const pickGroup = () => pick(group.value.ids.includes(groupFocus.value) ? groupFocus.value : group.value.ids[0])
const close = (id) => act(() => browserClose(id))
const add = () => act(() => browserOpen(null))
// a private tab: keeps nothing (Ctrl+Shift+T)
const addPrivate = () => act(() => browserOpen(null, true))
const beside = (id) => act(() => browserBeside(id))
const phoneView = () => act(() => browserPhoneView(!tabs.value.phoneView))
// the group chip's list: its pages, each with Take out
const groupMenu = ref(false)
watch(groupMenu, (open) => overlay('group-menu', open))
watch(group, (g) => {
  if (!g) groupMenu.value = false
})
const takeOut = (id) => act(() => browserUnpane(id))
// the Passwords sheet: the pages step aside while it is open
const passwordsOpen = ref(false)
watch(passwordsOpen, (open) => overlay('passwords', open))
// a column's x: the tab stays open as a chip, the column goes
const closeColumn = (id) => act(() => browserUnpane(id))
function focusPane(id) {
  if (id !== tabs.value.active) act(() => browserSwitch(id))
}

// ---------- the panes: columns, grips, and where each page goes ----------
const shares = ref([])
watch(
  () => tabs.value.widths,
  (w) => (shares.value = [...(w ?? [])]),
  { immediate: true },
)
const gridColumns = computed(() => columnsOf(many.value ? shares.value : []))
const panesEl = ref(null)
let dragging = null
function grab(event, at) {
  dragging = { at, x: event.clientX, start: [...shares.value] }
  event.target.setPointerCapture(event.pointerId)
}
function drag(event) {
  if (!dragging) return
  shares.value = dragWidths(dragging.start, dragging.at, event.clientX - dragging.x, panesEl.value?.clientWidth ?? 0, 0.15)
}
function release() {
  if (!dragging) return
  dragging = null
  act(() => browserWidths([...shares.value]))
}

// the boxes the pages go into, measured; while something is open over the pages they all hide
const pageEls = new Map()
const overlays = reactive(new Set())
function overlay(key, on) {
  if (on) overlays.add(key)
  else overlays.delete(key)
}
let frame = 0
let boxes = null
function schedule() {
  if (frame) return
  frame = requestAnimationFrame(() => {
    frame = 0
    place()
  })
}
function place() {
  const hidden = overlays.size > 0 || (props.embedded && !props.visible)
  const placed = hidden
    ? []
    : shownTabs.value
        .filter((t) => t.url && !t.danger)
        .map((t) => {
          const box = boundsOf(pageEls.get(t.id)?.getBoundingClientRect())
          return box ? { id: t.id, ...box } : null
        })
        .filter(Boolean)
  browserShow(placed).catch(() => {})
}
function pageEl(id, el) {
  const before = pageEls.get(id)
  if (before) boxes?.unobserve(before)
  if (el) {
    pageEls.set(id, el)
    boxes?.observe(el)
  } else pageEls.delete(id)
  schedule()
}
watch([tabs, () => overlays.size, () => props.visible, shares], schedule, { deep: true })

// ---------- translation settings, shared by the panes ----------
const translating = ref({ languages: [], language: 'en', sites: [], canTranslate: false, canKeepSites: false, bubbleWay: '' })
async function readTranslating() {
  translating.value = await browserTranslateSettings().catch(() => translating.value)
}

// ---------- the first-open welcome, one step at a time on the new tab ----------
const welcome = ref(null)
const welcomeBusy = ref(false)
const welcomeSaid = ref('')
const profile = ref('')
const guided = ref(false)
const shownStep = computed(() => (welcome.value ? offeredStep(welcome.value.answers, welcome.value.chrome) : null))
const words = computed(() => stepWords(shownStep.value, welcome.value?.companion))

async function readWelcome() {
  try {
    welcome.value = await browserWelcome()
    profile.value = profile.value || welcome.value.profiles?.[0]?.folder || ''
  } catch {
    welcome.value = null
  }
}

async function answer(kind) {
  const s = shownStep.value
  if (!s) return
  welcomeBusy.value = true
  problem.value = ''
  try {
    if (kind === 'skip') await browserWelcomeAnswer(s, 'skip')
    else if (s === 'history') welcomeSaid.value = doneLine(s, await browserImportHistory(profile.value))
    else if (s === 'bookmarks') welcomeSaid.value = doneLine(s, await browserImportBookmarks(profile.value))
    else if (s === 'passwords') {
      const done = await browserImportPasswords(guided.value)
      if (!done) return
      welcomeSaid.value = doneLine(s, done)
    } else await browserWelcomeAnswer(s, 'yes')
    if (kind === 'skip') welcomeSaid.value = ''
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    welcomeBusy.value = false
    await readWelcome()
  }
}

async function askGuide() {
  problem.value = ''
  try {
    await browserPasswordGuide()
    guided.value = true
    welcomeSaid.value = `${welcome.value?.companion ?? 'Your companion'} is ready to guide you in HOME's chat.`
  } catch (reason) {
    problem.value = String(reason)
  }
}

// ---------- the new tab's lists ----------
const library = ref({ visited: [], marks: [] })
async function refreshNewTab() {
  try {
    servers.value = await devServers()
  } catch {
    servers.value = []
  }
  library.value = libraryTiles(await browserNewTab().catch(() => null))
}
const openShortcut = (url, id) => act(() => browserGo(url, id))
watch(
  () => current.value && !current.value.url,
  async (blank) => {
    if (!blank) return
    await readWelcome()
    if (newTab.value === 'artifacts' && !shownStep.value) return openShortcut(shortcuts.value[0]?.url, current.value.id)
    await refreshNewTab()
    await nextTick()
    paneRefs.get(current.value?.id)?.selectAddress()
  },
)

// ---------- keys: Ctrl/Cmd T a new tab, W closes the tab in front, L its address ----------
const paneRefs = new Map()
function paneRef(id, el) {
  if (el) paneRefs.set(id, el)
  else paneRefs.delete(id)
}
function onKey(event) {
  const mod = isMac ? event.metaKey : event.ctrlKey
  if (!mod || (props.embedded && !props.visible)) return
  const key = event.key.toLowerCase()
  if (key === 't') event.shiftKey ? addPrivate() : add()
  else if (key === 'w' && current.value) close(current.value.id)
  else if (key === 'l') paneRefs.get(current.value?.id)?.selectAddress()
  else return
  event.preventDefault()
}

onMounted(async () => {
  if (!props.embedded) document.title = 'MemoryCore · Browser'
  boxes = new ResizeObserver(schedule)
  for (const el of pageEls.values()) boxes.observe(el)
  window.addEventListener('resize', schedule)
  window.addEventListener('keydown', onKey)
  stop = await onBrowserState((next) => (tabs.value = next))
  try {
    newTab.value = (await getSettings()).settings?.browserNewTab ?? 'shortcuts'
  } catch {
    // the default shortcuts
  }
  tabs.value = await browserState()
  // inside Castle the room opens on a new tab when nothing is open yet
  if (props.embedded && !tabs.value.list.length) await browserOpen(null).catch(() => {})
  await readTranslating()
  await readWelcome()
  if (current.value && !current.value.url) refreshNewTab()
  schedule()
})
onBeforeUnmount(() => {
  boxes?.disconnect()
  cancelAnimationFrame(frame)
  window.removeEventListener('resize', schedule)
  window.removeEventListener('keydown', onKey)
  if (props.embedded) browserShow([]).catch(() => {})
  stop?.()
})
</script>

<template>
  <div class="browser" :class="{ embedded }">
    <div class="chips" role="tablist" aria-label="Tabs">
      <span v-if="group" class="chip group" :class="{ on: group.on, news: group.unread > 0 }">
        <button class="name" role="tab" :aria-selected="group.on" :title="`Side by side:\n${group.list}`" @click="pickGroup">
          <span class="minimap" aria-hidden="true"><i v-for="id in group.ids" :key="id"></i></span>
          {{ chipTitle(group.label) }}
          <span v-if="chipCount(group.unread)" class="unread" :aria-label="`${group.unread} new`">{{ chipCount(group.unread) }}</span>
        </button>
        <button class="mini" :aria-expanded="groupMenu" aria-label="The pages in this tab" title="The pages in this tab" @click="groupMenu = !groupMenu">▾</button>
      </span>
      <span v-for="t in singles" :key="t.id" class="chip" :class="{ on: shownIds.includes(t.id), danger: t.danger, news: t.unread > 0, private: t.private }">
        <button class="name" role="tab" :aria-selected="t.id === tabs.active" :title="t.private ? `Private - ${t.url || t.title}` : t.url || t.title" @click="pick(t.id)" @auxclick.middle="close(t.id)">
          <svg v-if="t.private" class="mask" width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M2 9c0-1.1.9-2 2-2h16a2 2 0 0 1 2 2v1.5c0 3-2.4 5.5-5.4 5.5-1.8 0-3-1-3.6-2.4-.6 1.4-1.8 2.4-3.6 2.4C4.4 16 2 13.5 2 10.5zm5 1.2a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2zm10 0a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2z" /></svg>
          {{ chipTitle(t.title) }}
          <span v-if="chipCount(t.unread)" class="unread" :aria-label="`${t.unread} new`">{{ chipCount(t.unread) }}</span>
        </button>
        <button class="mini" :aria-label="`Open ${t.title} on the right`" title="Open on the right" @click="beside(t.id)">⫼</button>
        <button class="mini" :aria-label="`Close ${t.title}`" title="Close" @click="close(t.id)">×</button>
      </span>
      <button class="chip plus" aria-label="New tab" :title="`New tab (${isMac ? '⌘' : 'Ctrl+'}T)`" @click="add">+</button>
      <button class="chip private-new" :title="`New private tab (${isMac ? '⌘⇧' : 'Ctrl+Shift+'}T) - keeps nothing`" @click="addPrivate"><svg class="mask" width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M2 9c0-1.1.9-2 2-2h16a2 2 0 0 1 2 2v1.5c0 3-2.4 5.5-5.4 5.5-1.8 0-3-1-3.6-2.4-.6 1.4-1.8 2.4-3.6 2.4C4.4 16 2 13.5 2 10.5zm5 1.2a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2zm10 0a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2z" /></svg> Private</button>
      <button class="chip key" title="Passwords - sealed on this PC" @click="passwordsOpen = !passwordsOpen"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="8" cy="15" r="4" /><path d="M11 12l9-9M17 6l3 3M15 8l2 2" /></svg> Passwords</button>
      <slot name="chips" />
      <button v-if="shownIds.length >= 3" class="phone-view" role="switch" :aria-checked="tabs.phoneView" title="Three or four pages side by side get their phone layout" @click="phoneView">
        Phone view <span class="switch" :class="{ on: tabs.phoneView }" aria-hidden="true"><i></i></span>
      </button>
      <span v-if="hint" class="safe hint" role="status">{{ hint }}</span>
      <span v-else class="safe" title="Web pages here get no Castle permissions: they cannot read your memory, chats or files.">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l8 3v6c0 4.5-3.4 8.4-8 9-4.6-.6-8-4.5-8-9V6z" /></svg>
        Pages here can't reach Castle or your memory
      </span>
    </div>
    <div v-if="groupMenu && group" class="group-list" role="menu" aria-label="The pages in this tab">
      <span class="group-row" v-for="t in group.members" :key="t.id">
        <span class="title">{{ chipTitle(t.title) }}</span>
        <button role="menuitem" @click="takeOut(t.id)">Take out</button>
      </span>
      <button class="close" @click="groupMenu = false">Close</button>
    </div>

    <div ref="panesEl" class="panes" :style="{ gridTemplateColumns: gridColumns }" @pointermove="drag" @pointerup="release" @pointercancel="release">
      <template v-for="(t, i) in shownTabs" :key="t.id">
        <PaneBar
          :ref="(el) => paneRef(t.id, el)"
          :tab="t"
          :focused="t.id === tabs.active"
          :many="many"
          :translating="translating"
          @focus="focusPane"
          @close-column="closeColumn"
          @overlay="overlay"
          @page-el="pageEl"
          @problem="problem = $event"
          @translating="translating = $event"
        >
          <template #tools="{ tab }"><slot name="pane-tools" :tab="tab" /></template>
          <template #below="{ tab }"><slot name="pane-below" :tab="tab" /></template>
          <template #page="{ tab }">
            <DangerScreen v-if="tab.danger" :danger="tab.danger" @back="act(() => browserDangerBack(tab.id))" @open="act(() => browserDangerOpen(tab.id))" />
            <main v-else-if="!tab.url && !many" class="new-tab">
              <section v-if="words" class="welcome" aria-live="polite">
                <span class="count">{{ stepNumber(shownStep) }} of {{ STEPS.length }}</span>
                <h2>{{ words.title }}</h2>
                <p>{{ words.body }}</p>
                <div v-if="(shownStep === 'history' || shownStep === 'bookmarks') && welcome.profiles.length > 1" class="profile">
                  Chrome profile
                  <GlassSelect v-model="profile" :options="welcome.profiles.map((p) => ({ value: p.folder, label: p.name }))" aria-label="Chrome profile" />
                </div>
                <div class="choices">
                  <button class="pill" :disabled="welcomeBusy" @click="answer('skip')">Skip</button>
                  <button v-if="words.guide && !guided" class="pill" :disabled="welcomeBusy" @click="askGuide">{{ words.guide }}</button>
                  <button class="pill primary" :disabled="welcomeBusy" @click="answer('yes')">{{ welcomeBusy ? 'Working...' : words.yes }}</button>
                </div>
              </section>
              <p v-if="welcomeSaid" class="said" role="status">{{ welcomeSaid }}</p>
              <span class="label">NEW TAB</span>
              <div v-if="shortcuts.length" class="grid">
                <button v-for="s in shortcuts" :key="s.url" class="shortcut" @click="openShortcut(s.url, tab.id)">
                  <span class="letter" aria-hidden="true">{{ s.letter }}</span>{{ s.name }}
                </button>
              </div>
              <span class="hint">{{ shortcuts.length ? 'Your Artifacts and the dev servers running now' : 'Type an address or something to search for' }}</span>
              <template v-for="list in [{ name: 'MOST VISITED', tiles: library.visited }, { name: 'BOOKMARKS', tiles: library.marks }]" :key="list.name">
                <template v-if="list.tiles.length">
                  <span class="label">{{ list.name }}</span>
                  <div class="grid">
                    <button v-for="s in list.tiles" :key="s.url" class="shortcut" :title="s.url" @click="openShortcut(s.url, tab.id)">
                      <span class="letter" aria-hidden="true">{{ s.letter }}</span>{{ s.name }}
                    </button>
                  </div>
                </template>
              </template>
            </main>
            <p v-else-if="!tab.url" class="empty">{{ tab.private ? 'A private tab keeps nothing: no history, no sign-in, nothing after it closes.' : 'Type an address or a search above.' }}</p>
          </template>
        </PaneBar>
        <div v-if="i < shownTabs.length - 1" class="grip" role="separator" aria-orientation="vertical" title="Drag to change the widths" @pointerdown="grab($event, i)"></div>
      </template>
    </div>
    <PasswordsSheet v-if="passwordsOpen" @close="passwordsOpen = false" />
    <slot />
    <p v-if="problem" class="problem" role="alert">{{ problem }}</p>
  </div>
</template>

<style scoped>
/* the browser window has no App.vue around it: its own colours, Castle's defaults */
/* inside Castle: a dark glass panel in the room, framed by Castle (the pick browser-place-2) */
.browser.embedded {
  position: absolute;
  inset: 10px 14px 14px;
  border-radius: 14px;
  border: 1px solid rgba(150, 190, 255, 0.24);
  overflow: hidden;
  background: rgba(5, 11, 31, 0.94);
  box-shadow: 0 14px 40px rgba(0, 0, 0, 0.45);
}
.browser.embedded .problem {
  position: absolute;
}
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
/* the pages side by side as one stacked chip, to the pick browser-split-2 */
.tab.group {
  border: 1px solid #b9a2ff;
  box-shadow:
    3px -3px 0 -1px rgba(185, 162, 255, 0.35),
    0 0 10px rgba(185, 162, 255, 0.3);
}
.minimap {
  display: inline-flex;
  gap: 2px;
}
.minimap i {
  width: 5px;
  height: 14px;
  border-radius: 2px;
  border: 1px solid #b9a2ff;
}
.x.beside {
  opacity: 0;
}
.tab:hover .x.beside {
  opacity: 1;
}
.phone-view {
  margin-left: auto;
  align-self: center;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 4px 10px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: rgba(5, 11, 31, 0.85);
  color: var(--text);
  font-size: 12px;
  cursor: pointer;
}
.switch {
  width: 30px;
  height: 16px;
  border-radius: 999px;
  background: rgba(150, 190, 255, 0.25);
  display: inline-flex;
  align-items: center;
  padding: 2px;
  box-sizing: border-box;
}
.switch i {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: #fff;
  transition: transform 0.15s;
}
.switch.on {
  background: #3fcf9a;
}
.switch.on i {
  transform: translateX(14px);
}
.columns {
  flex: 1;
  min-height: 0;
  display: flex;
}
.column {
  min-width: 0;
  display: flex;
}
.column-new {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  border-left: 1px solid rgba(150, 190, 255, 0.12);
  cursor: pointer;
}
/* a site with news: a mint count and a soft gold glow, the twin of CHAT's */
.tab.news {
  box-shadow:
    inset 0 0 0 1px rgba(230, 194, 122, 0.9),
    0 0 10px rgba(230, 194, 122, 0.45);
}
.news-count {
  flex-shrink: 0;
  padding: 0 6px;
  border-radius: 999px;
  background: #7ee0b8;
  color: #0b2a20;
  font-size: 11px;
  font-weight: 700;
  line-height: 16px;
}
/* a tab the guard held back glows red until it leaves the page */
.tab.held {
  color: #ffd6d9;
  background: rgba(200, 10, 20, 0.35);
  box-shadow:
    inset 0 0 0 1px #ff3a46,
    0 0 12px rgba(255, 40, 50, 0.6);
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
.safe.warning {
  max-width: 60%;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-weight: 600;
}
.safe.warning.dangerous {
  color: #ff7a6b;
}
.safe.warning.suspicious {
  color: var(--warn, #e0b060);
}
.safe.hint {
  color: var(--warn);
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
.address {
  position: relative;
}
.suggest-under {
  top: 42px;
  left: 0;
  right: 0;
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
.welcome {
  position: relative;
  width: min(560px, 100%);
  padding: 18px 20px;
  border: 1px solid var(--line);
  border-radius: 14px;
  background: rgba(150, 190, 255, 0.06);
}
.welcome h2 {
  margin: 0 0 8px;
  font-size: 17px;
}
.welcome p {
  margin: 0 0 14px;
  color: var(--text-dim);
  font-size: 13.5px;
  line-height: 1.5;
}
.welcome .count {
  position: absolute;
  top: 14px;
  right: 16px;
  font-size: 12px;
  color: var(--text-dim);
}
.welcome .profile {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-bottom: 12px;
  font-size: 13px;
}
.choices {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  justify-content: flex-end;
}
.icon.starred {
  color: var(--warn);
}
/* the page is its own webview just below the bar: the choice stays inside the bar, never under it */
.fill {
  display: flex;
  gap: 6px;
  align-items: center;
}
.logins {
  display: flex;
  gap: 4px;
  max-width: 320px;
  overflow-x: auto;
}
.logins button {
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--line);
  border-radius: 15px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
  white-space: nowrap;
  cursor: pointer;
}
.logins button:hover {
  background: var(--hover);
}
.pill.primary {
  border-color: var(--accent);
  background: rgba(143, 184, 255, 0.22);
}
.said {
  margin: 0;
  color: var(--good);
  font-size: 13px;
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

/* ---------- the Keep's look: chips, the group's list, panes with their own toolbars ---------- */
.chips {
  flex-shrink: 0;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  padding: 10px 12px 8px;
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 0 4px 0 2px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(20, 16, 50, 0.45);
  font-size: 13px;
  color: var(--text);
}
.chip.on {
  border-color: rgba(168, 140, 255, 0.9);
  background: rgba(168, 140, 255, 0.14);
}
.chip .name {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 230px;
  padding: 6px 8px;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
}
.chip .mini {
  padding: 4px 6px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font-size: 14px;
  cursor: pointer;
}
.chip .mini:hover {
  background: rgba(168, 140, 255, 0.15);
  color: var(--text);
}
.chip.plus {
  padding: 5px 12px;
  cursor: pointer;
  font-size: 15px;
}
.chip .unread {
  min-width: 16px;
  padding: 0 5px;
  border-radius: 999px;
  background: #7ee0b8;
  color: #0b2a20;
  font-size: 11px;
  font-weight: 700;
  line-height: 16px;
  text-align: center;
}
.chip.news {
  box-shadow:
    inset 0 0 0 1px rgba(230, 194, 122, 0.9),
    0 0 10px rgba(230, 194, 122, 0.45);
}
.chip.danger {
  border-color: #ff3a46;
  background: rgba(200, 10, 20, 0.35);
  box-shadow: 0 0 12px rgba(255, 40, 50, 0.6);
}
.chip.private {
  border-color: rgba(168, 120, 255, 0.7);
}
.chip.private-new {
  gap: 6px;
  padding: 6px 12px;
  border-color: rgba(168, 120, 255, 0.7);
  background: rgba(120, 80, 220, 0.3);
  cursor: pointer;
}
.chip.key {
  gap: 6px;
  padding: 6px 12px;
  color: #8ff0cb;
  cursor: pointer;
}
.mask {
  flex-shrink: 0;
  color: #c9b4ff;
}
.chip.group .name {
  gap: 8px;
}
.group-list {
  flex-shrink: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding: 0 12px 8px;
}
.group-row {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 4px 6px 4px 12px;
  border: 1px solid rgba(168, 140, 255, 0.5);
  border-radius: 999px;
  font-size: 12.5px;
}
.group-row button,
.group-list .close {
  padding: 3px 10px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: rgba(5, 11, 31, 0.85);
  color: var(--text);
  font-size: 12px;
  cursor: pointer;
}
.panes {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-rows: 100%;
  padding: 0 10px 10px;
}
.grip {
  cursor: col-resize;
  background: linear-gradient(90deg, transparent 4px, rgba(168, 140, 255, 0.35) 4px, rgba(168, 140, 255, 0.35) 6px, transparent 6px);
}
.grip:hover {
  background: linear-gradient(90deg, transparent 3px, rgba(168, 140, 255, 0.9) 3px, rgba(168, 140, 255, 0.9) 7px, transparent 7px);
}
.empty {
  margin: auto;
  color: var(--text-dim);
  font-size: 13px;
  text-align: center;
  padding: 0 16px;
}
.new-tab {
  overflow-y: auto;
}
</style>
