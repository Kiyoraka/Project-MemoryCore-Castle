<script setup>
import { computed, ref, watch } from 'vue'
import { AUDIENCES, starPositions } from './profile.js'

// PROFILE, the Constellation look (design profile-tab-2): her traits as stars joined in her own
// constellation, waiting ones as faint dotted stars, the chosen star opened in a card beside the
// sky. Draws only; ProfileView decides.
const props = defineProps({
  companion: { type: String, default: 'Your companion' },
  lived: { type: Array, default: () => [] },
  waiting: { type: Array, default: () => [] },
  busy: { type: String, default: '' },
})
const emit = defineEmits(['open', 'keep', 'edit', 'no', 'audience', 'scan', 'write'])

const W = 600
const H = 400
const stars = computed(() => [
  ...props.lived.map((t) => ({ key: `t${t.number}`, name: t.name, item: t })),
  ...props.waiting.map((w) => ({ key: w.key, name: w.name, item: w, waiting: true })),
])
const placed = computed(() => {
  const at = starPositions(stars.value, W, H)
  return stars.value.map((s, i) => ({ ...s, ...at[i] }))
})
const path = computed(() => {
  const lit = placed.value.filter((s) => !s.waiting)
  if (lit.length < 2) return ''
  const points = lit.map((s) => `${s.x},${s.y}`)
  return lit.length > 2 ? `M${points.join(' L')} Z` : `M${points.join(' L')}`
})
const chosenKey = ref('')
const chosen = computed(() => placed.value.find((s) => s.key === chosenKey.value) ?? placed.value.find((s) => !s.waiting) ?? placed.value[0] ?? null)
watch(stars, () => {
  if (!stars.value.some((s) => s.key === chosenKey.value)) chosenKey.value = ''
})

// the label sits away from the sky's middle, so it never covers the lines
const anchor = (s) => (s.x < W / 2 - 20 ? 'end' : s.x > W / 2 + 20 ? 'start' : 'middle')
const dx = (s) => (anchor(s) === 'end' ? -12 : anchor(s) === 'start' ? 12 : 0)
const dy = (s) => (anchor(s) === 'middle' ? (s.y < H / 2 ? -14 : 22) : 5)
</script>

<template>
  <div class="constellation">
    <div class="sky-and-card">
      <svg class="sky" :viewBox="`0 0 ${W} ${H}`" role="img" :aria-label="`${companion}'s traits as a constellation`">
        <path v-if="path" :d="path" class="link" />
        <g v-for="s in placed" :key="s.key" class="star" :class="{ waiting: s.waiting, on: chosen?.key === s.key }" tabindex="0" role="button" :aria-label="s.name" @click="chosenKey = s.key" @keydown.enter="chosenKey = s.key">
          <circle :cx="s.x" :cy="s.y" r="16" class="hit" />
          <circle v-if="s.waiting" :cx="s.x" :cy="s.y" r="9" class="ring" />
          <path :d="`M${s.x} ${s.y - 8} L${s.x + 2} ${s.y - 2} L${s.x + 8} ${s.y} L${s.x + 2} ${s.y + 2} L${s.x} ${s.y + 8} L${s.x - 2} ${s.y + 2} L${s.x - 8} ${s.y} L${s.x - 2} ${s.y - 2} Z`" class="spark" />
          <text :x="s.x + dx(s)" :y="s.y + dy(s)" :text-anchor="anchor(s)">{{ s.waiting ? `${s.name} (waiting)` : s.name }}</text>
        </g>
        <text v-if="!placed.length" :x="W / 2" :y="H / 2" text-anchor="middle" class="empty">No stars yet. Scan your diary, or write a trait.</text>
      </svg>

      <aside v-if="chosen" class="card">
        <p class="title">✦ {{ chosen.name }}</p>
        <p class="sound">How {{ companion }} sounds: <em>{{ chosen.item.line }}</em></p>
        <div class="pills" role="radiogroup" :aria-label="`Who hears ${chosen.name} in the room`">
          <button v-for="a in AUDIENCES" :key="a.id" type="button" role="radio" :aria-checked="(chosen.item.audience ?? 'everyone') === a.id" :class="{ on: (chosen.item.audience ?? 'everyone') === a.id }" :title="a.line" :disabled="!!busy" @click="emit('audience', chosen.item, a.id)">{{ a.label }}</button>
        </div>
        <div class="row">
          <template v-if="chosen.waiting">
            <button class="primary" :disabled="!!busy" @click="emit('keep', chosen.item)">Keep</button>
            <button @click="emit('edit', chosen.item)">Edit</button>
            <button @click="emit('no', chosen.item)">No</button>
          </template>
          <button v-else @click="emit('open', chosen.item)">Open</button>
        </div>
      </aside>
    </div>

    <div class="actions">
      <button class="primary" :disabled="!!busy" @click="emit('scan')">{{ busy === 'diary' ? 'Reading…' : 'Scan my diary' }}</button>
      <button :disabled="!!busy" @click="emit('write')">Write a trait</button>
    </div>
  </div>
</template>

<style scoped>
.constellation {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.sky-and-card {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}
.sky {
  flex: 2 1 320px;
  min-width: 0;
  height: auto;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: radial-gradient(ellipse at center, color-mix(in srgb, var(--accent) 8%, transparent), rgba(0, 0, 0, 0.25));
}
.link {
  fill: none;
  stroke: color-mix(in srgb, var(--accent) 70%, transparent);
  stroke-width: 1.4;
}
.star {
  cursor: pointer;
  outline: none;
}
.hit {
  fill: transparent;
}
.spark {
  fill: #fff;
  filter: drop-shadow(0 0 6px var(--accent));
}
.star.on .spark {
  fill: color-mix(in srgb, var(--accent) 40%, #fff);
  transform-box: fill-box;
  transform-origin: center;
  transform: scale(1.35);
}
.star.waiting .spark {
  opacity: 0.45;
}
.ring {
  fill: none;
  stroke: var(--text-dim);
  stroke-dasharray: 2 3;
}
.star text {
  fill: var(--text);
  font-size: 15px;
}
.star.waiting text {
  fill: var(--text-dim);
}
.star:focus-visible .spark {
  outline: 2px solid var(--accent);
}
.empty {
  fill: var(--text-dim);
  font-size: 15px;
}
.card {
  display: flex;
  flex: 1 1 200px;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: rgba(0, 0, 0, 0.22);
}
.card .title {
  margin: 0;
  font-size: 17px;
}
.sound {
  margin: 0;
  font-size: 13px;
  color: var(--text-dim);
}
.pills {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.pills button {
  border-radius: 999px;
  font-size: 12px;
}
.pills button.on {
  border-color: var(--accent);
  background: var(--accent);
  color: #07142e;
}
.row,
.actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 8px;
}
.actions button {
  min-width: 160px;
  border-radius: 999px;
}
button {
  padding: 7px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
button.primary {
  border-color: var(--accent);
  background: var(--accent);
  color: #07142e;
}
button:disabled {
  opacity: 0.45;
  cursor: default;
}
button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
