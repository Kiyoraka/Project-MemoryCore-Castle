<script setup>
import { computed } from 'vue'

// PHOTO v3's CHANNELS: the picture (RGB) and its red, green, blue and alpha channels as small greys from a
// light thumbnail. A click views that channel alone on the canvas (RGB goes back); Ctrl+click loads the
// channel as a selection (its brightness = how selected).
const props = defineProps({
  thumb: { type: Object, default: null }, // { data, width, height } - the picture at thumbnail size
  view: { type: String, default: '' }, // '' (RGB) | 'r' | 'g' | 'b' | 'alpha'
})
const emit = defineEmits(['view', 'load'])

const CHANNELS = [
  { id: '', label: 'RGB', keys: 'Ctrl+2' },
  { id: 'r', label: 'Red', keys: 'Ctrl+3' },
  { id: 'g', label: 'Green', keys: 'Ctrl+4' },
  { id: 'b', label: 'Blue', keys: 'Ctrl+5' },
  { id: 'alpha', label: 'Alpha', keys: '' },
]

function urlOf(pixels, channel) {
  if (!pixels || typeof document === 'undefined') return ''
  const c = document.createElement('canvas')
  c.width = pixels.width
  c.height = pixels.height
  const ctx = c.getContext('2d')
  const img = ctx.createImageData(pixels.width, pixels.height)
  const s = pixels.data
  const d = img.data
  const k = { r: 0, g: 1, b: 2, alpha: 3 }[channel]
  for (let i = 0; i < d.length; i += 4) {
    if (k === undefined) {
      d[i] = s[i]
      d[i + 1] = s[i + 1]
      d[i + 2] = s[i + 2]
      d[i + 3] = s[i + 3]
    } else {
      const v = k === 3 ? s[i + 3] : (s[i + k] * s[i + 3]) / 255
      d[i] = d[i + 1] = d[i + 2] = v
      d[i + 3] = 255
    }
  }
  ctx.putImageData(img, 0, 0)
  return c.toDataURL('image/png')
}

const images = computed(() => Object.fromEntries(CHANNELS.map((ch) => [ch.id, urlOf(props.thumb, ch.id)])))

function pick(ch, event) {
  if (event.ctrlKey || event.metaKey) emit('load', ch.id || 'rgb')
  else emit('view', ch.id)
}
</script>

<template>
  <section class="channels" aria-label="Channels">
    <p v-if="!thumb" class="dim">Open a photo to see its channels.</p>
    <ul v-else>
      <li v-for="ch in CHANNELS" :key="ch.id || 'rgb'">
        <button type="button" class="row" :class="{ on: view === ch.id }" :aria-pressed="view === ch.id" :title="`${ch.label} - click to view it alone, Ctrl+click to load it as a selection`" @click="pick(ch, $event)">
          <span class="eye" :class="{ off: view && view !== ch.id && ch.id }" aria-hidden="true">
            <svg viewBox="0 0 24 24"><path d="M2 12s3.6-6.5 10-6.5S22 12 22 12s-3.6 6.5-10 6.5S2 12 2 12Z" /><circle cx="12" cy="12" r="3" /></svg>
          </span>
          <img :src="images[ch.id]" alt="" />
          <span class="name">{{ ch.label }}</span>
          <kbd v-if="ch.keys">{{ ch.keys }}</kbd>
        </button>
      </li>
    </ul>
    <p class="dim hint">Ctrl+click a channel to load it as a selection.</p>
  </section>
</template>

<style scoped>
.channels {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  padding: 10px 12px;
  overflow-y: auto;
  color: var(--text);
}

ul {
  display: flex;
  flex-direction: column;
  gap: 3px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.row {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 4px 6px;
  border: 1px solid transparent;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text);
  text-align: left;
}

.row.on {
  border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  background: color-mix(in srgb, var(--accent) 18%, transparent);
}

.eye {
  display: grid;
  place-items: center;
  width: 22px;
  color: var(--accent);
}

.eye.off {
  opacity: 0.35;
}

svg {
  width: 17px;
  height: 17px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
}

img {
  width: 44px;
  height: 32px;
  border-radius: 5px;
  background: #000;
  object-fit: contain;
}

.name {
  flex: 1;
  font-size: 13px;
}

kbd {
  color: var(--text-dim);
  font: inherit;
  font-size: 11px;
}

.dim {
  margin: 0;
  color: var(--text-dim);
  font-size: 12px;
}

.hint {
  margin-top: auto;
}

button {
  font: inherit;
  cursor: pointer;
}

button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
</style>
