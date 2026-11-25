<script setup>
// The BROWSER window's own page (browser.rs, webview `browser-ui`): the tabs and the toolbar,
// exactly BAR (94 px) tall above the web page, or the whole window with the new-tab page when the
// tab in front has no page yet. The web pages themselves are separate webviews below it.
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  browserClose,
  browserDangerBack,
  browserDangerOpen,
  browserGo,
  browserOpen,
  browserOutside,
  browserPutInChat,
  browserState,
  browserStep,
  browserSwitch,
  browserImportBookmarks,
  browserImportHistory,
  browserImportPasswords,
  browserPasswordGuide,
  browserWelcome,
  browserWelcomeAnswer,
  browserBeside,
  browserBookmarkToggle,
  browserPhoneView,
  browserUnpane,
  browserWidths,
  browserChrome,
  browserForgetSearch,
  browserSuggest,
  browserFill,
  browserIsBookmarked,
  browserLogins,
  browserNewTab,
  devServers,
  getSettings,
  onBrowserState,
} from '../api/castle.js'
import { isMac } from '../platform.js'
import GlassSelect from '../components/GlassSelect.vue'
import DangerScreen from './DangerScreen.vue'
import SuggestList from './SuggestList.vue'
import PaneRow from './PaneRow.vue'
import { chipCount, fillLabel, groupOf, libraryTiles, moveSelection, onScreen, singleTabs, suggestionGroups, suggestionInput, newTabShortcuts, shownAddress, signInHint, siteWarning } from './browserview.js'
import { STEPS, doneLine, offeredStep, stepNumber, stepWords } from './browser_welcome.js'

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
const hint = computed(() => signInHint(page.value))

watch(page, (url) => (typed.value = url), { immediate: true })

async function act(work) {
  problem.value = ''
  try {
    await work()
  } catch (reason) {
    problem.value = String(reason)
  }
}

// The address bar's suggestions: asked after a short pause while the member types, the pages
// stepping aside while the list is open over them.
const BAR_PX = 94
const suggestions = ref([])
const selected = ref(-1)
const focused = ref(false)
const suggestOrder = computed(() => suggestionGroups(suggestions.value).order)
const suggestOpen = computed(() => focused.value && suggestions.value.length > 0)
let suggestTimer = null

async function askSuggest() {
  const asked = typed.value
  const rows = await browserSuggest(asked).catch(() => [])
  if (asked === typed.value && focused.value) suggestions.value = rows
}
function onType() {
  clearTimeout(suggestTimer)
  selected.value = -1
  suggestTimer = setTimeout(askSuggest, 120)
}
function closeSuggest() {
  clearTimeout(suggestTimer)
  suggestions.value = []
  selected.value = -1
}
function onFieldFocus(event) {
  focused.value = true
  event.target.select()
  // an empty field (the new tab) offers the newest searches
  if (!typed.value.trim()) askSuggest()
}
function onFieldBlur() {
  focused.value = false
  closeSuggest()
}
function onFieldKey(event) {
  if (!suggestOpen.value) return
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    selected.value = moveSelection(selected.value, event.key, suggestOrder.value.length)
    event.preventDefault()
  } else if (event.key === 'Escape') {
    closeSuggest()
    event.preventDefault()
  }
}
function pickSuggestion(row) {
  closeSuggest()
  field.value?.blur()
  act(() => browserGo(suggestionInput(row)))
}
function forgetSuggestion(row) {
  act(async () => {
    await browserForgetSearch(row.text)
    await askSuggest()
  })
}
// Pages side by side: the group's chip, the row above the columns, and the bar grown to hold it.
const PANE_ROW = 30
const group = computed(() => groupOf(tabs.value))
const singles = computed(() => singleTabs(tabs.value))
const shownIds = computed(() => onScreen(tabs.value))
const split = computed(() => shownIds.value.length >= 2)
const shownTabs = computed(() => shownIds.value.map((id) => tabs.value.list.find((t) => t.id === id)).filter(Boolean))
const barHeight = computed(() => BAR_PX + (split.value ? PANE_ROW : 0))
// the group's chip brings back the page last in front in it
const groupFocus = ref(null)
watch(
  () => tabs.value.active,
  (id) => {
    if (group.value?.ids.includes(id)) groupFocus.value = id
  },
)
const pickGroup = () => pick(group.value.ids.includes(groupFocus.value) ? groupFocus.value : group.value.ids[0])
const beside = (id) => act(() => browserBeside(id))
const unpane = (id) => act(() => browserUnpane(id))
const setWidths = (widths) => act(() => browserWidths(widths))
const phoneView = () => act(() => browserPhoneView(!tabs.value.phoneView))
watch([barHeight, suggestOpen], ([bar, open]) => browserChrome(bar, open).catch(() => {}))

