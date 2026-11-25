<script setup>
// Castle's one dropdown: a glass button with the chosen label and a chevron, and a glass list in
// the theme's colours (a native <select> lets Windows draw a grey list that ignores the theme).
// v-model; options as [{ value, label, disabled }] or plain strings. The list is teleported to the
// page so no card clips it, opens below or above by the room left, and works as a listbox does.
import { computed, nextTick, onBeforeUnmount, ref, watch, useId } from 'vue'
import { edgeEnabled, filterByQuery, labelOf, nextEnabled, normalise } from './glassselect.js'

const props = defineProps({
  modelValue: { type: [String, Number, Boolean, null], default: null },
  options: { type: Array, default: () => [] },
  disabled: { type: Boolean, default: false },
  placeholder: { type: String, default: '' },
  // Opt-in: the trigger becomes a text input that filters the list as it's typed into, for a
  // list long enough that scrolling to find one entry (MARKET's symbol picker) is the wrong tool.
  // Every other GlassSelect keeps its plain click-to-open button, unchanged.
  searchable: { type: Boolean, default: false },
})
const emit = defineEmits(['update:modelValue', 'change', 'open'])
defineOptions({ inheritAttrs: false })

const list = computed(() => normalise(props.options))
const label = computed(() => labelOf(list.value, props.modelValue, props.placeholder))
const id = `glass-${useId()}`
const open = ref(false)
// whether the list is open, for a page that must make room for it (the browser's pages step aside)
watch(open, (now) => emit('open', now))
const active = ref(-1)
const button = ref(null)
const panel = ref(null)
const place = ref({})
const query = ref('')
const filtering = ref(false) // typing a search, before a choice is made or the panel closes
const shown = computed(() => (props.searchable && filtering.value ? filterByQuery(list.value, query.value) : list.value))
watch(label, (l) => { if (!filtering.value) query.value = l }, { immediate: true })

function position() {
  const r = button.value?.getBoundingClientRect()
  if (!r) return
  const room = window.innerHeight - r.bottom
  const tall = Math.min(280, shown.value.length * 34 + 10)
  const above = room < tall + 12 && r.top > room
  place.value = {
    left: `${Math.round(r.left)}px`,
    minWidth: `${Math.round(r.width)}px`,
    ...(above ? { bottom: `${Math.round(window.innerHeight - r.top + 4)}px` } : { top: `${Math.round(r.bottom + 4)}px` }),
  }
}

function onOutside(event) {
  if (!button.value?.contains(event.target) && !panel.value?.contains(event.target)) close()
}
function onScroll(event) {
  if (!panel.value?.contains(event.target)) close()
}

