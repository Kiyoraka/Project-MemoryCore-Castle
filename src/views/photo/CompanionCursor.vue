<script setup>
// PHOTO: the companion's hand on the canvas - a glowing pointer that glides along what it is about to do.
// The parent calls play(points, kind) and awaits it before the change lands.
import { computed, onBeforeUnmount, ref } from 'vue'
import { durationFor, ease, normPoints, pathLength, pointAt } from './cursorPath.js'

const props = defineProps({
  // canvas px -> screen px
  zoom: { type: Number, default: 1 },
  // screen position of canvas pixel 0,0
  offset: { type: Object, default: () => ({ x: 0, y: 0 }) },
  // the companion's name, shown beside the pointer
  name: { type: String, default: 'Your companion' },
})

const FADE_IN = 160
const FADE_OUT = 220

const shown = ref(false)
const fading = ref(false)
const at = ref({ x: 0, y: 0 })
const trail = ref([])
const trailOn = ref(false)

let frame = 0
let timer = 0
let finish = null

const toScreen = (p) => ({
  x: (props.offset?.x || 0) + p.x * (props.zoom || 1),
  y: (props.offset?.y || 0) + p.y * (props.zoom || 1),
})

const screenAt = computed(() => toScreen(at.value))
const trailPoints = computed(() =>
  trail.value
    .map((p) => {
      const s = toScreen(p)
      return `${s.x.toFixed(1)},${s.y.toFixed(1)}`
    })
    .join(' '),
)
const trailWidth = computed(() => Math.max(3, Math.min(18, 8 * (props.zoom || 1))))

function reducedMotion() {
  try {
    return typeof window !== 'undefined' && !!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
  } catch {
    return false
  }
}

const wait = (ms) =>
  new Promise((resolve) => {
    timer = setTimeout(resolve, ms)
  })

// stops a glide in flight and lets its promise go
function stop() {
  if (frame) cancelAnimationFrame(frame)
  if (timer) clearTimeout(timer)
  frame = 0
  timer = 0
  if (finish) {
    const done = finish
    finish = null
    done()
  }
}

function hide() {
  shown.value = false
  fading.value = false
  trail.value = []
  trailOn.value = false
}

// Glides through the points; resolves once she has lifted her hand.
function play(points, kind = 'move') {
  stop()
  const pts = normPoints(points)
  if (!pts.length || reducedMotion()) {
    hide()
    return Promise.resolve()
  }
  return new Promise((resolve) => {
    let token = null
    finish = () => {
      hide()
      resolve()
    }
    token = finish
    const alive = () => finish === token

    const run = async () => {
      at.value = { ...pts[0] }
      trail.value = []
      trailOn.value = kind === 'brush'
      fading.value = true
      shown.value = true
      // one frame so the fade-in starts from nothing
      await new Promise((r) => {
        frame = requestAnimationFrame(() => r())
      })
      if (!alive()) return
      fading.value = false
      await wait(FADE_IN)
      if (!alive()) return

      const length = pathLength(pts) * (props.zoom || 1)
      const ms = durationFor(length)
      const start = performance.now()
      await new Promise((r) => {
        const step = (now) => {
          if (!alive()) return r()
          const t = Math.min(1, (now - start) / ms)
          const p = pointAt(pts, ease(t))
          at.value = p
          if (trailOn.value) trail.value.push(p)
          if (t < 1) frame = requestAnimationFrame(step)
          else r()
        }
        frame = requestAnimationFrame(step)
      })
      if (!alive()) return
      fading.value = true
      await wait(FADE_OUT)
      if (!alive()) return
      finish = null
      hide()
      resolve()
    }
    run()
  })
}

onBeforeUnmount(stop)

defineExpose({ play })
</script>

<template>
  <div v-show="shown" class="companion-cursor" :class="{ fading }" aria-hidden="true">
    <svg v-if="trailOn && trail.length > 1" class="trail">
      <defs>
        <filter id="companion-trail-glow" x="-20%" y="-20%" width="140%" height="140%">
          <feGaussianBlur stdDeviation="3" />
        </filter>
      </defs>
      <polyline :points="trailPoints" :stroke-width="trailWidth" class="trail-glow" filter="url(#companion-trail-glow)" />
      <polyline :points="trailPoints" :stroke-width="Math.max(1.5, trailWidth / 3)" class="trail-line" />
    </svg>
    <div class="hand" :style="{ transform: `translate(${screenAt.x}px, ${screenAt.y}px)` }">
      <svg class="arrow" viewBox="0 0 24 24" width="26" height="26">
        <path d="M3 2.5 20.5 11l-7.4 2.1L9.6 20.6Z" />
      </svg>
      <span class="name">{{ name }}</span>
    </div>
  </div>
</template>

<style scoped>
.companion-cursor {
  position: absolute;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
  z-index: 20;
  opacity: 1;
  transition: opacity 180ms ease;
}

.companion-cursor.fading {
  opacity: 0;
}

.trail {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: visible;
}

.trail polyline {
  fill: none;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.trail-glow {
  stroke: rgba(167, 139, 250, 0.35);
}

.trail-line {
  stroke: rgba(196, 181, 253, 0.7);
}

.hand {
  position: absolute;
  left: 0;
  top: 0;
  will-change: transform;
}

.arrow {
  display: block;
  /* the tip of the arrow sits on the point */
  margin: -3px 0 0 -3px;
  filter: drop-shadow(0 0 4px rgba(167, 139, 250, 0.95)) drop-shadow(0 0 12px rgba(139, 92, 246, 0.7));
}

.arrow path {
  fill: #a78bfa;
  stroke: #f5f3ff;
  stroke-width: 1.4;
  stroke-linejoin: round;
}

.name {
  position: absolute;
  left: 18px;
  top: 18px;
  padding: 2px 8px;
  border-radius: 999px;
  background: rgba(124, 58, 237, 0.92);
  color: #fff;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.02em;
  white-space: nowrap;
  box-shadow: 0 0 10px rgba(139, 92, 246, 0.6);
}

@media (prefers-reduced-motion: reduce) {
  .companion-cursor {
    transition: none;
  }
}
</style>
