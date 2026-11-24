<script setup>
// The light over a theme's painted shining thing (the Crystal's great crystal): bright and
// breathing at a full weekly limit, dimmer as it is used, warm and slowly pulsing under a fifth,
// a faint ember when none is left. Sits behind HOME's panel and widgets; never takes a click.
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { cliQuota } from '../api/castle.js'
import { providerLabel } from './providers.js'
import { energyLook, quotaWords, WARM } from '../world3d/energy.js'
import { clipPath, coverRect, glowBoxes, readGlow } from '../themes/quotaglow.js'

const props = defineProps({
  theme: { type: Object, default: null },
  visible: { type: Boolean, default: true },
})

// dev only: ?energy=0.13 tries a level without a CLI
const devEnergy = import.meta.env.DEV ? new URLSearchParams(location.search).get('energy') : null
const quota = ref(devEnergy !== null ? { cli: 'claude', leftPercent: Number(devEnergy) * 100, resetsAt: null } : null)
const layer = ref(null)
const image = ref(null) // the wallpaper's own size
const area = ref(null) // where the wallpaper lands, in this layer's pixels

const glow = computed(() => readGlow(props.theme?.quotaGlow))
const energy = computed(() => (quota.value ? quota.value.leftPercent / 100 : null))
const look = computed(() => energyLook(energy.value))
const boxes = computed(() => glowBoxes(glow.value, area.value))
const shown = computed(() => Boolean(glow.value && quota.value && boxes.value))
const sparkles = computed(() => Math.round(2 + 10 * (energy.value ?? 0)))
const words = computed(() => {
  if (!quota.value) return ''
  return `${providerLabel(quota.value.cli)} · ${quotaWords(quota.value)} this week`
})

const px = (v) => `${v.toFixed(1)}px`
const thingStyle = computed(() => {
  const t = boxes.value.thing
  const l = look.value
  return {
    left: px(t.x),
    top: px(t.y),
    width: px(t.width),
    height: px(t.height),
    '--clip': clipPath(glow.value.outline),
    '--halo': (0.12 + 0.4 * l.factor).toFixed(3),
    '--veil': ((1 - l.factor) * 0.6).toFixed(3),
    '--warm': (l.warm * 0.6).toFixed(3),
    '--warm-colour': WARM,
    '--beat': l.pulse ? '3.2s' : '7s',
  }
})
const shineStyle = computed(() => {
  const s = boxes.value.shine
  const t = boxes.value.thing
  const l = look.value
  return {
    left: px(s.x - t.width),
    top: px(s.y - t.height * 0.09),
    width: px(t.width * 2),
    height: px(t.height * 0.18),
    '--halo': (0.1 + 0.35 * l.factor).toFixed(3),
    '--veil': ((1 - l.factor) * 0.55).toFixed(3),
    '--warm': (l.warm * 0.5).toFixed(3),
    '--warm-colour': WARM,
    '--beat': l.pulse ? '3.2s' : '7s',
  }
})
const chipStyle = computed(() => {
  const s = boxes.value.shine
  const t = boxes.value.thing
  return { left: px(s ? s.x : t.x + t.width / 2), top: px(s ? s.y + t.height * 0.12 : t.y + t.height + 12) }
})
// sparkles drift up around the thing; fixed seeds so they do not jump on every reading
const sparkleSpots = computed(() =>
  Array.from({ length: sparkles.value }, (_, i) => {
    const r = (n) => ((Math.sin((i + 1) * 12.9898 + n * 78.233) * 43758.5453) % 1 + 1) % 1
    return { left: `${(8 + r(1) * 84).toFixed(1)}%`, top: `${(20 + r(2) * 75).toFixed(1)}%`, animationDelay: `${(r(3) * 6).toFixed(2)}s`, animationDuration: `${(5 + r(4) * 4).toFixed(2)}s` }
  }),
)

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
  const url = glow.value && props.theme?.wallpaperUrl
  if (!url) return
  const img = new Image()
  img.onload = () => {
    if (props.theme?.wallpaperUrl !== url) return
    image.value = { width: img.naturalWidth, height: img.naturalHeight }
    measure()
  }
  img.src = url
}

let timer = 0
async function readQuota() {
  if (devEnergy !== null || !glow.value) return
  try {
    quota.value = await cliQuota()
  } catch {
    quota.value = null
  }
}

let observer
onMounted(() => {
  loadImage()
  readQuota()
  // the limit moves slowly: read it every five minutes (the core keeps its own short cache)
  timer = setInterval(() => props.visible && readQuota(), 5 * 60 * 1000)
  observer = new ResizeObserver(measure)
  const castle = layer.value?.closest('.castle')
  if (castle) observer.observe(castle)
})
onBeforeUnmount(() => {
  clearInterval(timer)
  observer?.disconnect()
})
watch(() => [props.theme?.id, props.theme?.wallpaperUrl], () => {
  loadImage()
  readQuota()
})
watch(() => props.visible, (now) => {
  if (now) {
    measure()
    readQuota()
  }
})
</script>

