<script setup>
import { onBeforeUnmount, onMounted, ref } from 'vue'
import TopMusic from './TopMusic.vue'
import TopClock from './TopClock.vue'
import SentinelShield from './SentinelShield.vue'
import { neighbourOf } from './taborder.js'
import { layoutForKey, layoutHint } from './windowkeys.js'
import { castleWindowLayout, getSettings, onWindowLayoutChanged } from '../api/castle.js'
import { isMac } from '../platform.js'

const moveHint = isMac ? 'Drag to move this tab, or ⌥← / ⌥→' : 'Drag to move this tab, or Alt+← / Alt+→'
const hideLabel = isMac ? 'Hide to menu bar' : 'Hide to tray'

const props = defineProps({
  tabs: { type: Array, required: true },
  active: { type: String, required: true },
  displayClock: { type: Boolean, default: false },
  // Castle's version ("0.2.4"), said after its name
  version: { type: String, default: '' },
  clockFormat: { type: String, default: '24h' },
  // Tabs with something new waiting (the story room speaking, a new chat line): their button changes colour.
  marks: { type: Array, default: () => [] },
  // what is new, per tab, for its tooltip ("2 new · Rin online")
  markTitles: { type: Object, default: () => ({}) },
  // a number beside a tab's name (BROWSER: the news in its tabs), shown when above 0
  counts: { type: Object, default: () => ({}) },
})
const emit = defineEmits(['select', 'settings', 'hide', 'reorder', 'sentinel'])

// How much of the screen Castle takes: full screen in its own Space, the whole visible frame,
// half of it, or a size the member drags.
const LAYOUTS = [
  { id: 'fullscreen', label: 'Full screen' },
  { id: 'full', label: 'Full width' },
  { id: 'left', label: 'Left half' },
  { id: 'right', label: 'Right half' },
  { id: 'free', label: 'Free: drag the edges' },
]
const layoutOpen = ref(false)
const layout = ref('full')
async function toggleLayout() {
  layoutOpen.value = !layoutOpen.value
  if (!layoutOpen.value) return
  try {
    layout.value = (await getSettings()).settings?.windowLayout || 'full'
  } catch {
    // outside Castle's window
  }
}
async function chooseLayout(id) {
  layoutOpen.value = false
  try {
    // the command answers with the layout it settled on (a toggle or Esc resolves there)
    layout.value = (await castleWindowLayout(id)) || id
  } catch {
    // the window stays as it was
  }
}
function onLayoutKey(event) {
  const wanted = layoutForKey(event, { mac: isMac, layout: layout.value })
  if (!wanted) return
  event.preventDefault()
  chooseLayout(wanted)
}
async function readLayout() {
  try {
    layout.value = (await getSettings()).settings?.windowLayout || 'full'
  } catch {
    // outside Castle's window
  }
}
let stopLayoutEvents = null
onMounted(async () => {
  // bubble phase: a dialog or the mic that takes Esc first keeps it
  window.addEventListener('keydown', onLayoutKey)
  readLayout()
  try {
    stopLayoutEvents = await onWindowLayoutChanged(readLayout)
  } catch {
    // outside Castle's window
  }
})
function layoutOut(event) {
  if (!event.currentTarget.contains(event.relatedTarget)) layoutOpen.value = false
}

// The member arranges the tabs: press a tab and drag it onto another (pointer events, so it works
// whatever the window does with dropped files), or Alt+Left / Alt+Right on the focused tab.
// `reorder` carries { from, to }: the moved tab and the tab it lands beside.
const DRAG_START = 6 // pixels the pointer moves before a press becomes a drag
const dragging = ref(null)
const over = ref(null)
let press = null
let justDragged = false

