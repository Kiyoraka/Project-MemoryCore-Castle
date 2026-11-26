<script setup>
// PHOTO's tool column: sixteen tools as line icons with their shortcut in the tooltip, then the two
// colour squares (front = primary, back = secondary; X swaps them).
const props = defineProps({
  tool: { type: String, default: 'move' },
  primary: { type: String, default: '#ffffff' },
  secondary: { type: String, default: '#000000' },
})
const emit = defineEmits(['tool', 'colour'])

const TOOLS = [
  { id: 'move', label: 'Move', key: 'V', path: 'M12 3v18M3 12h18M12 3 9.5 5.5M12 3l2.5 2.5M12 21l-2.5-2.5M12 21l2.5-2.5M3 12l2.5-2.5M3 12l2.5 2.5M21 12l-2.5-2.5M21 12l-2.5 2.5' },
  { id: 'rect', label: 'Rectangle select', key: 'M', path: 'M4 7V4h3M10 4h4M17 4h3v3M20 10v4M20 17v3h-3M14 20h-4M7 20H4v-3M4 14v-4' },
  { id: 'lasso', label: 'Lasso', key: 'L', path: 'M7.5 15.4C4.8 14.4 3 12.4 3 10c0-3.9 4-7 9-7s9 3.1 9 7-4 7-9 7c-1.2 0-2.3-.2-3.3-.5M7.5 15.4c-1.2.6-1.6 1.9-.9 2.8.8 1 2.4.9 3-.2.5-1-.1-2.2-2.1-2.6Zm1.3 3.2c-.2 1.2-.9 2.1-2 2.4' },
  { id: 'wand', label: 'Magic wand', key: 'W', path: 'M4 20 15 9m-2-2 4 4M18 3v3m-1.5-1.5h3M20 9.5v2m-1-1h2M10 3v2M9 4h2' },
  { id: 'crop', label: 'Crop (Enter applies)', key: 'C', path: 'M6 2v14a2 2 0 0 0 2 2h14M2 6h14a2 2 0 0 1 2 2v14' },
  { id: 'heal', label: 'Healing brush (Alt+click sets the source)', key: 'J', path: 'M10.6 3.6a3 3 0 0 1 4.2 0l5.6 5.6a3 3 0 0 1 0 4.2l-7 7a3 3 0 0 1-4.2 0l-5.6-5.6a3 3 0 0 1 0-4.2l7-7ZM7 10l7 7m-4-10 7 7M11 12h.01M13 10h.01M11 14h.01M9 12h.01' },
  { id: 'brush', label: 'Brush', key: 'B', path: 'M20 4c-3 1.5-7.5 6-9.4 8.6l1.8 1.8C15 12.5 19 8 20 4ZM9.6 13.6c-2 .1-3.2 1.3-3.4 3.2-.1 1.3-.8 2.2-2.2 2.7 2.8 1 6.6.3 7.4-2.4l-1.8-3.5Z' },
  { id: 'clone', label: 'Clone stamp (Alt+click sets the source)', key: 'S', path: 'M10 13 9.4 9.7A3 3 0 0 1 8.5 7.5V6a3.5 3.5 0 0 1 7 0v1.5a3 3 0 0 1-.9 2.2L14 13M5 13h14a1 1 0 0 1 1 1v3H4v-3a1 1 0 0 1 1-1Zm1 7h12' },
  { id: 'eraser', label: 'Eraser', key: 'E', path: 'M8.5 19.5h11M4.6 14.6l8.8-8.8a2 2 0 0 1 2.8 0l2.9 2.9a2 2 0 0 1 0 2.8l-7.9 7.9H9.1l-4.5-4.5a.2.2 0 0 1 0-.3Zm3.6-3.6 5.7 5.7' },
  { id: 'gradient', label: 'Gradient', key: 'G', path: 'M4 4h16v16H4ZM8 4v16m4-16v16m4-16v16' },
  { id: 'shape', label: 'Shape (rectangle, ellipse, line, polygon)', key: 'U', path: 'M4 4h10v10H4ZM17 10a4 4 0 1 1 0 8 4 4 0 0 1 0-8Z' },
  { id: 'pen', label: 'Pen (Enter: selection, Shift+Enter: shape)', key: 'P', path: 'M12 3 6 13l6 8 6-8-6-10Zm0 0v8m0 2.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3ZM4 21h16' },
  { id: 'text', label: 'Text', key: 'T', path: 'M5 7V4h14v3M12 4v16m-3 0h6' },
  { id: 'eyedropper', label: 'Eyedropper', key: 'I', path: 'M15 4.5a2.1 2.1 0 0 1 3 0L19.5 6a2.1 2.1 0 0 1 0 3L17 11.5 12.5 7 15 4.5ZM13.8 8.2 5.5 16.5V19h2.5l8.3-8.3M5.5 16.5h7' },
  { id: 'hand', label: 'Hand (drag to pan)', key: 'H', path: 'M8 13V5.5a1.5 1.5 0 0 1 3 0V11m0-6.5a1.5 1.5 0 0 1 3 0V11m0-5a1.5 1.5 0 0 1 3 0v5m0-2.5a1.5 1.5 0 0 1 3 0V15a6 6 0 0 1-6 6h-1.5a6 6 0 0 1-4.6-2.2L4.4 15a1.6 1.6 0 0 1 2.4-2.1L8 14' },
  { id: 'zoom', label: 'Zoom (click in, Alt+click out)', key: 'Z', path: 'M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM20 20l-4.8-4.8M8 10.5h5m-2.5-2.5v5' },
]

