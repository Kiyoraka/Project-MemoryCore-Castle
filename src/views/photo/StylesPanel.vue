<script setup>
import { defaultStyle } from './viewHelpers.js'

// PHOTO v3's STYLES: ready layer styles as tiles; a click puts the style on the chosen layer (its effects
// replaced), the first tile clears them. Each tile shows its look on "Aa".
defineProps({
  disabled: { type: Boolean, default: false },
})
const emit = defineEmits(['apply'])

const on = (key, fields) => ({ ...defaultStyle(key), ...fields, on: true })

const STYLE_PRESETS = [
  { id: 'none', name: 'No style', styles: null, look: { background: 'rgba(255,255,255,0.06)' }, text: {} },
  { id: 'soft-shadow', name: 'Soft shadow', styles: { dropShadow: on('dropShadow', { opacity: 0.55, distance: 10, size: 24, blend: 'multiply' }) }, look: { background: 'linear-gradient(135deg,#8f7cf0,#5b48c9)' }, text: { textShadow: '0 4px 8px rgba(0,0,0,.6)' } },
  { id: 'hard-shadow', name: 'Hard shadow', styles: { dropShadow: on('dropShadow', { opacity: 0.9, distance: 6, size: 0, angle: 135 }) }, look: { background: 'linear-gradient(135deg,#c7c3e8,#8d88b8)' }, text: { textShadow: '3px 3px 0 #000' } },
  { id: 'white-stroke', name: 'White stroke', styles: { stroke: on('stroke', { color: '#ffffff', width: 4 }) }, look: { background: 'linear-gradient(135deg,#59b7ff,#2f6fd8)' }, text: { WebkitTextStroke: '1.5px #fff', color: 'transparent' } },
  { id: 'neon', name: 'Neon glow', styles: { outerGlow: on('outerGlow', { color: '#b98cff', size: 24, opacity: 0.9 }), colorOverlay: on('colorOverlay', { color: '#f2e8ff' }) }, look: { background: 'linear-gradient(135deg,#6a5cff,#3a2c9a)' }, text: { color: '#f2e8ff', textShadow: '0 0 8px #b98cff, 0 0 16px #b98cff' } },
  { id: 'gold', name: 'Gold bevel', styles: { bevel: on('bevel', { depth: 120, size: 6 }), gradientOverlay: on('gradientOverlay', { stops: [{ at: 0, color: '#fff2b0' }, { at: 1, color: '#c8871c' }] }) }, look: { background: 'linear-gradient(135deg,#ffcf6b,#e37b2c)' }, text: { color: '#fff2b0', textShadow: '0 1px 0 #7a4a08' } },
  { id: 'stone', name: 'Stone', styles: { bevel: on('bevel', { depth: 80, size: 4 }), innerShadow: on('innerShadow', { opacity: 0.5, distance: 3, size: 6 }), colorOverlay: on('colorOverlay', { color: '#9a9aa6' }) }, look: { background: 'linear-gradient(135deg,#9a9aa6,#5d5d6b)' }, text: { color: '#d8d8e0', textShadow: '0 -1px 1px #000' } },
  { id: 'violet', name: 'Violet glass', styles: { colorOverlay: on('colorOverlay', { color: '#7b5cf0', opacity: 0.6 }), innerGlow: on('innerGlow', { color: '#ffffff', size: 10, opacity: 0.5 }) }, look: { background: 'linear-gradient(135deg,#7f6af2,#4b3fae)' }, text: { color: '#c9b8ff' } },
  { id: 'pink', name: 'Candy', styles: { gradientOverlay: on('gradientOverlay', { stops: [{ at: 0, color: '#ffb3f1' }, { at: 1, color: '#9b6cff' }] }), dropShadow: on('dropShadow', { opacity: 0.4, distance: 4, size: 10 }) }, look: { background: 'linear-gradient(135deg,#ff9ff0,#a879ff)' }, text: { color: '#fff' } },
  { id: 'dots', name: 'Night', styles: { colorOverlay: on('colorOverlay', { color: '#2b2560' }), stroke: on('stroke', { color: '#8f7cf0', width: 2 }) }, look: { background: 'radial-gradient(circle,#4a3f8c 1px,transparent 1.5px) 0 0/6px 6px,#2b2560' }, text: { color: '#2b2560', WebkitTextStroke: '1px #8f7cf0' } },
]
</script>

<template>
  <section class="styles" aria-label="Styles">
    <p class="dim">Click a style to put it on the chosen layer</p>
    <div class="grid">
      <button v-for="p in STYLE_PRESETS" :key="p.id" type="button" class="tile" :style="p.look" :disabled="disabled" :title="p.name" :aria-label="p.name" @click="emit('apply', p)">
        <span :style="p.text">{{ p.id === 'none' ? '∅' : 'Aa' }}</span>
      </button>
    </div>
  </section>
</template>

<style scoped>
.styles {
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
  gap: 8px;
}

.tile {
  display: grid;
  place-items: center;
  aspect-ratio: 1;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 9px;
  color: #fff;
  font-family: Georgia, 'Times New Roman', serif;
  font-size: 16px;
  font-weight: 700;
}

.tile:hover:not(:disabled) {
  box-shadow: 0 0 0 2px var(--accent);
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
