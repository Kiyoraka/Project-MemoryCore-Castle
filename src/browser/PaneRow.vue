<script setup>
// The thin row above pages side by side (browser_tabs.rs): each column's page by name, the one the
// toolbar acts on outlined, an x that makes it a tab on its own again, and a grip between columns
// that drags their widths (the pages follow when the grip is let go).
import { ref, watch } from 'vue'
import { dragWidths } from './browserview.js'

const props = defineProps({
  panes: { type: Array, required: true },
  widths: { type: Array, required: true },
  active: { type: Number, default: null },
})
const emit = defineEmits(['focus', 'close', 'widths'])
const row = ref(null)
const shares = ref([...props.widths])
watch(
  () => props.widths,
  (next) => (shares.value = [...next]),
)

let dragging = null
function grab(event, at) {
  dragging = { at, x: event.clientX, start: [...shares.value] }
  event.target.setPointerCapture(event.pointerId)
}
function move(event) {
  if (!dragging) return
  shares.value = dragWidths(dragging.start, dragging.at, event.clientX - dragging.x, row.value?.clientWidth ?? 0)
}
function release() {
  if (!dragging) return
  dragging = null
  emit('widths', [...shares.value])
}
</script>

<template>
  <div ref="row" class="panes" role="tablist" aria-label="Pages side by side">
    <template v-for="(t, i) in panes" :key="t.id">
      <div class="pane" :class="{ on: t.id === active }" :style="{ flex: `${shares[i] ?? 1} 1 0` }" role="tab" :aria-selected="t.id === active" :title="`${t.title} - the toolbar acts on this page`" @click="emit('focus', t.id)">
        <span v-if="t.phone" class="phone" title="Shown as on a phone">
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="7" y="2" width="10" height="20" rx="2" /></svg>
        </span>
        <span class="name">{{ t.title }}</span>
        <span v-if="t.unread > 0" class="news">{{ t.unread > 99 ? '99+' : t.unread }}</span>
        <button class="x" :aria-label="`Take ${t.title} out of the side by side`" title="Make this a tab on its own" @click.stop="emit('close', t.id)">
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      </div>
      <div v-if="i < panes.length - 1" class="grip" role="separator" aria-orientation="vertical" title="Drag to change the widths" @pointerdown="grab($event, i)" @pointermove="move" @pointerup="release" @pointercancel="release"></div>
    </template>
  </div>
</template>

<style scoped>
.panes {
  height: 30px;
  flex-shrink: 0;
  display: flex;
  align-items: stretch;
  background: rgba(8, 16, 44, 0.95);
  border-bottom: 1px solid rgba(150, 190, 255, 0.18);
}
.pane {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 6px 0 10px;
  margin: 3px 2px;
  border-radius: 7px;
  border: 1px solid transparent;
  color: var(--text-dim);
  font-size: 12px;
  cursor: pointer;
}
.pane:hover {
  background: var(--hover);
}
.pane.on {
  color: var(--text);
  border-color: #b9a2ff;
  box-shadow: 0 0 10px rgba(185, 162, 255, 0.35);
}
.name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.phone {
  display: grid;
  color: #b9a2ff;
}
.news {
  padding: 0 5px;
  border-radius: 999px;
  background: #7ee0b8;
  color: #0b2a20;
  font-size: 10px;
  font-weight: 700;
  line-height: 14px;
}
.x {
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  border: 0;
  border-radius: 50%;
  background: transparent;
  color: var(--text-dim);
  cursor: pointer;
}
.x:hover {
  background: var(--hover);
  color: var(--text);
}
.grip {
  flex: 0 0 6px;
  cursor: col-resize;
  background: linear-gradient(90deg, transparent 2px, rgba(185, 162, 255, 0.45) 2px, rgba(185, 162, 255, 0.45) 4px, transparent 4px);
}
.grip:hover {
  background: linear-gradient(90deg, transparent 1px, #b9a2ff 1px, #b9a2ff 5px, transparent 5px);
}
</style>
