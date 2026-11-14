<script setup>
// HOME's picture fills with the week: the theme's low picture (its wallpaper at 0% used) lies over
// the CSS wallpaper, and the wallpaper itself is revealed above it as the member's weekly limit is
// used - a rise along the theme's angle, or a fade. Nothing is drawn by code: two pictures, one
// mask or one opacity, and one gentle breath that rests while HOME or the window is hidden.
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { cliQuota } from '../api/castle.js'
import { coverRect } from '../themes/quotaglow.js'
import { readReveal, revealEdge, REVEAL_SOFT } from '../themes/usagereveal.js'

const props = defineProps({
  theme: { type: Object, default: null },
  visible: { type: Boolean, default: true },
})

// the same dev override QuotaGlow reads: ?energy=0..1 is the week LEFT
const devEnergy = import.meta.env.DEV ? new URLSearchParams(location.search).get('energy') : null
const quota = ref(devEnergy !== null ? { cli: 'claude', leftPercent: Number(devEnergy) * 100 } : null)
const layer = ref(null)
const image = ref(null)
const area = ref(null)
const awake = ref(document.visibilityState === 'visible')

const reveal = computed(() => readReveal(props.theme?.usageReveal))
const used = computed(() => (quota.value ? 100 - Number(quota.value.leftPercent ?? 100) : null))
const shown = computed(() => Boolean(reveal.value && area.value && used.value !== null && props.theme?.wallpaperUrl))

const px = (v) => `${v.toFixed(1)}px`
const boxStyle = computed(() => {
  const a = area.value
  return { left: px(a.x), top: px(a.y), width: px(a.width), height: px(a.height) }
})
const lowStyle = computed(() => ({ backgroundImage: `url("${reveal.value.lowUrl}")` }))
const litStyle = computed(() => {
  const r = reveal.value
  if (r.mode === 'fade') return { opacity: Math.min(1, Math.max(0, used.value / 100)) }
  const edge = revealEdge(used.value, r) * 100
  const mask = `linear-gradient(${r.angle}, #000 calc(var(--edge) - ${REVEAL_SOFT * 100}%), transparent var(--edge))`
  return { '--edge': `${edge.toFixed(2)}%`, maskImage: mask, WebkitMaskImage: mask }
})
const wallStyle = computed(() => ({ backgroundImage: `url("${props.theme.wallpaperUrl}")` }))

function measure() {
  const el = layer.value
  const castle = el?.closest('.castle')
  if (!el || !castle || !image.value) return (area.value = null)
  const c = castle.getBoundingClientRect()
  const l = el.getBoundingClientRect()
  const wall = coverRect({ width: c.width, height: c.height }, image.value, props.theme?.wallpaperPosition)
  area.value = wall && { ...wall, x: wall.x + c.left - l.left, y: wall.y + c.top - l.top }
}

function loadImage() {
  image.value = null
  const url = reveal.value && props.theme?.wallpaperUrl
  if (!url) return
  const img = new Image()
  img.onload = () => {
    if (props.theme?.wallpaperUrl !== url) return
    image.value = { width: img.naturalWidth, height: img.naturalHeight }
    measure()
  }
  img.src = url
}

async function readQuota() {
  if (devEnergy !== null || !reveal.value) return
  try {
    quota.value = await cliQuota()
  } catch {
    quota.value = null
  }
}

const onVisibility = () => (awake.value = document.visibilityState === 'visible')
let timer = 0
let observer
onMounted(() => {
  loadImage()
  readQuota()
  timer = setInterval(() => props.visible && awake.value && readQuota(), 5 * 60 * 1000)
  observer = new ResizeObserver(measure)
  const castle = layer.value?.closest('.castle')
  if (castle) observer.observe(castle)
  document.addEventListener('visibilitychange', onVisibility)
})
onBeforeUnmount(() => {
  clearInterval(timer)
  observer?.disconnect()
  document.removeEventListener('visibilitychange', onVisibility)
})
watch(() => [props.theme?.id, props.theme?.wallpaperUrl], () => {
  loadImage()
  readQuota()
})
watch(() => props.visible, (now) => now && readQuota())
</script>

<template>
  <div ref="layer" class="usage-reveal" aria-hidden="true">
    <div v-if="shown" class="box" :style="boxStyle">
      <div class="low" :style="lowStyle"></div>
      <div class="lit" :class="reveal.mode" :style="litStyle">
        <div class="wall" :class="{ resting: !visible || !awake }" :style="wallStyle"></div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* the edge glides between two readings instead of jumping */
@property --edge {
  syntax: '<percentage>';
  inherits: false;
  initial-value: 0%;
}
.usage-reveal {
  position: absolute;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
}
.box {
  position: absolute;
}
.low,
.lit,
.wall {
  position: absolute;
  inset: 0;
}
.low,
.wall {
  background-size: 100% 100%;
  background-repeat: no-repeat;
}
.lit {
  transition: --edge 2s ease, opacity 2s ease;
}
/* the lit part breathes, gently */
.wall {
  animation: breathe 6s ease-in-out infinite;
}
.wall.resting {
  animation-play-state: paused;
}
@keyframes breathe {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.82;
  }
}
@media (prefers-reduced-motion: reduce) {
  .wall {
    animation: none;
  }
  .lit {
    transition: none;
  }
}
</style>
