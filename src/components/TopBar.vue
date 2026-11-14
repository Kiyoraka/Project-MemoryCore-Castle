<script setup>
import { onBeforeUnmount, ref } from 'vue'
import TopMusic from './TopMusic.vue'
import TopClock from './TopClock.vue'
import { neighbourOf } from './taborder.js'

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
})
const emit = defineEmits(['select', 'settings', 'hide', 'reorder'])

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
  if (!event.altKey || (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight')) return
  event.preventDefault()
  const to = neighbourOf(props.tabs.map((t) => t.id), id, event.key === 'ArrowLeft' ? -1 : 1)
  if (to) emit('reorder', { from: id, to })
}
onBeforeUnmount(stopListening)
</script>

<template>
  <header class="top-bar">
    <span class="title"><span class="gem">◆</span> MEMORYCORE<span v-if="version" class="version" :title="`MemoryCore Castle ${version}`">v{{ version }}</span></span>
    <nav class="tabs" :class="{ sorting: dragging }">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        class="tab"
        :data-tab="tab.id"
        :class="{ active: tab.id === active, fresh: marks.includes(tab.id), dragging: tab.id === dragging, 'drop-target': dragging && tab.id === over && tab.id !== dragging }"
        :title="marks.includes(tab.id) ? markTitles[tab.id] || 'Something new is waiting' : undefined"
        @pointerdown="onDown($event, tab.id)"
        @click="onClick(tab.id)"
        @keydown="onKey($event, tab.id)"
      >
        {{ tab.label }}
      </button>
    </nav>
    <div class="actions">
      <TopClock v-if="displayClock" :format="clockFormat" />
      <TopMusic />
      <button class="action" title="Settings" aria-label="Settings" @click="$emit('settings')">&#9881;</button>
      <button class="action" title="Hide to tray" aria-label="Hide to tray" @click="$emit('hide')">&#8211;</button>
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

button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
