<script setup>
// The card a dragged browser tab shows: a glowing glass card with the page's little picture (or the
// private mask, or the title and site where no picture can be taken), its title and how many pages go
// together, and a pill under it saying where a drop would go now. It lives in a small see-through
// window of its own that never takes a click or the focus; the core moves it with the cursor
// (browser_out.rs) and tells it the drop's state.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { browserGhostThumb, onBrowserGhostState, onBrowserGhostThumb } from '../api/castle.js'
import { isMac } from '../platform.js'
import { ghostLine } from './browserview.js'
import { wearMemberTheme } from './theme.js'

const props = defineProps({ initial: { type: Object, required: true } })
const thumb = ref('')
const state = ref('stay')
const line = computed(() => ghostLine(state.value))
const count = computed(() => Number(props.initial.count) || 1)
let stopState = null
let stopThumb = null

async function readThumb() {
  thumb.value = (await browserGhostThumb().catch(() => null)) ?? thumb.value
}

onMounted(async () => {
  wearMemberTheme()
  stopState = await onBrowserGhostState((s) => (state.value = s))
  // a private page is never pictured: the mask stays
  if (props.initial.private) return
  stopThumb = await onBrowserGhostThumb(readThumb)
  readThumb()
})

onBeforeUnmount(() => {
  stopState?.()
  stopThumb?.()
})
</script>

<template>
  <div class="ghost" :class="{ ready: state !== 'stay', mac: isMac }">
    <div class="card" :class="{ private: initial.private }">
      <img v-if="thumb && !initial.private" class="thumb" :src="thumb" alt="" />
      <div v-else class="thumb blank" aria-hidden="true">
        <svg v-if="initial.private" class="mask" viewBox="0 0 24 24" fill="currentColor"><path d="M2 9c0-1.1.9-2 2-2h16a2 2 0 0 1 2 2v1.5c0 3-2.4 5.5-5.4 5.5-1.8 0-3-1-3.6-2.4-.6 1.4-1.8 2.4-3.6 2.4C4.4 16 2 13.5 2 10.5zm5 1.2a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2zm10 0a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2z" /></svg>
        <template v-else>
          <span class="letter">{{ (initial.site || initial.title || '?').slice(0, 1).toUpperCase() }}</span>
          <span v-if="initial.site" class="site">{{ initial.site }}</span>
        </template>
      </div>
      <span v-if="count > 1" class="count" :aria-label="`${count} pages`">{{ count }}</span>
      <span class="title">{{ initial.title }}</span>
    </div>
    <span v-if="line" class="pill">{{ line }}</span>
  </div>
</template>

<style scoped>
/* every colour is the member's theme; the private tint stays violet */
.ghost {
  display: flex;
  flex-direction: column;
  gap: 10px;
  align-items: flex-start;
  padding: 10px;
  box-sizing: border-box;
  height: 100vh;
  font-family: -apple-system, 'Segoe UI', system-ui, sans-serif;
  pointer-events: none;
  user-select: none;
}
/* macOS draws no see-through window here: the card sits on the theme's ground */
.ghost.mac {
  background: var(--background);
}
.card {
  position: relative;
  width: 220px;
  height: 124px;
  flex-shrink: 0;
  overflow: hidden;
  border: 1.5px solid color-mix(in srgb, var(--accent) 90%, transparent);
  border-radius: 12px;
  background: color-mix(in srgb, var(--background) 72%, transparent);
  box-shadow:
    0 0 16px 3px color-mix(in srgb, var(--accent) 70%, transparent),
    0 0 34px 8px color-mix(in srgb, var(--accent) 30%, transparent);
  opacity: 0.8;
  transform: rotate(-2deg);
  transition:
    opacity 0.15s,
    box-shadow 0.15s;
}
.ghost.ready .card {
  opacity: 0.96;
  box-shadow:
    0 0 20px 5px color-mix(in srgb, var(--accent) 90%, transparent),
    0 0 44px 12px color-mix(in srgb, var(--accent) 45%, transparent);
}
.card.private {
  border-color: rgba(190, 140, 255, 0.95);
}
.thumb {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
  object-position: top left;
}
.thumb.blank {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding-bottom: 22px;
  box-sizing: border-box;
  background: linear-gradient(160deg, color-mix(in srgb, var(--accent) 28%, var(--background)), var(--background));
}
.card.private .thumb.blank {
  background: linear-gradient(160deg, rgba(120, 80, 220, 0.55), rgba(40, 22, 90, 0.9));
}
.mask {
  width: 46px;
  height: 46px;
  color: #c9b4ff;
}
.letter {
  width: 36px;
  height: 36px;
  display: grid;
  place-items: center;
  border-radius: 9px;
  background: color-mix(in srgb, var(--accent) 24%, transparent);
  color: var(--accent);
  font-size: 17px;
  font-weight: 700;
}
.site {
  max-width: 190px;
  overflow: hidden;
  color: var(--text-dim);
  font-size: 12px;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.count {
  position: absolute;
  top: 6px;
  right: 6px;
  min-width: 18px;
  padding: 0 6px;
  border-radius: 999px;
  background: var(--accent);
  color: var(--background);
  font-size: 11.5px;
  font-weight: 700;
  line-height: 18px;
  text-align: center;
}
.title {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  padding: 5px 10px;
  overflow: hidden;
  background: linear-gradient(transparent, color-mix(in srgb, var(--background) 92%, transparent));
  color: var(--text);
  font-size: 12.5px;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.pill {
  max-width: 236px;
  padding: 6px 14px;
  border: 1px solid color-mix(in srgb, var(--accent) 75%, transparent);
  border-radius: 999px;
  background: color-mix(in srgb, var(--background) 90%, transparent);
  color: var(--text);
  font-size: 12.5px;
  white-space: nowrap;
  box-shadow: 0 0 12px color-mix(in srgb, var(--accent) 55%, transparent);
}
</style>
