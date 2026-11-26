<script setup>
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'

// One window for Castle's readers (a plan, a story part, the board's column): 80% of the
// screen, in the theme's own glass, Castle dimmed and softly blurred behind it - never a black
// page. The header stays; only the body scrolls. It holds Esc while open, so Esc never reaches
const props = defineProps({
  tag: { type: String, default: '' },
  title: { type: String, default: '' },
  bodyKey: { type: String, default: '' },
})
const emit = defineEmits(['close'])
const body = ref(null)
const win = ref(null)
let before = null
watch(() => props.bodyKey, () => { if (body.value) body.value.scrollTop = 0 }, { flush: 'post' })

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

// Tab and Shift+Tab stay inside the window while it is open
function trap(event) {
  const items = [...(win.value?.querySelectorAll(FOCUSABLE) ?? [])].filter((el) => el.offsetParent !== null || el === document.activeElement)
  if (!items.length) return
  const first = items[0]
  const last = items[items.length - 1]
  const inside = win.value.contains(document.activeElement)
  if (event.shiftKey && (document.activeElement === first || !inside)) {
    event.preventDefault()
    last.focus()
  } else if (!event.shiftKey && (document.activeElement === last || !inside)) {
    event.preventDefault()
    first.focus()
  }
}

function onKey(event) {
  if (event.key === 'Tab') return trap(event)
  // a field that records a shortcut ([data-takes-keys]) uses Esc itself, to leave without a change
  if (event.key === 'Escape' && !event.target?.closest?.('[data-takes-keys]')) {
    event.preventDefault()
    event.stopImmediatePropagation()
    emit('close')
  }
}

onMounted(() => {
  before = document.activeElement
  window.addEventListener('keydown', onKey, true)
  // the window takes the focus unless its content already put it somewhere inside
  requestAnimationFrame(() => { if (!win.value?.contains(document.activeElement)) win.value?.querySelector(FOCUSABLE)?.focus() })
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey, true)
  before?.focus?.({ preventScroll: true })
})
</script>

<template>
  <Teleport to="body">
    <div class="sheet-backdrop" role="dialog" aria-modal="true" :aria-label="title || tag" @click.self="emit('close')">
      <article ref="win" class="sheet-window">
        <header class="sheet-head">
          <span v-if="tag" class="sheet-tag">{{ tag }}</span>
          <span class="sheet-title">{{ title }}</span>
          <slot name="head" />
          <button class="sheet-close" aria-label="Close" @click="emit('close')">&times;</button>
        </header>
        <div v-if="$slots.navigation" class="sheet-navigation"><slot name="navigation" /></div>
        <div ref="body" class="sheet-body">
          <slot />
        </div>
      </article>
    </div>
  </Teleport>
</template>

<style scoped>
.sheet-backdrop {
  position: fixed;
  inset: 0;
  z-index: 100;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(6, 4, 20, 0.42);
  -webkit-backdrop-filter: blur(3px); backdrop-filter: blur(3px);
}

.sheet-window {
  display: flex;
  flex-direction: column;
  width: 80vw;
  height: 80vh;
  overflow: hidden;
  border: 1px solid var(--line, rgba(200, 180, 255, 0.38));
  border-radius: 16px;
  background: var(--panel-background, rgba(20, 16, 58, 0.62));
  -webkit-backdrop-filter: blur(var(--glass-blur, 12px)) saturate(1.1); backdrop-filter: blur(var(--glass-blur, 12px)) saturate(1.1);
  color: var(--text, #f3efff);
  box-shadow: 0 28px 80px rgba(0, 0, 0, 0.5);
  text-align: left;
}

.sheet-head {
  display: flex;
  flex: none;
  align-items: center;
  gap: 14px;
  padding: 14px 20px;
  border-bottom: 1px solid var(--line, rgba(200, 180, 255, 0.25));
  background: rgba(0, 0, 0, 0.12);
}

.sheet-tag {
  flex: none;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.18em;
  color: var(--accent, #b48cff);
}

.sheet-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: 15px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sheet-close {
  flex: none;
  width: 34px;
  height: 34px;
  border: 0;
  border-radius: 8px;
  background: none;
  color: inherit;
  font-size: 22px;
  cursor: pointer;
}

.sheet-close:hover {
  background: var(--hover, rgba(200, 180, 255, 0.16));
}

.sheet-close:focus-visible {
  outline: 2px solid var(--accent, #b48cff);
}

.sheet-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 20px 30px 30px;
  font-size: 14px;
  line-height: 1.55;
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}

.sheet-navigation {
  flex: none;
  /* no rule of its own: the tabs draw the line under themselves, so the selected tab can be left
     open and sit flush with the content below it */
  padding: 8px 0 0;
}
</style>
