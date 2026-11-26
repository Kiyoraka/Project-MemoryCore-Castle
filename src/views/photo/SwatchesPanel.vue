<script setup>
import { ref } from 'vue'

// PHOTO v3's SWATCHES: his palette, kept on this machine. A click makes a swatch the primary colour
// (Alt+click: the secondary), right-click takes it away; + keeps the primary colour as a new swatch.
const props = defineProps({
  primary: { type: String, default: '#ffffff' },
})
const emit = defineEmits(['pick'])

const KEY = 'photo.swatches'
const START = ['#000000', '#ffffff', '#7b5cf0', '#a98bff', '#ff6b8b', '#ffb36b', '#ffe066', '#6bd6a0', '#59b7ff', '#2b2560', '#e8d8b8', '#808080']

function load() {
  try {
    const list = JSON.parse(localStorage.getItem(KEY) || 'null')
    if (Array.isArray(list)) return list.filter((c) => /^#[0-9a-f]{6}$/i.test(c)).slice(0, 120)
  } catch {
    // nothing kept yet
  }
  return [...START]
}

const swatches = ref(load())

function keep() {
  try {
    localStorage.setItem(KEY, JSON.stringify(swatches.value))
  } catch {
    // a private window keeps nothing - the palette still works for now
  }
}

function add() {
  const c = String(props.primary || '').toLowerCase()
  if (!/^#[0-9a-f]{6}$/.test(c) || swatches.value.includes(c)) return
  swatches.value = [...swatches.value, c]
  keep()
}

function remove(i, event) {
  event.preventDefault()
  swatches.value = swatches.value.filter((_, j) => j !== i)
  keep()
}
</script>

<template>
  <section class="swatches" aria-label="Swatches">
    <div class="grid">
      <button
        v-for="(c, i) in swatches"
        :key="`${c}-${i}`"
        type="button"
        class="swatch"
        :class="{ on: c === primary.toLowerCase() }"
        :style="{ background: c }"
        :title="`${c} - click: primary, Alt+click: secondary, right-click: remove`"
        :aria-label="`Swatch ${c}`"
        @click="emit('pick', { color: c, which: $event.altKey ? 'secondary' : 'primary' })"
        @contextmenu="remove(i, $event)"
      ></button>
      <button type="button" class="swatch add" title="Keep the primary colour as a swatch" aria-label="Add the primary colour" @click="add">+</button>
    </div>
  </section>
</template>

<style scoped>
.swatches {
  min-height: 0;
  padding: 10px 12px;
  overflow-y: auto;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(26px, 1fr));
  gap: 6px;
}

.swatch {
  aspect-ratio: 1;
  padding: 0;
  border: 1px solid rgba(255, 255, 255, 0.25);
  border-radius: 6px;
  cursor: pointer;
}

.swatch.on {
  box-shadow: 0 0 0 2px var(--accent);
}

.swatch.add {
  display: grid;
  place-items: center;
  border-style: dashed;
  background: none;
  color: var(--text-dim);
  font-size: 16px;
}

.swatch:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
