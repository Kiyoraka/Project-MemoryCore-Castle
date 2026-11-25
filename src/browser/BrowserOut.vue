<script setup>
// A browser tab's window of its own (browser_out.rs): the same live pages the browser showed, moved
// here and never reloaded, so a private tab stays private and every sign-in stays. One glass header:
// a chip for each page (its news, the private mask, the way home), back, forward, reload, the address
// with its suggestions, Back to Castle / Back to the browser, full screen and the x - which closes this
// window's tabs for good. A chip dragged back over the browser goes home too. Every page is still
// guarded: one held back shows the DANGER screen here and its page stays hidden.
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  browserDangerBack,
  browserDangerOpen,
  browserDragEnd,
  browserDragStart,
  browserForgetSearch,
  browserGo,
  browserOutClose,
  browserOutFullscreen,
  browserPopIn,
  browserShow,
  browserState,
  browserStep,
  browserSuggest,
  onBrowserDragEnded,
  onBrowserState,
} from '../api/castle.js'
import { isMac } from '../platform.js'
import DangerScreen from './DangerScreen.vue'
import SuggestList from './SuggestList.vue'
import { backWord, outColumns, outHeader } from './browserout.js'
import { boundsOf, dragPast, moveSelection, siteWarning, suggestionGroups, suggestionInput } from './browserview.js'
import { wearMemberTheme } from './theme.js'

const props = defineProps({ initial: { type: Object, required: true } })
// this window's label, as its tabs name it
const mine = props.initial.window

const list = ref([])
const focusId = ref(props.initial.ids?.[0] ?? null)
const header = computed(() => outHeader(list.value, mine, focusId.value))
const ids = computed(() => header.value?.ids ?? [])
const tabOf = (id) => list.value.find((t) => t.id === id) ?? null
const focused = computed(() => tabOf(header.value?.focus))
const columns = computed(() => outColumns(ids.value.length))
const back = ref(backWord('window'))
const full = ref(false)
const problem = ref('')

async function act(work) {
  problem.value = ''
  try {
    await work()
  } catch (reason) {
    problem.value = String(reason)
  }
}

function setTabs(next) {
  list.value = next?.list ?? []
  if (!ids.value.includes(focusId.value)) focusId.value = ids.value[0] ?? null
}

// ---------- the pages over their boxes ----------
const pageEls = new Map()
let boxes = null
let frame = 0
function schedule() {
  if (frame) return
  frame = requestAnimationFrame(() => {
    frame = 0
    place()
  })
}
/** This window's pages shown at their boxes; one held back, or a new tab, has no page to show, and
 * while the suggestions are open over the pages they all step aside. */
function place() {
  const placed = suggestOpen.value
    ? []
    : ids.value
        .map((id) => tabOf(id))
        .filter((t) => t && t.url && !t.danger)
        .map((t) => {
          const box = boundsOf(pageEls.get(t.id)?.getBoundingClientRect())
          return box ? { id: t.id, ...box } : null
        })
        .filter(Boolean)
  browserShow(placed, mine).catch(() => {})
}
function pageEl(id, el) {
  const before = pageEls.get(id)
  if (before === el) return
  if (before) boxes?.unobserve(before)
  if (el) {
    pageEls.set(id, el)
    boxes?.observe(el)
  } else pageEls.delete(id)
  schedule()
}
watch(list, schedule, { deep: true })

// ---------- the address and its suggestions ----------
const field = ref(null)
const typed = ref('')
const editing = ref(false)
const suggestions = ref([])
const selected = ref(-1)
const order = computed(() => suggestionGroups(suggestions.value).order)
const suggestOpen = computed(() => editing.value && suggestions.value.length > 0)
watch(suggestOpen, schedule)
const secure = computed(() => typed.value.startsWith('https://'))
const syncTyped = () => {
  if (!editing.value) typed.value = header.value?.url ?? ''
}
watch(() => header.value?.url, syncTyped, { immediate: true })
watch(focusId, syncTyped)
let suggestTimer = null
async function askSuggest() {
  const asked = typed.value
  const rows = await browserSuggest(asked).catch(() => [])
  if (asked === typed.value && editing.value) suggestions.value = rows
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
  editing.value = true
  event.target.select()
  if (!typed.value.trim()) askSuggest()
}
function onFieldBlur() {
  editing.value = false
  closeSuggest()
  syncTyped()
}
function onFieldKey(event) {
  if (event.key === 'Escape') {
    closeSuggest()
    field.value?.blur()
    event.preventDefault()
    return
  }
  if (!suggestOpen.value) return
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    selected.value = moveSelection(selected.value, event.key, order.value.length)
    event.preventDefault()
  }
}
function goTo(input) {
  const id = header.value?.focus
  closeSuggest()
  field.value?.blur()
  if (id != null) act(() => browserGo(input, id))
}
function go() {
  const row = selected.value >= 0 ? order.value[selected.value] : null
  goTo(row ? suggestionInput(row) : typed.value)
}
const pickSuggestion = (row) => goTo(suggestionInput(row))
const forgetSuggestion = (row) =>
  act(async () => {
    await browserForgetSearch(row.text)
    await askSuggest()
  })
