<script setup>
// PHOTO's eight sliders from -100 to 100 with their numbers. Values go out live while a slider moves
// (final = false) and once more when it is let go (final = true). v2: PROPERTIES shows it `bare`
// (no frame) for an 'adjust' adjustment layer and for the document's own global adjust.
const props = defineProps({
  adjust: { type: Object, default: () => ({}) },
  disabled: { type: Boolean, default: false },
  bare: { type: Boolean, default: false },
  title: { type: String, default: 'ADJUST' },
})
const emit = defineEmits(['update', 'reset'])

const SLIDERS = [
  { key: 'exposure', label: 'Exposure', icon: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Zm0 0v18M15 9h3m-1.5-1.5v3M6 15h3' },
  { key: 'brightness', label: 'Brightness', icon: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Zm0-5v2m0 14v2M3 12h2m14 0h2M5.6 5.6 7 7m10 10 1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4' },
  { key: 'contrast', label: 'Contrast', icon: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Zm0 0v18', fill: 'M12 3a9 9 0 0 0 0 18Z' },
  { key: 'highlights', label: 'Highlights', icon: 'M3 18h18M6 18a6 6 0 0 1 12 0M12 6V4m6.4 4.6 1.4-1.4M5.6 8.6 4.2 7.2' },
  { key: 'shadows', label: 'Shadows', icon: 'M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5Z' },
  { key: 'warmth', label: 'Warmth', icon: 'M12 3a2 2 0 0 0-2 2v8.5a4 4 0 1 0 4 0V5a2 2 0 0 0-2-2Zm0 8v6' },
  { key: 'saturation', label: 'Saturation', icon: 'M12 3s6 6.4 6 11a6 6 0 0 1-12 0c0-4.6 6-11 6-11Z' },
  { key: 'sharpness', label: 'Sharpness', icon: 'M12 3 4 20h16L12 3Zm0 6v7' },
]

const anyChanged = () => SLIDERS.some((s) => props.adjust?.[s.key])
const value = (key) => Math.round(props.adjust?.[key] || 0)
const signed = (n) => (n > 0 ? `+${n}` : `${n}`)
</script>

<template>
  <section class="adjust" :class="{ bare }" :aria-label="title">
    <header>
      <h2>{{ title }}</h2>
      <button type="button" class="reset" :disabled="disabled || !anyChanged()" title="Every slider back to 0" @click="emit('reset')">Reset</button>
    </header>
    <ul>
      <li v-for="s in SLIDERS" :key="s.key" :class="{ moved: value(s.key) }">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path v-if="s.fill" class="fill" :d="s.fill" />
          <path :d="s.icon" />
        </svg>
        <div class="body">
          <div class="top">
            <span>{{ s.label }}</span>
            <span class="num" @dblclick="emit('update', s.key, 0, true)">{{ signed(value(s.key)) }}</span>
          </div>
          <input
            type="range"
            min="-100"
            max="100"
            step="1"
            :value="value(s.key)"
            :disabled="disabled"
            :aria-label="s.label"
            :style="{ '--fill': `${(value(s.key) + 100) / 2}%` }"
            @input="emit('update', s.key, Number($event.target.value), false)"
            @change="emit('update', s.key, Number($event.target.value), true)"
            @dblclick="emit('update', s.key, 0, true)"
          />
        </div>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.adjust {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  padding: 14px 16px 12px;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 16px;
  background: var(--panel-background, rgba(24, 19, 66, 0.72));
  backdrop-filter: blur(var(--glass-blur, 8px));
  color: var(--text);
}

.adjust.bare {
  padding: 0;
  border: 0;
  background: none;
  backdrop-filter: none;
}

.bare h2 {
  color: var(--text-dim);
  font-size: 12px;
  letter-spacing: 0.12em;
}

header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

h2 {
  margin: 0;
  color: var(--accent);
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0.22em;
}

.reset {
  padding: 3px 12px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text-dim);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.reset:disabled {
  opacity: 0.4;
  cursor: default;
}

ul {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin: 0;
  padding: 0 4px 0 0;
  overflow-y: auto;
  list-style: none;
}

li {
  display: flex;
  align-items: center;
  gap: 12px;
}

svg {
  flex: none;
  width: 22px;
  height: 22px;
  fill: none;
  stroke: var(--text-dim);
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

svg .fill {
  fill: var(--text-dim);
  stroke: none;
}

li.moved svg {
  stroke: var(--accent);
}

li.moved svg .fill {
  fill: var(--accent);
}

.body {
  flex: 1;
  min-width: 0;
}

.top {
  display: flex;
  justify-content: space-between;
  font-size: 13.5px;
}

.num {
  color: var(--text-dim);
  font-variant-numeric: tabular-nums;
  cursor: default;
}

input[type='range'] {
  width: 100%;
  height: 18px;
  margin: 0;
  background: none;
  -webkit-appearance: none;
  appearance: none;
}

input[type='range']::-webkit-slider-runnable-track {
  height: 5px;
  border-radius: 3px;
  background: linear-gradient(90deg, var(--accent) var(--fill), color-mix(in srgb, var(--accent) 22%, transparent) var(--fill));
}

input[type='range']::-webkit-slider-thumb {
  width: 16px;
  height: 16px;
  margin-top: -5.5px;
  border: 0;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 8px color-mix(in srgb, var(--accent) 60%, transparent);
  -webkit-appearance: none;
  appearance: none;
}

input[type='range']:disabled {
  opacity: 0.4;
}

input:focus-visible,
button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
