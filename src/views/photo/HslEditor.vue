<script setup>
import { computed, ref } from 'vue'
import { HSL_RANGES } from './viewHelpers.js'

// PHOTO's hue/saturation: pick a colour range (or all of them), then hue, saturation and lightness.
const props = defineProps({
  params: { type: Object, default: () => ({ ranges: { all: { hue: 0, sat: 0, light: 0 } } }) },
})
const emit = defineEmits(['update'])

const NAMES = { all: 'Master', reds: 'Reds', yellows: 'Yellows', greens: 'Greens', cyans: 'Cyans', blues: 'Blues', magentas: 'Magentas' }
const SWATCH = { all: 'conic-gradient(red, yellow, lime, cyan, blue, magenta, red)', reds: '#ff4a4a', yellows: '#ffd84a', greens: '#4ade6a', cyans: '#4ae0e8', blues: '#4a7aff', magentas: '#e84ae0' }
const SLIDERS = [
  { key: 'hue', label: 'Hue', min: -180, max: 180 },
  { key: 'sat', label: 'Saturation', min: -100, max: 100 },
  { key: 'light', label: 'Lightness', min: -100, max: 100 },
]

const range = ref('all')
const values = computed(() => props.params?.ranges?.[range.value] || { hue: 0, sat: 0, light: 0 })
const moved = (key) => {
  const r = props.params?.ranges?.[key]
  return !!r && (r.hue || r.sat || r.light)
}

function set(key, value, final) {
  const ranges = { ...(props.params?.ranges || {}), [range.value]: { ...values.value, [key]: Number(value) } }
  emit('update', { ...props.params, ranges }, final)
}

function resetRange() {
  const ranges = { ...(props.params?.ranges || {}) }
  if (range.value === 'all') ranges.all = { hue: 0, sat: 0, light: 0 }
  else delete ranges[range.value]
  emit('update', { ...props.params, ranges }, true)
}
</script>

<template>
  <div class="hsl">
    <div class="ranges" role="radiogroup" aria-label="Colour range">
      <button
        v-for="r in HSL_RANGES"
        :key="r"
        type="button"
        role="radio"
        :aria-checked="range === r"
        :class="{ on: range === r, moved: moved(r) }"
        :title="NAMES[r]"
        @click="range = r"
      >
        <span class="sw" :style="{ background: SWATCH[r] }"></span>{{ NAMES[r] }}
      </button>
    </div>
    <label v-for="s in SLIDERS" :key="s.key" class="row">
      <span class="top">
        <span>{{ s.label }}</span>
        <span class="num">{{ values[s.key] > 0 ? '+' : '' }}{{ values[s.key] }}</span>
      </span>
      <input
        type="range"
        :min="s.min"
        :max="s.max"
        step="1"
        :value="values[s.key]"
        :aria-label="`${NAMES[range]} ${s.label}`"
        :class="s.key"
        @input="set(s.key, $event.target.value, false)"
        @change="set(s.key, $event.target.value, true)"
        @dblclick="set(s.key, 0, true)"
      />
    </label>
    <button type="button" class="reset" @click="resetRange">Reset {{ NAMES[range] }}</button>
  </div>
</template>

<style scoped>
.hsl {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.ranges {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.ranges button {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 3px 8px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text-dim);
  font: inherit;
  font-size: 11.5px;
  cursor: pointer;
}

.ranges button.on {
  border-color: var(--accent);
  color: var(--text);
  background: color-mix(in srgb, var(--accent) 20%, transparent);
}

.ranges button.moved {
  color: var(--text);
}

.sw {
  width: 10px;
  height: 10px;
  border-radius: 50%;
}

.row {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 12.5px;
}

.top {
  display: flex;
  justify-content: space-between;
}

.num {
  color: var(--text-dim);
  font-variant-numeric: tabular-nums;
}

input[type='range'] {
  width: 100%;
  accent-color: var(--accent);
}

input.hue {
  accent-color: #ff9a4a;
}

.reset {
  align-self: flex-end;
  padding: 3px 12px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text-dim);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}
</style>
