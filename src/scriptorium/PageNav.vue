<script setup>
// Moving between pages, by the editor's own kind of page (room.js pageNav): Ledger's sheet tabs, Stage's
// strip of slides, and page up / page down with "Page N of M" for Scroll and Folio. The page itself
// sits in the default slot.
import { computed, nextTick, ref, watch } from 'vue'
import { pageLabel, stepPage } from './room.js'

const props = defineProps({
  // { kind: 'sheets' | 'slides' | 'pages', count, tabs?, aspect? }
  nav: { type: Object, required: true },
  page: { type: Number, default: 0 },
  busy: { type: Boolean, default: false },
})
const emit = defineEmits(['page'])

const count = computed(() => props.nav?.count ?? 0)
const go = (delta) => emit('page', stepPage(props.page, delta, count.value))
// a long deck draws its strip as you scroll: only the first slides, then more on demand
const STRIP_STEP = 60
const shown = ref(STRIP_STEP)
const strip = ref(null)
watch(
  () => [props.page, count.value],
  async () => {
    if (props.page >= shown.value) shown.value = Math.ceil((props.page + 1) / STRIP_STEP) * STRIP_STEP
    await nextTick()
    strip.value?.querySelector('.slide.on')?.scrollIntoView?.({ block: 'nearest' })
  },
)
function more(e) {
  const el = e.target
  if (el.scrollTop + el.clientHeight >= el.scrollHeight - 40 && shown.value < count.value) shown.value += STRIP_STEP
}
const slides = computed(() => Array.from({ length: Math.min(count.value, shown.value) }, (_, i) => i))
</script>

<template>
  <div class="nav" :class="nav.kind">
    <ol v-if="nav.kind === 'slides'" ref="strip" class="strip" aria-label="Slides" @scroll.passive="more">
      <li v-for="i in slides" :key="i">
        <button class="slide" :class="{ on: i === page }" :aria-current="i === page ? 'page' : undefined" :aria-label="`Slide ${i + 1}`" :style="{ aspectRatio: nav.aspect }" @click="emit('page', i)">
          <span>{{ i + 1 }}</span>
        </button>
      </li>
    </ol>
    <div v-if="nav.kind === 'sheets' && nav.tabs.length > 1" class="tabs" role="tablist" aria-label="Sheets">
      <button v-for="t in nav.tabs" :key="t.index" role="tab" :aria-selected="t.index === page" :class="{ on: t.index === page }" :title="t.name" @click="emit('page', t.index)">{{ t.name }}</button>
    </div>
    <div v-if="nav.kind === 'pages' && count > 1" class="pager" role="group" aria-label="Pages">
      <button :disabled="busy || page <= 0" title="Previous page (Page Up)" aria-label="Previous page" @click="go(-1)">‹</button>
      <span class="where" aria-live="polite">{{ pageLabel(page, count) }}</span>
      <button :disabled="busy || page >= count - 1" title="Next page (Page Down)" aria-label="Next page" @click="go(1)">›</button>
    </div>
    <div class="page"><slot /></div>
  </div>
</template>

<style scoped>
.nav {
  position: absolute;
  inset: 0;
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  grid-template-columns: minmax(0, 1fr);
}
.nav.slides {
  grid-template-columns: 132px minmax(0, 1fr);
  grid-template-rows: minmax(0, 1fr);
}
.page {
  position: relative;
  min-width: 0;
  min-height: 0;
}
.strip {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin: 0;
  padding: 12px 12px 60px;
  border-right: 1px solid var(--scr-line);
  list-style: none;
  overflow: auto;
}
.slide {
  position: relative;
  display: block;
  width: 100%;
  padding: 0;
  border: 2px solid transparent;
  border-radius: 6px;
  background: #fff;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
  cursor: pointer;
}
.slide.on {
  border-color: var(--scr-accent);
}
.slide span {
  position: absolute;
  left: -2px;
  bottom: -2px;
  padding: 1px 7px;
  border-radius: 0 6px 0 6px;
  background: var(--scr-chip);
  color: var(--scr-text);
  font-size: 11px;
}
.slide.on span {
  background: var(--scr-accent);
  color: #fff;
}
.tabs {
  display: flex;
  gap: 2px;
  padding: 6px 12px 0;
  border-bottom: 1px solid var(--scr-line);
  overflow-x: auto;
}
.tabs button {
  flex: none;
  max-width: 180px;
  padding: 5px 14px;
  border: 1px solid transparent;
  border-bottom: 0;
  border-radius: 8px 8px 0 0;
  background: transparent;
  color: var(--scr-muted);
  font: inherit;
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}
.tabs button:hover {
  color: var(--scr-text);
  background: var(--scr-hover);
}
.tabs button.on {
  border-color: var(--scr-line);
  background: var(--scr-panel);
  color: var(--scr-text);
}
.pager {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 6px;
  padding: 6px 12px;
  border-bottom: 1px solid var(--scr-line);
  color: var(--scr-muted);
  font-size: 13px;
}
.pager button {
  width: 28px;
  height: 26px;
  border: 0;
  border-radius: 8px;
  background: var(--scr-chip);
  color: var(--scr-text);
  font: inherit;
  font-size: 16px;
  line-height: 1;
  cursor: pointer;
}
.pager button:disabled {
  opacity: 0.38;
  cursor: default;
}
.where {
  min-width: 96px;
  text-align: center;
  font-variant-numeric: tabular-nums;
}
</style>