function stopListening() {
  window.removeEventListener('pointermove', onMove)
  window.removeEventListener('pointerup', onUp)
  window.removeEventListener('pointercancel', onCancel)
}
function onDown(event, id) {
  if (event.button !== 0) return
  press = { id, x: event.clientX }
  justDragged = false
  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onCancel)
}
function onMove(event) {
  if (!press) return
  if (!dragging.value) {
    if (Math.abs(event.clientX - press.x) < DRAG_START) return
    dragging.value = press.id
  }
  const target = document.elementFromPoint(event.clientX, event.clientY)?.closest?.('[data-tab]')
  over.value = target?.dataset.tab ?? null
}
function onUp() {
  if (dragging.value && over.value && over.value !== dragging.value) emit('reorder', { from: dragging.value, to: over.value })
  justDragged = !!dragging.value
  onCancel()
}
function onCancel() {
  press = null
  dragging.value = null
  over.value = null
  stopListening()
}
function onClick(id) {
  // a drag that ends on the tab it started from still clicks it: that is not a choice
  if (justDragged) {
    justDragged = false
    return
  }
  emit('select', id)
}
function onKey(event, id) {
  if (!event.altKey || event.ctrlKey || (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight')) return
  event.preventDefault()
  const to = neighbourOf(props.tabs.map((t) => t.id), id, event.key === 'ArrowLeft' ? -1 : 1)
  if (to) emit('reorder', { from: id, to })
}
onBeforeUnmount(() => {
  stopListening()
  window.removeEventListener('keydown', onLayoutKey)
  stopLayoutEvents?.()
})
</script>

<template>
  <!-- the bar's empty space moves the window (the free layout); tabs and buttons keep their clicks -->
  <header class="top-bar" data-tauri-drag-region>
    <span class="title" data-tauri-drag-region><span class="gem">◆</span> MEMORYCORE<span v-if="version" class="version" :title="`MemoryCore Castle ${version}`">v{{ version }}</span></span>
    <nav class="tabs" :class="{ sorting: dragging }">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        class="tab"
        :data-tab="tab.id"
        :class="{ active: tab.id === active, fresh: marks.includes(tab.id), dragging: tab.id === dragging, 'drop-target': dragging && tab.id === over && tab.id !== dragging }"
        :title="marks.includes(tab.id) ? markTitles[tab.id] || 'Something new is waiting' : tab.fixed ? tab.title || tab.label : moveHint"
        @pointerdown="tab.fixed || onDown($event, tab.id)"
        @click="onClick(tab.id)"
        @keydown="tab.fixed || onKey($event, tab.id)"
      >
        {{ tab.label }}<span v-if="counts[tab.id] > 0" class="count">{{ counts[tab.id] > 99 ? '99+' : counts[tab.id] }}</span>
      </button>
    </nav>
    <div class="actions">
      <SentinelShield @open="$emit('sentinel')" />
      <TopClock v-if="displayClock" :format="clockFormat" />
      <TopMusic />
      <div class="layout" @focusout="layoutOut" @keydown.esc.prevent="layoutOpen = false">
        <button class="action" title="Window size" aria-label="Window size" :aria-expanded="layoutOpen" @click="toggleLayout">
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true"><rect x="1.5" y="2.5" width="13" height="11" rx="1.5" /><path d="M8 2.5v11" /></svg>
        </button>
        <div v-if="layoutOpen" class="layout-menu" role="menu">
          <button v-for="l in LAYOUTS" :key="l.id" role="menuitemradio" :aria-checked="l.id === layout" :class="{ on: l.id === layout }" @click="chooseLayout(l.id)">
            <span class="check" aria-hidden="true">{{ l.id === layout ? '✓' : '' }}</span><span class="name">{{ l.label }}</span><kbd v-if="layoutHint(l.id, isMac)" class="keys">{{ layoutHint(l.id, isMac) }}</kbd>
          </button>
        </div>
      </div>
      <button class="action" title="Settings" aria-label="Settings" @click="$emit('settings')">&#9881;</button>
      <button class="action" :title="hideLabel" :aria-label="hideLabel" @click="$emit('hide')">&#8211;</button>
    </div>
  </header>
</template>

<style scoped>
.top-bar {
  display: flex;
  align-items: center;
  gap: 40px;
  height: var(--bar-height);
  padding: 0 20px;
  background: var(--bar-background);
  border-bottom: 1px solid var(--line);
  color: var(--text);
}

/* Something new is waiting there: gold-violet, a small dot. */
.tab.fresh {
  position: relative;
  color: #fff8e8;
  background: linear-gradient(135deg, rgba(138, 92, 240, 0.85), rgba(199, 154, 74, 0.85));
  border-radius: 8px;
  box-shadow: 0 0 14px rgba(230, 194, 122, 0.5);
}

.tab .count {
  margin-left: 6px;
  padding: 0 6px;
  border-radius: 999px;
  background: #7ee0b8;
  color: #0b2a20;
  font-size: 10px;
  font-weight: 700;
  line-height: 15px;
}

.tab.fresh::after {
  content: '';
  position: absolute;
  top: 4px;
  right: 4px;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #ffe39a;
  box-shadow: 0 0 6px #ffe39a;
}

.title {
  font-size: 13px;
  letter-spacing: 0.2em;
  white-space: nowrap;
}

.gem {
  margin-right: 6px;
  color: var(--accent);
}

.version {
  margin-left: 8px;
  font-size: 10.5px;
  letter-spacing: 0.08em;
  opacity: 0.55;
}

.tabs {
  display: flex;
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  scrollbar-width: none;
  gap: 8px;
}

.tabs::-webkit-scrollbar {
  display: none;
}

button {
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
}

.tab {
  padding: 8px 14px;
  font-size: 13px;
  letter-spacing: 0.14em;
  color: var(--text-dim);
  border-bottom: 2px solid transparent;
}

.tab:hover {
  color: var(--text);
}

.tab.active {
  color: var(--text);
  border-bottom-color: var(--accent);
}

/* Arranging the tabs: the one being moved fades, the one it will land beside lights up. */
.tabs.sorting,
.tabs.sorting .tab {
  cursor: grabbing;
  user-select: none;
}

.tab.dragging {
  opacity: 0.45;
}

.tab.drop-target {
  color: var(--text);
  background: var(--hover);
  border-radius: 8px;
  box-shadow: 0 0 0 1px var(--accent) inset;
}

.actions {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  gap: 4px;
}

.action {
  width: 32px;
  height: 32px;
  font-size: 16px;
  border-radius: 6px;
  color: var(--text-dim);
}

.action:hover {
  color: var(--text);
  background: var(--hover);
}
.layout {
  position: relative;
}
.layout .action {
  display: flex;
  align-items: center;
  justify-content: center;
}
.layout-menu {
  position: absolute;
  right: 0;
  top: calc(100% + 6px);
  z-index: 50;
  display: flex;
  flex-direction: column;
  min-width: 230px;
  padding: 6px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel-background);
  -webkit-backdrop-filter: blur(var(--glass-blur, 12px)); backdrop-filter: blur(var(--glass-blur, 12px));
}
.layout-menu button {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 34px;
  padding: 0 10px;
  border-radius: 6px;
  text-align: left;
  font-size: 13px;
  color: var(--text);
}
.layout-menu button:hover,
.layout-menu button:focus-visible {
  background: var(--hover);
}
.layout-menu .name {
  flex: 1;
}
.layout-menu .keys {
  font: inherit;
  font-size: 12px;
  color: var(--text-dim);
  white-space: nowrap;
}
.layout-menu .check {
  width: 14px;
  color: var(--accent);
}

button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