function openPanel() {
  position()
  open.value = true
  document.addEventListener('pointerdown', onOutside, true)
  window.addEventListener('scroll', onScroll, true)
  window.addEventListener('resize', close)
}
async function show() {
  if (props.disabled || open.value) return
  const at = list.value.findIndex((o) => o.value === props.modelValue && !o.disabled)
  active.value = at >= 0 ? at : edgeEnabled(list.value)
  openPanel()
  await nextTick()
  panel.value?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' })
}
function close() {
  open.value = false
  filtering.value = false
  query.value = label.value
  document.removeEventListener('pointerdown', onOutside, true)
  window.removeEventListener('scroll', onScroll, true)
  window.removeEventListener('resize', close)
}
function choose(index) {
  const option = shown.value[index]
  if (!option || option.disabled) return
  if (option.value !== props.modelValue) {
    emit('update:modelValue', option.value)
    emit('change', option.value)
  }
  close()
  button.value?.focus()
}
function onType(event) {
  filtering.value = true
  query.value = event.target.value
  if (!open.value) openPanel()
  active.value = edgeEnabled(shown.value)
}
// Selecting the current text on focus means typing straight away replaces it - no one has to
// notice this is editable and manually clear it first, the way a plain button's label would read.
function onSearchFocus(event) {
  if (!open.value) show()
  event.target.select()
}
function key(event) {
  if (props.disabled) return
  const k = event.key
  if (!open.value) {
    // the search input opens on focus/typing already; only the plain button needs a key to open
    if (!props.searchable && ['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(k)) {
      event.preventDefault()
      show()
    }
    return
  }
  if (k === 'Escape') {
    event.preventDefault()
    close()
  } else if (k === 'ArrowDown' || k === 'ArrowUp') {
    event.preventDefault()
    const next = nextEnabled(shown.value, active.value, k === 'ArrowDown' ? 1 : -1)
    if (next >= 0) active.value = next
  } else if (k === 'Home' || k === 'End') {
    event.preventDefault()
    active.value = edgeEnabled(shown.value, k === 'End')
  } else if (k === 'Enter' || (k === ' ' && !props.searchable)) {
    // space is a normal character while typing a search - only Enter chooses then
    event.preventDefault()
    choose(active.value)
  } else if (k === 'Tab') {
    close()
  }
}
onBeforeUnmount(close)
</script>

<template>
  <span v-if="searchable" class="search-wrap">
    <svg class="search-icon" viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="M21 21l-4.3-4.3" /></svg>
    <input
      ref="button"
      v-bind="$attrs"
      type="text"
      class="glass-select glass-search"
      role="combobox"
      :aria-expanded="open"
      :aria-controls="`${id}-list`"
      aria-haspopup="listbox"
      :aria-activedescendant="open && active >= 0 ? `${id}-${active}` : undefined"
      :disabled="disabled"
      :placeholder="placeholder"
      :value="query"
      autocomplete="off"
      @focus="onSearchFocus"
      @input="onType"
      @keydown="key"
    />
  </span>
  <button
    v-else
    ref="button"
    v-bind="$attrs"
    type="button"
    class="glass-select"
    role="combobox"
    :aria-expanded="open"
    :aria-controls="`${id}-list`"
    aria-haspopup="listbox"
    :aria-activedescendant="open && active >= 0 ? `${id}-${active}` : undefined"
    :disabled="disabled"
    @click="open ? close() : show()"
    @keydown="key"
  >
    <span class="value" :class="{ placeholder: label === placeholder && placeholder }">{{ label }}</span>
    <svg class="chevron" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
  </button>
  <Teleport to="body">
    <ul v-if="open" :id="`${id}-list`" ref="panel" class="glass-list" role="listbox" :style="place">
      <li v-if="searchable && !shown.length" class="empty">No match</li>
      <li
        v-for="(o, i) in shown"
        :id="`${id}-${i}`"
        :key="String(o.value)"
        role="option"
        :aria-selected="o.value === modelValue"
        :aria-disabled="o.disabled || undefined"
        :class="{ active: i === active, chosen: o.value === modelValue, disabled: o.disabled }"
        @pointerenter="!o.disabled && (active = i)"
        @click="choose(i)"
      >
        {{ o.label }}
      </li>
    </ul>
  </Teleport>
</template>

<style scoped>
.glass-select {
  display: inline-flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  min-width: 120px;
  max-width: 100%;
  padding: 7px 10px 7px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 14px;
  letter-spacing: normal;
  text-align: left;
  cursor: pointer;
}
.glass-select:hover:not(:disabled) {
  border-color: color-mix(in srgb, var(--accent) 50%, var(--line));
}
.glass-select:focus-visible {
  outline: 2px solid color-mix(in srgb, var(--accent) 70%, transparent);
  outline-offset: 2px;
}
.glass-select:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.search-wrap {
  position: relative;
  display: inline-flex;
  max-width: 100%;
}
.search-icon {
  position: absolute;
  left: 10px;
  top: 50%;
  transform: translateY(-50%);
  width: 14px;
  height: 14px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2;
  stroke-linecap: round;
  color: var(--text-dim);
  pointer-events: none;
}
.glass-search {
  width: 100%;
  padding-left: 30px;
  cursor: text;
}
.glass-search::placeholder {
  color: var(--text-dim);
}
.value {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.value.placeholder {
  color: var(--text-dim);
}
.chevron {
  flex: none;
  width: 14px;
  height: 14px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
  opacity: 0.8;
}
</style>

<style>
/* the list is teleported to <body>, outside this component's scope */
.glass-list {
  position: fixed;
  z-index: 1000;
  max-height: 280px;
  margin: 0;
  padding: 5px;
  overflow-y: auto;
  list-style: none;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: color-mix(in srgb, var(--panel-background) 92%, #050b1f);
  -webkit-backdrop-filter: blur(var(--glass-blur, 12px)); backdrop-filter: blur(var(--glass-blur, 12px));
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
  color: var(--text);
  font-size: 14px;
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}
.glass-list li {
  padding: 7px 12px;
  border-radius: 7px;
  white-space: nowrap;
  cursor: pointer;
}
.glass-list li.active {
  background: color-mix(in srgb, var(--accent) 22%, transparent);
}
.glass-list li.chosen {
  color: var(--accent);
  font-weight: 600;
}
.glass-list li.disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.glass-list li.empty {
  color: var(--text-dim);
  cursor: default;
}
</style>
