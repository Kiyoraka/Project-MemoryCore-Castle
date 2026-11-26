<script setup>
import { nextTick, ref, watch } from 'vue'

// PHOTO's HISTORY: every step newest first with its icon and label (hers carry a small sparkle);
// a click jumps the picture back (or forward) to that step.
const props = defineProps({
  entries: { type: Array, default: () => [] },
  index: { type: Number, default: -1 },
  companion: { type: String, default: 'Your companion' },
})
const emit = defineEmits(['jump', 'close'])

const ICONS = {
  open: 'M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z',
  brush: 'M20 4c-3 1.5-7.5 6-9.4 8.6l1.8 1.8C15 12.5 19 8 20 4ZM9.6 13.6c-2 .1-3.2 1.3-3.4 3.2-.1 1.3-.8 2.2-2.2 2.7 2.8 1 6.6.3 7.4-2.4l-1.8-3.5Z',
  eraser: 'M8.5 19.5h11M4.6 14.6l8.8-8.8a2 2 0 0 1 2.8 0l2.9 2.9a2 2 0 0 1 0 2.8l-7.9 7.9H9.1l-4.5-4.5Z',
  heal: 'M10.6 3.6a3 3 0 0 1 4.2 0l5.6 5.6a3 3 0 0 1 0 4.2l-7 7a3 3 0 0 1-4.2 0l-5.6-5.6a3 3 0 0 1 0-4.2l7-7ZM7 10l7 7m-4-10 7 7',
  clone: 'M10 13 9.4 9.7A3 3 0 0 1 8.5 7.5V6a3.5 3.5 0 0 1 7 0v1.5a3 3 0 0 1-.9 2.2L14 13M5 13h14a1 1 0 0 1 1 1v3H4v-3a1 1 0 0 1 1-1Zm1 7h12',
  curves: 'M4 4h16v16H4ZM6 18c4 0 4-12 12-12',
  levels: 'M4 20V10m4 10V6m4 14v-8m4 8V4m4 16v-6',
  hsl: 'M12 3s6 6.4 6 11a6 6 0 0 1-12 0c0-4.6 6-11 6-11Z',
  mask: 'M4 4h16v16H4ZM12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z',
  select: 'M4 7V4h3M10 4h4M17 4h3v3M20 10v4M20 17v3h-3M14 20h-4M7 20H4v-3M4 14v-4',
  crop: 'M6 2v14a2 2 0 0 0 2 2h14M2 6h14a2 2 0 0 1 2 2v14',
  transform: 'M4 4v5h5M4.6 9A8 8 0 1 1 6 16.5',
  text: 'M5 7V4h14v3M12 4v16m-3 0h6',
  gradient: 'M4 4h16v16H4ZM8 4v16m4-16v16m4-16v16',
  shape: 'M4 4h10v10H4ZM17 10a4 4 0 1 1 0 8 4 4 0 0 1 0-8Z',
  filter: 'M4 5h16l-6 7v6l-4 2v-8L4 5Z',
  adjust: 'M4 6h10m4 0h2M4 12h4m4 0h8M4 18h12m4 0h0M16 4v4M10 10v4M18 16v4',
  layer: 'm12 3 9 5-9 5-9-5 9-5Zm-9 9 9 5 9-5M3 16l9 5 9-5',
  edit: 'M4 20h4L19 9l-4-4L4 16v4Z',
}

const rows = ref(null)
watch(
  () => props.index,
  async () => {
    await nextTick()
    rows.value?.querySelector('.on')?.scrollIntoView?.({ block: 'nearest' })
  },
)
</script>

<template>
  <section class="history" aria-label="History">
    <header>
      <svg class="head-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3a9 9 0 1 0 9 9M12 7v5l3 2M21 4v5h-5" /></svg>
      <h2>HISTORY</h2>
      <button type="button" class="x" aria-label="Close history" title="Close" @click="emit('close')">×</button>
    </header>
    <p v-if="!entries.length" class="dim">Nothing done yet.</p>
    <ul ref="rows">
      <li v-for="e in [...entries].reverse()" :key="e.at">
        <button type="button" :class="{ on: e.at === index, later: e.at > index }" :title="e.at > index ? 'Redo up to here' : 'Go back to here'" @click="emit('jump', e.at)">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="ICONS[e.icon] || ICONS.edit" /></svg>
          <span class="label">{{ e.label }}</span>
          <span v-if="e.hers" class="sparkle" :title="`${companion} did this`" :aria-label="`${companion} did this`">✦</span>
          <span v-if="e.at === index" class="dot" aria-hidden="true"></span>
        </button>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.history {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  padding: 12px 12px 10px;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 16px;
  background: var(--panel-background, rgba(24, 19, 66, 0.72));
  backdrop-filter: blur(var(--glass-blur, 8px));
  color: var(--text);
}

header {
  display: flex;
  align-items: center;
  gap: 8px;
}

h2 {
  flex: 1;
  margin: 0;
  color: var(--accent);
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0.22em;
}

.head-icon {
  width: 16px;
  height: 16px;
}

.x {
  padding: 0 4px;
  border: 0;
  background: none;
  color: var(--text-dim);
  font-size: 18px;
  line-height: 1;
}

ul {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  list-style: none;
}

li button {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 6px 8px;
  border: 0;
  border-radius: 8px;
  background: none;
  color: var(--text);
  font-size: 13px;
  text-align: left;
}

li button:hover {
  background: rgba(255, 255, 255, 0.05);
}

li button.on {
  background: color-mix(in srgb, var(--accent, #8a5cf0) 22%, transparent);
}

li button.later {
  opacity: 0.5;
}

svg {
  flex: none;
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.sparkle {
  color: var(--accent);
  font-size: 11px;
  text-shadow: 0 0 6px var(--accent);
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
}

.dim {
  margin: 0;
  color: var(--text-dim);
  font-size: 12px;
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
