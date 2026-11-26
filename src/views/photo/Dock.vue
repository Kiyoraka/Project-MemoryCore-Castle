<script setup>
import { computed, reactive } from 'vue'

// PHOTO v3's right dock (his pick photo-v3-1): groups of tabbed panels - LAYERS/CHANNELS/PATHS,
// ADJUSTMENTS/STYLES/SWATCHES, ACTIONS/HISTORY/NAVIGATOR (and PROPERTIES) - each group folds to its tabs.
// The open tab and the folded groups are remembered on this machine. A panel the Window menu hides
// leaves its group; a group with no panel left goes away. Each tab's body is the slot named by its id.
const props = defineProps({
  groups: { type: Array, required: true }, // [{ id, grow?, tabs: [{ id, label }] }]
  hidden: { type: Array, default: () => [] }, // panel ids the Window menu turned off
})

const KEY = 'photo.dock'
function load() {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) || '{}')
    return { tabs: raw.tabs && typeof raw.tabs === 'object' ? raw.tabs : {}, folded: raw.folded && typeof raw.folded === 'object' ? raw.folded : {} }
  } catch {
    return { tabs: {}, folded: {} }
  }
}
const state = reactive(load())

function keep() {
  try {
    localStorage.setItem(KEY, JSON.stringify({ tabs: state.tabs, folded: state.folded }))
  } catch {
    // a private window keeps nothing - the dock still works
  }
}

const shown = computed(() =>
  props.groups
    .map((g) => {
      const tabs = g.tabs.filter((t) => !props.hidden.includes(t.id))
      const active = tabs.some((t) => t.id === state.tabs[g.id]) ? state.tabs[g.id] : tabs[0]?.id
      return { ...g, tabs, active, folded: !!state.folded[g.id] }
    })
    .filter((g) => g.tabs.length),
)

function pick(group, id) {
  state.tabs[group.id] = id
  if (state.folded[group.id]) state.folded[group.id] = false
  keep()
}

function fold(group) {
  state.folded[group.id] = !state.folded[group.id]
  keep()
}

// Bring a panel to the front of its group (and unfold the group).
function show(id) {
  const g = props.groups.find((x) => x.tabs.some((t) => t.id === id))
  if (!g) return
  state.tabs[g.id] = id
  state.folded[g.id] = false
  keep()
}

function reset() {
  state.tabs = {}
  state.folded = {}
  keep()
}

const activeOf = (id) => shown.value.find((g) => g.tabs.some((t) => t.id === id))?.active === id

defineExpose({ show, reset, activeOf })
</script>

<template>
  <div class="dock">
    <section v-for="g in shown" :key="g.id" class="group" :class="{ folded: g.folded }" :style="{ flexGrow: g.folded ? 0 : g.grow || 1 }">
      <header>
        <div class="tabs" role="tablist">
          <button
            v-for="t in g.tabs"
            :key="t.id"
            type="button"
            role="tab"
            class="tab"
            :class="{ on: t.id === g.active && !g.folded }"
            :aria-selected="t.id === g.active"
            @click="pick(g, t.id)"
          >
            {{ t.label }}
          </button>
        </div>
        <button type="button" class="fold" :aria-label="g.folded ? 'Open the panel' : 'Fold the panel'" :title="g.folded ? 'Open' : 'Fold'" :aria-expanded="!g.folded" @click="fold(g)">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="g.folded ? 'm6 9 6 6 6-6' : 'm6 15 6-6 6 6'" /></svg>
        </button>
      </header>
      <div v-if="!g.folded" class="body" role="tabpanel">
        <slot :name="g.active" />
      </div>
    </section>
  </div>
</template>

<style scoped>
.dock {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  height: 100%;
}

.group {
  display: flex;
  flex: 1 1 0;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 16px;
  background: var(--panel-background, rgba(24, 19, 66, 0.72));
  backdrop-filter: blur(var(--glass-blur, 8px));
  color: var(--text);
}

.group.folded {
  flex: 0 0 auto;
}

header {
  display: flex;
  flex: none;
  align-items: stretch;
  border-bottom: 1px solid var(--line);
}

.group.folded header {
  border-bottom: 0;
}

.tabs {
  display: flex;
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  scrollbar-width: none;
}

.tab {
  flex: none;
  padding: 9px 13px 8px;
  border: 0;
  border-right: 1px solid var(--line);
  background: none;
  color: var(--text-dim);
  font-size: 12px;
  letter-spacing: 0.14em;
  white-space: nowrap;
}

.tab.on {
  background: color-mix(in srgb, var(--accent, #8a5cf0) 14%, transparent);
  color: var(--text);
  box-shadow: inset 0 -2px 0 var(--accent);
}

.fold {
  display: grid;
  flex: none;
  place-items: center;
  width: 32px;
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-dim);
}

.fold svg {
  width: 16px;
  height: 16px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.body {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

/* the panels inside bring their own glass in v2: inside the dock it is the dock's */
.body > :deep(*) {
  flex: 1;
  min-height: 0;
  border: 0 !important;
  border-radius: 0 !important;
  background: none !important;
  backdrop-filter: none !important;
}

button {
  font: inherit;
  cursor: pointer;
}

button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
</style>
