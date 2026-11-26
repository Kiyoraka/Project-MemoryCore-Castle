<script setup>
// The Translate Lens's overlay: a see-through window the core lays exactly over the game. It never takes
// a click. Each read line gets a violet glass speech bubble with a lavender edge whose tail points at the
// game's own words; a soft lens ring circles the newest line, and a small Worldsmith badge sits in the
// bottom-right corner. The picture's pixels map onto this window (overlay.js); a hide or an error fades
// everything out, and a resize places the bubbles again.
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { onWorldsmithLines } from '../../api/castle.js'
import { LINE_HEIGHT, bubblePlace, fitSize, lensRing, lineCount, newest, scaleBox, shownLines, textWidth } from './overlay.js'

const props = defineProps({ initial: { type: Object, default: () => ({}) } })

// the bubble's padding and its sparkle: the words get what is left of the width
const PAD_X = 18
const PAD_Y = 10
const SPARK = 26
const MAX_LINES = 3

const translation = ref(Array.isArray(props.initial?.lines) ? props.initial : null)
const shown = ref(Boolean(translation.value))
const win = ref({ w: window.innerWidth, h: window.innerHeight })
const measured = ref([])
const els = []
let stop = null

const maxWidth = computed(() => Math.max(120, Math.round(win.value.w * 0.38)))

// each line: its box on this window, its font size and the bubble's estimated size (until measured)
const bubbles = computed(() => {
  const t = translation.value
  if (!t) return []
  return shownLines(t).map((line) => {
    const box = scaleBox(line, t.width, t.height, win.value.w, win.value.h)
    const room = maxWidth.value - 2 * PAD_X - SPARK
    const size = fitSize(line.translated, room, MAX_LINES)
    const rows = Math.min(MAX_LINES, lineCount(line.translated, size, room))
    const w = Math.min(maxWidth.value, Math.ceil(textWidth(line.translated, size)) + 2 * PAD_X + SPARK + 2)
    const h = Math.ceil(rows * size * LINE_HEIGHT) + 2 * PAD_Y + 2
    return { ...line, box, size, w, h }
  })
})

const placed = computed(() =>
  bubbles.value.map((b, i) => {
    const real = measured.value[i]
    const w = real?.w || b.w
    const h = real?.h || b.h
    return { ...b, place: bubblePlace(b.box, w, h, win.value.w, win.value.h) }
  }),
)

const ring = computed(() => {
  const at = newest(bubbles.value.map((b) => b.box))
  return at < 0 ? null : lensRing(bubbles.value[at].box, win.value.w, win.value.h)
})

async function measure() {
  measured.value = []
  await nextTick()
  measured.value = bubbles.value.map((_, i) => {
    const el = els[i]
    return el ? { w: el.offsetWidth, h: el.offsetHeight } : null
  })
}

function heard(payload) {
  if (!payload) return
  if (payload.hidden || payload.error) {
    shown.value = false
    return
  }
  if (Array.isArray(payload.lines)) {
    translation.value = payload
    shown.value = true
  }
}

function resized() {
  win.value = { w: window.innerWidth, h: window.innerHeight }
}

watch(bubbles, measure)

onMounted(async () => {
  window.addEventListener('resize', resized)
  stop = await onWorldsmithLines(heard)
  measure()
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', resized)
  stop?.()
})
</script>

<template>
  <div class="overlay" :class="{ shown }" aria-live="polite">
    <div
      v-if="ring"
      class="lens"
      aria-hidden="true"
      :style="{ left: `${ring.left}px`, top: `${ring.top}px`, width: `${ring.size}px`, height: `${ring.size}px` }"
    >
      <span class="glint"></span>
    </div>

    <div
      v-for="(b, i) in placed"
      :key="`${b.index}-${b.translated}`"
      :ref="(el) => (els[i] = el)"
      class="bubble"
      :class="`tail-${b.place.tail}`"
      :style="{
        left: `${b.place.left}px`,
        top: `${b.place.top}px`,
        maxWidth: `${maxWidth}px`,
        fontSize: `${b.size}px`,
        '--tail-x': `${b.place.tailX}px`,
      }"
    >
      <svg class="spark" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M12 2l2.2 7.8L22 12l-7.8 2.2L12 22l-2.2-7.8L2 12l7.8-2.2z" />
      </svg>
      <span class="words">{{ b.translated }}</span>
      <span class="tail" aria-hidden="true"></span>
    </div>

    <div class="badge">
      <svg viewBox="0 0 48 48" aria-hidden="true">
        <path d="M4 22H38V26Q31 27 30 30V34H35V39H13V34H18V30Q17 27 10 26Z" fill="currentColor" />
        <path d="M24 2L25.8 8.2 32 10 25.8 11.8 24 18 22.2 11.8 16 10 22.2 8.2Z" class="star" />
      </svg>
      <span>Worldsmith</span>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
  user-select: none;
  opacity: 0;
  transition: opacity 0.35s ease;
  font-family: 'Segoe UI', -apple-system, system-ui, 'Yu Gothic UI', 'Noto Sans JP', sans-serif;
  color: #fbf8ff;
}
.overlay.shown {
  opacity: 1;
}

