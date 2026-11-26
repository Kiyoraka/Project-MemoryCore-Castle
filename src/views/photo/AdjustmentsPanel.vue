<script setup>
import { ADJUSTMENT_NAMES, ADJUSTMENT_TYPES } from './viewHelpers.js'

// PHOTO v3's ADJUSTMENTS: an icon for every adjustment layer; a click adds it above the chosen layer
// (inside the selection when there is one) and PROPERTIES opens on it.
defineProps({
  disabled: { type: Boolean, default: false },
})
const emit = defineEmits(['add'])

const ICONS = {
  brightnessContrast: 'M12 3v2m0 14v2M3 12h2m14 0h2M5.6 5.6l1.4 1.4m10 10 1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z',
  levels: 'M3 20h18M5 20l3-8 2 4 3-10 3 9 2-3 2 8',
  curves: 'M4 4h16v16H4ZM6 18c4 0 4-12 12-12',
  exposure: 'M4 4h16v16H4ZM4 20 20 4M7 8h4M9 6v4M13 16h4',
  vibrance: 'M12 4 20 19H4L12 4Zm0 5-3.5 7h7L12 9Z',
  hsl: 'M12 3s6 6.4 6 11a6 6 0 0 1-12 0c0-4.6 6-11 6-11Z',
  colorBalance: 'M12 3v18M5 7h14M7 7l-3 7a3 3 0 0 0 6 0L7 7Zm10 0-3 7a3 3 0 0 0 6 0l-3-7Z',
  blackWhite: 'M4 4h16v16H4ZM4 20 20 4',
  photoFilter: 'M4 8h3l2-3h6l2 3h3v11H4V8Zm8 2a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z',
  channelMixer: 'M9 9a4 4 0 1 0 0 8M15 9a4 4 0 1 1 0 8M12 4a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z',
  colorLookup: 'M4 5h16v14H4ZM4 10h16M4 15h16M9 5v14M14 5v14',
  invert: 'M12 4a8 8 0 1 0 0 16V4Z',
  posterize: 'M4 20h4v-5H4ZM10 20h4v-9h-4ZM16 20h4V6h-4Z',
  threshold: 'M4 4h16v16H4ZM12 4v16',
  gradientMap: 'M4 6h16v12H4ZM8 6v12m4-12v12m4-12v12',
  selectiveColor: 'M12 4a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM8 12a4 4 0 1 0 0 8 4 4 0 0 0 0-8Zm8 0a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z',
  shadowsHighlights: 'M12 3a9 9 0 1 0 0 18V3Zm0 4h5m-5 5h7m-7 5h5',
  adjust: 'M4 6h10m4 0h2M4 12h4m4 0h8M4 18h12M16 4v4M10 10v4M18 16v4',
}
// Photoshop's order: light first, then colour, then the special ones
const ORDER = ['brightnessContrast', 'levels', 'curves', 'exposure', 'vibrance', 'hsl', 'colorBalance', 'blackWhite', 'photoFilter', 'channelMixer', 'colorLookup', 'invert', 'posterize', 'threshold', 'gradientMap', 'selectiveColor', 'shadowsHighlights', 'adjust']
const types = [...ORDER.filter((t) => ADJUSTMENT_TYPES.includes(t)), ...ADJUSTMENT_TYPES.filter((t) => !ORDER.includes(t))]
</script>

<template>
  <section class="adjustments" aria-label="Adjustments">
    <p class="dim">Add an adjustment layer</p>
    <div class="grid">
      <button v-for="t in types" :key="t" type="button" class="icon" :disabled="disabled" :title="ADJUSTMENT_NAMES[t]" :aria-label="`New ${ADJUSTMENT_NAMES[t]} layer`" @click="emit('add', t)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="ICONS[t] || ICONS.adjust" /></svg>
      </button>
    </div>
  </section>
</template>

<style scoped>
.adjustments {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  padding: 10px 12px;
  overflow-y: auto;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(44px, 1fr));
  gap: 6px;
}

.icon {
  display: grid;
  place-items: center;
  aspect-ratio: 1;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text);
}

.icon:hover:not(:disabled) {
  border-color: color-mix(in srgb, var(--accent) 60%, transparent);
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  box-shadow: 0 0 12px color-mix(in srgb, var(--accent) 30%, transparent);
}

svg {
  width: 24px;
  height: 24px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.5;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.dim {
  margin: 0;
  color: var(--text-dim);
  font-size: 12px;
}

button {
  cursor: pointer;
}

button:disabled {
  opacity: 0.4;
  cursor: default;
}

button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