function setColour(which, value) {
  emit('colour', { primary: props.primary, secondary: props.secondary, [which]: value })
}

function swap() {
  emit('colour', { primary: props.secondary, secondary: props.primary })
}
</script>

<template>
  <aside class="tools" aria-label="Tools">
    <button
      v-for="t in TOOLS"
      :key="t.id"
      type="button"
      class="tool"
      :class="{ on: tool === t.id }"
      :aria-pressed="tool === t.id"
      :aria-label="t.label"
      :title="`${t.label} (${t.key})`"
      @click="emit('tool', t.id)"
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="t.path" /></svg>
    </button>
    <div class="colours">
      <label class="swatch back" title="Secondary colour - click to choose" :style="{ background: secondary }">
        <input type="color" :value="secondary" aria-label="Secondary colour" @change="setColour('secondary', $event.target.value)" />
      </label>
      <label class="swatch front" title="Primary colour - click to choose" :style="{ background: primary }">
        <input type="color" :value="primary" aria-label="Primary colour" @change="setColour('primary', $event.target.value)" />
      </label>
      <button type="button" class="swap" title="Swap colours (X)" aria-label="Swap colours" @click="swap">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 4 4 7l3 3M4 7h11a5 5 0 0 1 5 5v1M17 20l3-3-3-3m3 3H9a5 5 0 0 1-5-5v-1" /></svg>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.tools {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 3px;
  padding: 10px 6px;
  overflow-y: auto;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 16px;
  background: var(--panel-background, rgba(24, 19, 66, 0.72));
  backdrop-filter: blur(var(--glass-blur, 8px));
}

.tool {
  display: grid;
  flex: none;
  place-items: center;
  width: 44px;
  height: 38px;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 12px;
  background: none;
  color: var(--accent, #a98bff);
  cursor: pointer;
}

.tool svg,
.swap svg {
  width: 22px;
  height: 22px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.tool:hover:not(.on) {
  border-color: var(--line);
}

.tool.on {
  background: color-mix(in srgb, var(--accent, #8a5cf0) 28%, transparent);
  border-color: color-mix(in srgb, var(--accent, #8a5cf0) 50%, transparent);
  color: var(--text, #fff);
  box-shadow: 0 0 14px color-mix(in srgb, var(--accent, #8a5cf0) 35%, transparent);
}

.colours {
  position: relative;
  flex: none;
  width: 52px;
  height: 52px;
  margin-top: 8px;
}

.swatch {
  position: absolute;
  width: 30px;
  height: 30px;
  border: 2px solid rgba(255, 255, 255, 0.85);
  border-radius: 7px;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.5);
  cursor: pointer;
}

.swatch.front {
  top: 0;
  left: 2px;
  z-index: 1;
}

.swatch.back {
  right: 2px;
  bottom: 2px;
}

.swatch input {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  opacity: 0;
  cursor: pointer;
}

.swap {
  position: absolute;
  top: -2px;
  right: -4px;
  display: grid;
  place-items: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-dim);
  cursor: pointer;
}

.swap svg {
  width: 14px;
  height: 14px;
}

button:focus-visible,
.swatch:focus-within {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