const step = (name) => header.value && act(() => browserStep(name, header.value.focus))
const warning = computed(() => (focused.value && !focused.value.danger && focused.value.warning?.verdict !== 'safe' ? siteWarning(focused.value.warning) : ''))

// ---------- home, full screen, and the x ----------
/** Back to Castle / Back to the browser: the pages home again (as the group for two to four). */
const popIn = () => act(() => browserPopIn(mine))
/** The x: this window's tabs close for good. */
const closeAll = () => act(() => browserOutClose(mine))
async function toggleFull() {
  await act(async () => {
    full.value = await browserOutFullscreen(mine, !full.value)
  })
  await nextTick()
  schedule()
}
function onKey(event) {
  const mod = isMac ? event.metaKey : event.ctrlKey
  if (event.key === 'F11' || (event.key === 'Escape' && full.value && !editing.value)) {
    event.preventDefault()
    toggleFull()
  } else if (mod && event.key.toLowerCase() === 'l') {
    event.preventDefault()
    field.value?.focus()
    field.value?.select()
  }
}

// ---------- a chip dragged back over the browser ----------
const lifted = ref(false)
let press = null
let wasDrag = false
function chipDown(event) {
  if (event.button !== 0 || !ids.value.length) return
  press = { x: event.clientX, y: event.clientY, lifted: false, started: null }
  try {
    event.currentTarget.setPointerCapture(event.pointerId)
  } catch {
    // the drag still works inside the window
  }
  window.addEventListener('pointermove', chipMove)
  window.addEventListener('pointerup', chipUp)
  window.addEventListener('pointercancel', chipUp)
}
function chipMove(event) {
  if (!press || press.lifted || !dragPast(press, { x: event.clientX, y: event.clientY })) return
  press.lifted = true
  lifted.value = true
  // the window's pages go together
  press.started = browserDragStart(ids.value, mine).then(
    () => true,
    (reason) => {
      problem.value = String(reason)
      return false
    },
  )
}
/** The press over: its listeners gone and the chips back. Returns the press it ended. */
function settleDrag() {
  window.removeEventListener('pointermove', chipMove)
  window.removeEventListener('pointerup', chipUp)
  window.removeEventListener('pointercancel', chipUp)
  const was = press
  press = null
  lifted.value = false
  return was
}
async function chipUp() {
  const was = settleDrag()
  if (!was?.lifted) return
  wasDrag = true
  setTimeout(() => (wasDrag = false), 0)
  // the card is up first, so the drop always finds its drag
  if (await was.started) act(() => browserDragEnd(mine))
}
/** A chip's click (unless it ended a drag): the address row acts on that page. */
function pickChip(id) {
  if (wasDrag) return (wasDrag = false)
  focusId.value = id
}

let stop = null
let stopDragEnded = null
onMounted(async () => {
  boxes = new ResizeObserver(schedule)
  for (const el of pageEls.values()) boxes.observe(el)
  window.addEventListener('resize', schedule)
  window.addEventListener('keydown', onKey)
  stop = await onBrowserState(setTabs)
  // the core made the drop itself (the release went past every window): the chip settles
  stopDragEnded = await onBrowserDragEnded((ended) => ended?.from === mine && settleDrag())
  wearMemberTheme().then((view) => (back.value = backWord(view?.settings?.browserOpens)))
  try {
    setTabs(await browserState())
  } catch {
    // the core tells the first change
  }
  await nextTick()
  schedule()
})
onBeforeUnmount(() => {
  stop?.()
  stopDragEnded?.()
  settleDrag()
  boxes?.disconnect()
  cancelAnimationFrame(frame)
  clearTimeout(suggestTimer)
  window.removeEventListener('resize', schedule)
  window.removeEventListener('keydown', onKey)
})
</script>