/* a violet frosted-glass bubble with a lavender glowing edge */
.bubble {
  position: absolute;
  box-sizing: border-box;
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 18px;
  width: max-content;
  border-radius: 16px;
  border: 1.5px solid rgba(203, 184, 255, 0.85);
  background: linear-gradient(160deg, rgba(104, 76, 214, 0.72), rgba(58, 36, 150, 0.78));
  backdrop-filter: blur(10px) saturate(1.2);
  box-shadow:
    0 0 14px rgba(178, 150, 255, 0.55),
    inset 0 0 10px rgba(214, 198, 255, 0.25),
    0 6px 18px rgba(10, 4, 40, 0.35);
  line-height: 1.3;
  text-shadow: 0 1px 2px rgba(20, 8, 60, 0.6);
  transition: left 0.2s ease, top 0.2s ease;
}
.words {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.spark {
  flex: none;
  width: 0.9em;
  height: 0.9em;
  margin-top: 0.2em;
  fill: #fff6d8;
  filter: drop-shadow(0 0 4px rgba(255, 240, 200, 0.8));
}
/* the tail: a rotated square clipped to its outer half, so it shares the bubble's edge */
.tail {
  position: absolute;
  left: calc(var(--tail-x) - 8px);
  width: 16px;
  height: 16px;
  box-sizing: border-box;
  border-right: 1.5px solid rgba(203, 184, 255, 0.85);
  border-bottom: 1.5px solid rgba(203, 184, 255, 0.85);
  background: linear-gradient(135deg, rgba(58, 36, 150, 0.78), rgba(58, 36, 150, 0.82));
  clip-path: polygon(100% 0, 100% 100%, 0 100%);
}
.tail-down .tail {
  bottom: -8.5px;
  transform: rotate(45deg);
}
.tail-up .tail {
  top: -8.5px;
  transform: rotate(225deg);
  background: linear-gradient(135deg, rgba(104, 76, 214, 0.72), rgba(104, 76, 214, 0.72));
}

/* the soft lens ring over the newest line */
.lens {
  position: absolute;
  box-sizing: border-box;
  border-radius: 50%;
  border: 2px solid rgba(190, 160, 255, 0.9);
  background: radial-gradient(circle at 50% 50%, rgba(170, 140, 255, 0.04) 55%, rgba(150, 110, 255, 0.18) 100%);
  box-shadow:
    0 0 18px rgba(160, 120, 255, 0.65),
    inset 0 0 16px rgba(170, 140, 255, 0.45);
  animation: breathe 3.2s ease-in-out infinite;
  transition: left 0.25s ease, top 0.25s ease, width 0.25s ease, height 0.25s ease;
}
.glint {
  position: absolute;
  right: 18%;
  bottom: 16%;
  width: 14%;
  height: 8%;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.85);
  filter: blur(1.5px);
  transform: rotate(-35deg);
}
@keyframes breathe {
  0%,
  100% {
    box-shadow: 0 0 16px rgba(160, 120, 255, 0.55), inset 0 0 14px rgba(170, 140, 255, 0.4);
  }
  50% {
    box-shadow: 0 0 26px rgba(170, 130, 255, 0.8), inset 0 0 20px rgba(180, 150, 255, 0.55);
  }
}

/* the small Worldsmith badge in the bottom-right corner */
.badge {
  position: absolute;
  right: 18px;
  bottom: 18px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 14px 7px 10px;
  border-radius: 14px;
  border: 1.5px solid rgba(203, 184, 255, 0.8);
  background: linear-gradient(160deg, rgba(104, 76, 214, 0.7), rgba(58, 36, 150, 0.78));
  backdrop-filter: blur(8px);
  box-shadow: 0 0 12px rgba(178, 150, 255, 0.5);
  font-size: 14px;
  letter-spacing: 0.02em;
}
.badge svg {
  width: 24px;
  height: 24px;
  color: #f4efff;
}
.badge .star {
  fill: #fff6d8;
}
</style>