function go() {
  const row = selected.value >= 0 ? suggestOrder.value[selected.value] : null
  if (row) return pickSuggestion(row)
  closeSuggest()
  act(() => browserGo(typed.value))
}
const openShortcut = (url) => act(() => browserGo(url))
const step = (name) => act(() => browserStep(name))
const putInChat = () => act(() => browserPutInChat(page.value))
const outside = () => act(() => browserOutside(page.value))
const pick = (id) => act(() => browserSwitch(id))
const close = (id) => act(() => browserClose(id))
const add = () => act(() => browserOpen(null))
// the DANGER screen's two ways out (browser_guard.rs)
const dangerBack = () => act(() => browserDangerBack(current.value.id))
const dangerOpen = () => act(() => browserDangerOpen(current.value.id))

// The first-open welcome: one step at a time on the new tab, each the member's Yes or Skip.
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

// The new tab's own lists, the star and Fill (browser_library.rs): a password never reaches this page.
const library = ref({ visited: [], marks: [] })
const starred = ref(false)
const logins = ref([])
const choosing = ref(false)

async function refreshNewTab() {
  try {
    servers.value = await devServers()
  } catch {
    servers.value = []
  }
  library.value = libraryTiles(await browserNewTab().catch(() => null))
}

async function readPage(url) {
  choosing.value = false
  if (!url) {
    starred.value = false
    logins.value = []
    return
  }
  starred.value = await browserIsBookmarked(url).catch(() => false)
  logins.value = await browserLogins().catch(() => [])
}
watch(page, readPage)
// a sign-in page loading in the same address: read again once the page has had a moment
watch(() => current.value?.title, () => page.value && browserLogins().then((l) => (logins.value = l)).catch(() => {}))

const toggleStar = () => act(async () => (starred.value = await browserBookmarkToggle(page.value, current.value?.title ?? '')))
function fill(login) {
  choosing.value = false
  act(() => browserFill(login.id))
}
const pressFill = () => (logins.value.length === 1 ? fill(logins.value[0]) : (choosing.value = !choosing.value))

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
    await readWelcome()
    if (newTab.value === 'artifacts' && !shownStep.value) return openShortcut(shortcuts.value[0]?.url)
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
  await readWelcome()
  if (current.value && !current.value.url) refreshNewTab()
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey)
  stop?.()
})
</script>