<template>
  <div class="out">
    <header class="strip">
      <span
        v-for="c in header?.chips ?? []"
        :key="c.id"
        class="chip"
        :class="{ on: c.id === header.focus, private: c.private, danger: c.danger, news: c.unread > 0 && !c.danger, lifted }"
      >
        <button class="name" :title="`${c.full}\nDrag onto the browser to bring it back`" @pointerdown="chipDown" @click="pickChip(c.id)">
          <svg v-if="c.private" class="mask" width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M2 9c0-1.1.9-2 2-2h16a2 2 0 0 1 2 2v1.5c0 3-2.4 5.5-5.4 5.5-1.8 0-3-1-3.6-2.4-.6 1.4-1.8 2.4-3.6 2.4C4.4 16 2 13.5 2 10.5zm5 1.2a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2zm10 0a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2z" /></svg>
          <svg v-else class="glyph" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="12" cy="12" r="8.5" /><path d="M3.5 12h17M12 3.5c2.6 2.4 3.8 5.2 3.8 8.5s-1.2 6.1-3.8 8.5c-2.6-2.4-3.8-5.2-3.8-8.5S9.4 5.9 12 3.5z" /></svg>
          <span class="title">{{ c.title }}</span>
          <span v-if="c.unread" class="unread" :aria-label="`${c.unread} new`">{{ c.unread > 99 ? '99+' : c.unread }}</span>
        </button>
        <button class="mini" :aria-label="back" :title="back" @click="popIn">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6M20 4l-8 8M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
        </button>
      </span>
      <button class="nav" aria-label="Back" :disabled="!header?.url" @click="step('back')">‹</button>
      <button class="nav" aria-label="Forward" :disabled="!header?.url" @click="step('forward')">›</button>
      <button class="nav" aria-label="Reload" :disabled="!header?.url" @click="step('reload')">↻</button>
      <form class="address" @submit.prevent="go">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" :stroke="secure ? 'var(--good)' : 'var(--warn)'" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" /></svg>
        <input
          ref="field"
          v-model="typed"
          aria-label="Address"
          placeholder="Search or type an address"
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
      <button class="home-pill" :title="`${back}: these pages go home, nothing reloads`" @click="popIn">{{ back }}</button>
      <button class="nav" :aria-label="full ? 'Leave full screen' : 'Full screen'" :title="full ? 'Leave full screen (Esc)' : 'Full screen (F11)'" @click="toggleFull">
        <svg v-if="!full" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" /></svg>
        <svg v-else width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" /></svg>
      </button>
      <button class="nav close" aria-label="Close this window and its tabs" title="Close this window - its tabs close too" @click="closeAll">×</button>
    </header>
    <!-- in the flow under the header: the pages are drawn over anything placed on them -->
    <p v-if="problem" class="problem" role="alert" title="Click to dismiss" @click="problem = ''">{{ problem }}</p>
    <div v-if="warning" class="guard" :class="focused?.warning?.verdict === 'dangerous' ? 'coral' : 'amber'" role="alert"><span class="sign">!</span>{{ warning }}</div>
    <div class="pages" :style="{ gridTemplateColumns: columns }">
      <template v-for="(id, i) in ids" :key="id">
        <div v-if="i > 0" class="gap" aria-hidden="true"></div>
        <div class="col" :class="{ focused: ids.length > 1 && id === header?.focus, private: tabOf(id)?.private }" @pointerdown="focusId = id">
          <div :ref="(el) => pageEl(id, el)" class="page">
            <DangerScreen v-if="tabOf(id)?.danger" :danger="tabOf(id).danger" @back="act(() => browserDangerBack(id))" @open="act(() => browserDangerOpen(id))" />
            <p v-else-if="!tabOf(id)?.url" class="empty">{{ tabOf(id)?.private ? 'A private tab keeps nothing: no history, no sign-in, nothing after it closes.' : 'Type an address or a search above.' }}</p>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
