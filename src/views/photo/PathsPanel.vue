<script setup>
import { nextTick, ref } from 'vue'

// PHOTO v3's PATHS: the pen's paths, kept in the document and named. Choose one to see it on the canvas;
// double-click to rename. The footer strokes it with the brush (size and colour of the brush), fills it with
// the primary colour on a new layer, makes a selection from it, or deletes it.
const props = defineProps({
  paths: { type: Array, default: () => [] },
  activeId: { type: String, default: '' },
  width: { type: Number, default: 1 },
  height: { type: Number, default: 1 },
})
const emit = defineEmits(['pick', 'rename', 'remove', 'stroke', 'fill', 'select'])

const renaming = ref('')
const draft = ref('')
const box = ref(null)

const shapeOf = (p) => `M${p.points.map((q) => `${q[0]} ${q[1]}`).join('L')}${p.closed ? 'Z' : ''}`

async function startRename(p) {
  renaming.value = p.id
  draft.value = p.name
  await nextTick()
  box.value?.[0]?.focus()
  box.value?.[0]?.select()
}

function finish(save) {
  const id = renaming.value
  renaming.value = ''
  if (save && id && draft.value.trim()) emit('rename', id, draft.value.trim())
}
</script>

<template>
  <section class="paths" aria-label="Paths">
    <p v-if="!paths.length" class="dim">No paths yet. Click points with the pen (P); Enter keeps the path here.</p>
    <ul v-else>
      <li v-for="p in paths" :key="p.id">
        <div class="row" :class="{ on: p.id === activeId }" role="button" tabindex="0" @click="emit('pick', p.id === activeId ? '' : p.id)" @keydown.enter="emit('pick', p.id)">
          <svg class="thumb" :viewBox="`0 0 ${width} ${height}`" preserveAspectRatio="xMidYMid meet" aria-hidden="true">
            <path :d="shapeOf(p)" />
          </svg>
          <input
            v-if="renaming === p.id"
            ref="box"
            v-model="draft"
            class="name-edit"
            maxlength="60"
            aria-label="Path name"
            @click.stop
            @keydown.enter.prevent="finish(true)"
            @keydown.esc.prevent="finish(false)"
            @blur="finish(true)"
          />
          <span v-else class="name" @dblclick.stop="startRename(p)">{{ p.name }}</span>
          <span class="dim">{{ p.points.length }} pts</span>
        </div>
      </li>
    </ul>
    <footer>
      <button type="button" title="Stroke the path with the brush" aria-label="Stroke path" :disabled="!activeId" @click="emit('stroke', activeId)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="7" /></svg>
      </button>
      <button type="button" title="Fill the path with the primary colour" aria-label="Fill path" :disabled="!activeId" @click="emit('fill', activeId)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="7" fill="currentColor" /></svg>
      </button>
      <button type="button" title="Make a selection from the path" aria-label="Path to selection" :disabled="!activeId" @click="emit('select', activeId)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="7" stroke-dasharray="3 3" /></svg>
      </button>
      <button type="button" class="bad" title="Delete the path" aria-label="Delete path" :disabled="!activeId" @click="emit('remove', activeId)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M10 11v6m4-6v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3" /></svg>
      </button>
    </footer>
  </section>
</template>

<style scoped>
.paths {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  padding: 10px 12px 8px;
  color: var(--text);
}

ul {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 3px;
  min-height: 0;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  list-style: none;
}

.row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 6px;
  border: 1px solid transparent;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  cursor: pointer;
}

.row.on {
  border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  background: color-mix(in srgb, var(--accent) 18%, transparent);
}

.thumb {
  flex: none;
  width: 44px;
  height: 32px;
  border-radius: 5px;
  background: rgba(255, 255, 255, 0.9);
}

.thumb path {
  fill: rgba(80, 60, 160, 0.25);
  stroke: #3a2a8a;
  stroke-width: 2;
  vector-effect: non-scaling-stroke;
}

.name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: 13px;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.name-edit {
  flex: 1;
  min-width: 0;
  padding: 3px 6px;
  border: 1px solid var(--accent);
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

footer {
  display: flex;
  justify-content: flex-end;
  gap: 4px;
  padding-top: 4px;
  border-top: 1px solid var(--line);
}

footer button {
  display: grid;
  place-items: center;
  width: 30px;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: 8px;
  background: none;
  color: var(--text-dim);
}

footer button:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.06);
  color: var(--text);
}

footer button.bad:hover:not(:disabled) {
  color: var(--bad, #ff6b8b);
}

footer svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.dim {
  margin: 0;
  color: var(--text-dim);
  font-size: 11.5px;
}

button {
  font: inherit;
  cursor: pointer;
}

button:disabled {
  opacity: 0.4;
  cursor: default;
}

button:focus-visible,
.row:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
</style>