<template>
  <div class="browser">
    <div class="bar" :style="{ height: `${barHeight}px` }">
      <div class="strip" role="tablist" aria-label="Tabs">
        <div v-if="group" class="tab group" :class="{ on: group.on, news: group.unread > 0 }" role="tab" :aria-selected="group.on" :title="`Side by side:\n${group.list}`" @click="pickGroup">
          <span class="minimap" aria-hidden="true"><i v-for="id in group.ids" :key="id"></i></span>
          <span class="name">{{ group.label }}</span>
          <span v-if="chipCount(group.unread)" class="news-count" :aria-label="`${group.unread} new`">{{ chipCount(group.unread) }}</span>
        </div>
        <div v-for="t in singles" :key="t.id" class="tab" :class="{ on: t.id === tabs.active, held: t.danger, news: t.unread > 0 }" role="tab" :aria-selected="t.id === tabs.active" :title="t.url || t.title" @click="pick(t.id)" @auxclick.middle="close(t.id)">
          <span class="dot" :class="{ local: t.url?.startsWith('http://'), blank: !t.url }" aria-hidden="true"></span>
          <span class="name">{{ t.title }}</span>
          <span v-if="chipCount(t.unread)" class="news-count" :aria-label="`${t.unread} new`">{{ chipCount(t.unread) }}</span>
          <button v-if="page && t.id !== tabs.active && shownIds.length < 4" class="x beside" :aria-label="`Put ${t.title} side by side`" title="Side by side with the page in front" @click.stop="beside(t.id)">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M12 4v16" /></svg>
          </button>
          <button class="x" :aria-label="`Close ${t.title}`" @click.stop="close(t.id)">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
          </button>
        </div>
        <button class="icon add" aria-label="New tab" :title="`New tab (${isMac ? '⌘' : 'Ctrl+'}T)`" @click="add">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
        </button>
        <button v-if="shownIds.length >= 3" class="phone-view" role="switch" :aria-checked="tabs.phoneView" title="Three or four pages side by side get their phone layout" @click="phoneView">
          Phone view <span class="switch" :class="{ on: tabs.phoneView }" aria-hidden="true"><i></i></span>
        </button>
        <span v-if="siteWarning(current?.warning)" class="safe warning" :class="current.warning.verdict" role="alert" :title="siteWarning(current.warning)">{{ siteWarning(current.warning) }}</span>
        <span v-else-if="hint" class="safe hint" role="status">{{ hint }}</span>
        <span v-else class="safe" title="Web pages here get no Castle permissions: they cannot read your memory, chats or files.">
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
          <input
            id="browser-address"
            ref="field"
            v-model="typed"
            placeholder="Type an address or search"
            spellcheck="false"
            autocomplete="off"
            role="combobox"
            :aria-expanded="suggestOpen"
            @focus="onFieldFocus"
            @blur="onFieldBlur"
            @input="onType"
            @keydown="onFieldKey"
          />
          <SuggestList v-if="suggestOpen" class="suggest-under" :rows="suggestions" :selected="selected" @pick="pickSuggestion" @forget="forgetSuggestion" @hover="selected = $event" />
        </form>
        <button class="icon" :disabled="!page" :aria-label="starred ? 'Remove bookmark' : 'Bookmark this page'" :title="starred ? 'Remove bookmark' : 'Bookmark this page'" :class="{ starred }" @click="toggleStar">
          <svg width="17" height="17" viewBox="0 0 24 24" :fill="starred ? 'currentColor' : 'none'" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l2.8 5.8 6.2.9-4.5 4.4 1 6.2L12 17.4 6.5 20.3l1-6.2L3 9.7l6.2-.9z" /></svg>
        </button>
        <div v-if="page && logins.length" class="fill">
          <button class="pill" title="Castle fills this page's sign-in from your vault" @click="pressFill">{{ fillLabel(logins) }}</button>
          <div v-if="choosing" class="logins" role="menu">
            <button v-for="l in logins" :key="l.id" role="menuitem" @click="fill(l)">{{ l.username || '(no username)' }}</button>
          </div>
        </div>
        <button class="pill" :disabled="!page" title="Put this page's address in HOME's chat" @click="putInChat">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z" /></svg>
          Put in chat
        </button>
        <button class="pill" :disabled="!page" @click="outside">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
          Open in your browser
        </button>
      </div>
      <PaneRow v-if="split" :panes="shownTabs" :widths="tabs.widths" :active="tabs.active" @focus="pick" @close="unpane" @widths="setWidths" />
    </div>

    <!-- side by side: a column with no page to show (a new tab, a DANGER screen) is drawn here, under the pages -->
    <div v-if="split" class="columns">
      <div v-for="(t, i) in shownTabs" :key="t.id" class="column" :style="{ flex: `${tabs.widths[i] ?? 1} 1 0` }">
        <DangerScreen v-if="t.danger" :danger="t.danger" @back="act(() => browserDangerBack(t.id))" @open="act(() => browserDangerOpen(t.id))" />
        <div v-else-if="!t.url" class="column-new" @click="pick(t.id)">
          <span class="label">NEW TAB</span>
          <span class="hint">Click here, then type an address above</span>
        </div>
      </div>
    </div>

    <DangerScreen v-if="!split && current?.danger" :danger="current.danger" @back="dangerBack" @open="dangerOpen" />
    <main v-else-if="!split && current && !current.url" class="new-tab">
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
        <button v-for="s in shortcuts" :key="s.url" class="shortcut" @click="openShortcut(s.url)">
          <span class="letter" aria-hidden="true">{{ s.letter }}</span>{{ s.name }}
        </button>
      </div>
      <span class="hint">{{ shortcuts.length ? 'Your Artifacts and the dev servers running now' : 'Type an address or something to search for' }}</span>
      <template v-for="list in [{ name: 'MOST VISITED', tiles: library.visited }, { name: 'BOOKMARKS', tiles: library.marks }]" :key="list.name">
        <template v-if="list.tiles.length">
          <span class="label">{{ list.name }}</span>
          <div class="grid">
            <button v-for="s in list.tiles" :key="s.url" class="shortcut" :title="s.url" @click="openShortcut(s.url)">
              <span class="letter" aria-hidden="true">{{ s.letter }}</span>{{ s.name }}
            </button>
          </div>
        </template>
      </template>
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
</style>