<template>
  <div ref="layer" class="quota-glow" aria-hidden="true">
    <template v-if="shown">
      <div class="thing" :class="{ low: look.pulse }" :style="thingStyle">
        <div class="halo"></div>
        <div class="veil"></div>
        <div class="warm"></div>
        <span v-for="(s, i) in sparkleSpots" :key="i" class="sparkle" :class="{ ember: look.warm > 0 }" :style="s"></span>
      </div>
      <div v-if="boxes.shine" class="shine" :class="{ low: look.pulse }" :style="shineStyle">
        <div class="halo"></div>
        <div class="veil"></div>
        <div class="warm"></div>
      </div>
      <p class="quota-words" :class="{ low: look.warm > 0 }" :style="chipStyle">{{ words }}</p>
    </template>
  </div>
</template>

<style scoped>
.quota-glow {
  position: absolute;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
}

.thing,
.shine {
  position: absolute;
}

.halo,
.veil,
.warm {
  position: absolute;
  inset: 0;
}

/* a soft light around the thing, breathing */
.thing .halo {
  inset: -22% -40%;
  background: radial-gradient(ellipse at 50% 50%, rgba(214, 232, 255, 0.95) 0%, rgba(190, 214, 255, 0.45) 32%, rgba(170, 200, 255, 0) 68%);
  mix-blend-mode: screen;
  opacity: var(--halo);
  animation: breathe var(--beat) ease-in-out infinite;
}

/* a shadow inside the outline as the limit is used */
.thing .veil,
.thing .warm {
  clip-path: var(--clip);
  filter: blur(4px);
}
.thing .veil {
  background: radial-gradient(ellipse at 50% 45%, rgba(14, 18, 46, 0.7), rgba(8, 12, 34, 1));
  mix-blend-mode: multiply;
  opacity: var(--veil);
}
.thing .warm {
  background: var(--warm-colour);
  mix-blend-mode: overlay;
  opacity: var(--warm);
}
.low .warm {
  animation: ember var(--beat) ease-in-out infinite;
}

/* the light it throws on the water */
.shine .halo {
  background: radial-gradient(ellipse at 50% 50%, rgba(235, 242, 255, 0.95) 0%, rgba(200, 220, 255, 0.4) 30%, rgba(200, 220, 255, 0) 70%);
  mix-blend-mode: screen;
  opacity: var(--halo);
  animation: breathe var(--beat) ease-in-out infinite;
}
.shine .veil {
  inset: -30% -10%;
  background: radial-gradient(ellipse at 50% 50%, rgba(8, 12, 34, 1) 0%, rgba(8, 12, 34, 0.6) 40%, rgba(8, 12, 34, 0) 72%);
  mix-blend-mode: multiply;
  opacity: var(--veil);
}
.shine .warm {
  background: radial-gradient(ellipse at 50% 50%, var(--warm-colour) 0%, rgba(255, 176, 112, 0) 65%);
  mix-blend-mode: overlay;
  opacity: var(--warm);
}

.sparkle {
  position: absolute;
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: #f4f8ff;
  box-shadow: 0 0 6px 2px rgba(200, 225, 255, 0.85);
  opacity: 0;
  animation: rise 7s ease-in-out infinite;
}
.sparkle.ember {
  background: #ffe2c4;
  box-shadow: 0 0 6px 2px rgba(255, 176, 112, 0.85);
}

.quota-words {
  position: absolute;
  margin: 0;
  transform: translateX(-50%);
  padding: 3px 12px;
  border-radius: 999px;
  font-size: 12px;
  letter-spacing: 0.04em;
  white-space: nowrap;
  color: var(--text);
  background: rgba(10, 18, 48, 0.32);
  border: 1px solid var(--line);
  -webkit-backdrop-filter: blur(6px); backdrop-filter: blur(6px);
  text-shadow: 0 1px 3px rgba(0, 0, 0, 0.5);
}
.quota-words.low {
  border-color: rgba(255, 176, 112, 0.6);
}

@keyframes breathe {
  0%,
  100% {
    transform: scale(1);
    filter: brightness(1);
  }
  50% {
    transform: scale(1.04);
    filter: brightness(1.25);
  }
}
@keyframes ember {
  0%,
  100% {
    opacity: var(--warm);
  }
  50% {
    opacity: calc(var(--warm) * 0.45);
  }
}
@keyframes rise {
  0% {
    opacity: 0;
    transform: translateY(0) scale(0.6);
  }
  25% {
    opacity: 0.9;
  }
  100% {
    opacity: 0;
    transform: translateY(-60px) scale(1);
  }
}

@media (prefers-reduced-motion: reduce) {
  .halo,
  .warm,
  .sparkle {
    animation: none !important;
  }
}
</style>
