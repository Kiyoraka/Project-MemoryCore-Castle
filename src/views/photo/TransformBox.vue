<script setup>
import { computed } from 'vue'
import { transformCorners } from './viewHelpers.js'

// PHOTO's free-transform box, drawn in document units inside the canvas overlay: the outline, four
// corner handles (drag to scale, Shift keeps the proportions), the centre, and a hint that a drag
// outside turns it. The canvas does the pointer work (transformHit in viewHelpers.js).
const props = defineProps({
  t: { type: Object, required: true },
  px: { type: Number, default: 1 }, // one screen pixel in document units
})

const corners = computed(() => transformCorners(props.t))
const outline = computed(() => `M${corners.value.map((p) => `${p.x} ${p.y}`).join('L')}Z`)
const size = computed(() => 9 * props.px)
// the turn handle sits above the top edge's middle
const turn = computed(() => {
  const [a, b] = corners.value
  const mx = (a.x + b.x) / 2
  const my = (a.y + b.y) / 2
  const len = Math.hypot(b.x - a.x, b.y - a.y) || 1
  const nx = (b.y - a.y) / len
  const ny = -(b.x - a.x) / len
  const far = 26 * props.px
  return { mx, my, x: mx + nx * far, y: my + ny * far }
})
</script>

<template>
  <g class="transform-box">
    <path class="edge-back" :d="outline" />
    <path class="edge" :d="outline" />
    <line class="stem" :x1="turn.mx" :y1="turn.my" :x2="turn.x" :y2="turn.y" />
    <circle class="turn" :cx="turn.x" :cy="turn.y" :r="5 * px" />
    <rect v-for="(c, i) in corners" :key="i" class="handle" :x="c.x - size / 2" :y="c.y - size / 2" :width="size" :height="size" />
    <path class="centre" :d="`M${t.cx - 6 * px} ${t.cy}h${12 * px}M${t.cx} ${t.cy - 6 * px}v${12 * px}`" />
  </g>
</template>

<style scoped>
.edge-back,
.edge,
.stem,
.centre {
  fill: none;
  vector-effect: non-scaling-stroke;
}

.edge-back {
  stroke: rgba(0, 0, 0, 0.6);
  stroke-width: 3;
}

.edge {
  stroke: var(--accent, #a98bff);
  stroke-width: 1.5;
}

.stem {
  stroke: var(--accent, #a98bff);
  stroke-width: 1;
  stroke-dasharray: 3 3;
}

.turn {
  fill: var(--accent, #a98bff);
  stroke: #fff;
  stroke-width: 1;
  vector-effect: non-scaling-stroke;
}

.handle {
  fill: #fff;
  stroke: var(--accent, #a98bff);
  stroke-width: 1.5;
  vector-effect: non-scaling-stroke;
}

.centre {
  stroke: #fff;
  stroke-width: 1;
  filter: drop-shadow(0 0 1px #000);
}
</style>