/* every colour is the member's theme (status colours stay their own; the private tint stays violet) */
.out {
  position: fixed;
  inset: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 8px 10px 10px;
  box-sizing: border-box;
  background-color: var(--background);
  background-image: linear-gradient(color-mix(in srgb, var(--background) 60%, transparent), color-mix(in srgb, var(--background) 60%, transparent)), var(--wallpaper, none);
  background-position: var(--wallpaper-position, center);
  background-size: cover;
  color: var(--text);
  font-family: -apple-system, 'Segoe UI', system-ui, sans-serif;
  user-select: none;
}
.strip {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
  border-radius: 14px;
  background: color-mix(in srgb, var(--background) 62%, transparent);
  box-shadow:
    0 8px 26px rgba(0, 0, 0, 0.35),
    inset 0 1px 0 color-mix(in srgb, var(--text) 6%, transparent);
  backdrop-filter: blur(var(--glass-blur, 14px));
}
.chip {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 2px;
  max-width: 300px;
  padding: 0 4px 0 2px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: color-mix(in srgb, var(--background) 45%, transparent);
  font-size: 13px;
}
.chip.on {
  border-color: color-mix(in srgb, var(--accent) 90%, transparent);
  background: color-mix(in srgb, var(--accent) 16%, transparent);
  box-shadow:
    0 0 14px color-mix(in srgb, var(--accent) 45%, transparent),
    inset 0 0 0 1px color-mix(in srgb, var(--accent) 25%, transparent);
}
.chip.news {
  border-color: rgba(230, 194, 122, 0.85);
}
.chip.danger {
  border-color: #ff3a46;
  background: rgba(200, 10, 20, 0.35);
}
.chip.private {
  border-color: rgba(168, 120, 255, 0.85);
  background: rgba(120, 80, 220, 0.3);
}
.chip.lifted {
  border-style: dashed;
  box-shadow: none;
  opacity: 0.55;
}
.chip button {
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
}
.chip .name {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  padding: 6px 8px;
  overflow: hidden;
  color: var(--text);
  white-space: nowrap;
  touch-action: none;
  cursor: grab;
}
.chip .title {
  overflow: hidden;
  text-overflow: ellipsis;
}
.chip .glyph {
  flex-shrink: 0;
  color: var(--accent);
}
.mask {
  flex-shrink: 0;
  color: #c9b4ff;
}
/* the news count: mint, glowing */
.chip .unread {
  min-width: 16px;
  padding: 0 6px;
  border-radius: 999px;
  background: #7ee0b8;
  color: #0b2a20;
  font-size: 11px;
  font-weight: 700;
  line-height: 16px;
  text-align: center;
  box-shadow: 0 0 10px rgba(126, 224, 184, 0.75);
}
.chip .mini {
  display: grid;
  place-items: center;
  padding: 4px 6px;
  border-radius: 6px;
  color: var(--text-dim);
}
.chip .mini:hover {
  color: var(--text);
  background: color-mix(in srgb, var(--accent) 18%, transparent);
}
.nav {
  flex: none;
  width: 32px;
  height: 32px;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 8px;
  background: none;
  color: var(--text-dim);
  font: inherit;
  font-size: 17px;
  cursor: pointer;
}
.nav:hover:not(:disabled) {
  color: var(--text);
  background: color-mix(in srgb, var(--accent) 16%, transparent);
}
.nav:disabled {
  opacity: 0.4;
  cursor: default;
}
.nav.close {
  font-size: 20px;
}
.nav.close:hover {
  background: rgba(230, 90, 110, 0.5);
}
.address {
  position: relative;
  flex: 1;
  min-width: 120px;
  height: 34px;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  padding: 0 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: color-mix(in srgb, var(--background) 80%, transparent);
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
  font-size: 13.5px;
  user-select: text;
}
.suggest-under {
  top: calc(100% + 6px);
  left: 0;
  right: 0;
}
.home-pill {
  flex: none;
  padding: 7px 16px;
  border: 1px solid color-mix(in srgb, var(--accent) 80%, transparent);
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 45%, var(--background));
  color: var(--text);
  font: inherit;
  font-size: 13px;
  white-space: nowrap;
  box-shadow: 0 0 14px color-mix(in srgb, var(--accent) 50%, transparent);
  cursor: pointer;
}
.home-pill:hover {
  box-shadow: 0 0 20px color-mix(in srgb, var(--accent) 75%, transparent);
}
.guard {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 12px;
  border-radius: 8px;
  font-size: 13px;
}
.guard.coral {
  background: rgba(230, 90, 110, 0.85);
  color: #fff;
}
.guard.amber {
  background: rgba(245, 180, 82, 0.85);
  color: #2a1a00;
}
.guard .sign {
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.85);
  color: #a3283c;
  font-weight: 700;
  font-size: 12px;
}
.pages {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-rows: 100%;
}
.col {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: color-mix(in srgb, var(--background) 45%, transparent);
}
.col.focused {
  border-color: color-mix(in srgb, var(--accent) 90%, transparent);
}
.col.private {
  border-color: rgba(168, 120, 255, 0.75);
  background: rgba(70, 40, 140, 0.35);
}
.page {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
}
.empty {
  margin: auto;
  padding: 0 16px;
  color: var(--text-dim);
  font-size: 13px;
  text-align: center;
}
.problem {
  flex-shrink: 0;
  margin: 0;
  padding: 8px 12px;
  border-radius: 8px;
  background: color-mix(in srgb, var(--background) 92%, transparent);
  border: 1px solid var(--bad);
  color: var(--bad);
  font-size: 12px;
  cursor: pointer;
}
</style>
