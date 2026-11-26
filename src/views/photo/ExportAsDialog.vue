<script setup>
import { computed, reactive } from 'vue'
import { EXPORT_SIZES, exportPlan } from './viewHelpers.js'

// PHOTO v3's File > Export As: every artboard (or the chosen ones) and/or the whole canvas, at 1x or at the
// sizes ticked (the longer side), as PNG, JPG or WebP - all in one press into a folder he picks once.
const props = defineProps({
  doc: { type: Object, required: true },
  title: { type: String, default: 'image' },
  busy: { type: Boolean, default: false },
})
const emit = defineEmits(['export', 'cancel'])

const boards = props.doc.artboards || []
const state = reactive({
  boards: boards.map((a) => a.id),
  whole: !boards.length,
  sizes: [],
  custom: '',
  format: 'png',
  quality: 90,
})

const sizes = computed(() => {
  const extra = String(state.custom)
    .split(/[\s,]+/)
    .map((v) => Math.round(Number(v)))
    .filter((v) => v >= 1 && v <= 12000)
  return [...new Set([...state.sizes, ...extra])].sort((a, b) => a - b)
})
const plan = computed(() => exportPlan(props.doc, { boards: state.boards, sizes: sizes.value, whole: state.whole, format: state.format, title: props.title }))

function toggle(list, v) {
  const i = state[list].indexOf(v)
  if (i >= 0) state[list].splice(i, 1)
  else state[list].push(v)
}

function go() {
  if (!plan.value.length) return
  emit('export', { boards: [...state.boards], whole: state.whole, sizes: sizes.value, format: state.format, quality: state.quality, plan: plan.value })
}
</script>

<template>
  <div class="back" @pointerdown.self="emit('cancel')">
    <form class="export" role="dialog" aria-label="Export As" @submit.prevent="go">
      <h3>Export As</h3>
      <fieldset>
        <legend>What</legend>
        <label class="check"><input v-model="state.whole" type="checkbox" /> The whole canvas ({{ doc.width }}×{{ doc.height }})</label>
        <label v-for="a in boards" :key="a.id" class="check">
          <input type="checkbox" :checked="state.boards.includes(a.id)" @change="toggle('boards', a.id)" />
          {{ a.name }} ({{ a.w }}×{{ a.h }})
        </label>
        <p v-if="!boards.length" class="dim">No artboards yet - Image › New artboard makes one.</p>
      </fieldset>
      <fieldset>
        <legend>Sizes (the longer side; none = 1x)</legend>
        <div class="sizes">
          <label v-for="n in EXPORT_SIZES" :key="n" class="chip" :class="{ on: state.sizes.includes(n) }">
            <input type="checkbox" :checked="state.sizes.includes(n)" @change="toggle('sizes', n)" />{{ n }}
          </label>
        </div>
        <input v-model="state.custom" class="custom" placeholder="More sizes, e.g. 20, 40, 60" aria-label="More sizes" />
      </fieldset>
      <fieldset class="row">
        <legend>Format</legend>
        <label v-for="f in ['png', 'jpeg', 'webp']" :key="f" class="chip" :class="{ on: state.format === f }"><input v-model="state.format" type="radio" :value="f" />{{ f === 'jpeg' ? 'JPG' : f.toUpperCase() }}</label>
        <label v-if="state.format !== 'png'" class="quality">
          Quality <input v-model.number="state.quality" type="range" min="40" max="100" /> <span>{{ state.quality }}</span>
        </label>
      </fieldset>
      <p class="dim">{{ plan.length }} file{{ plan.length === 1 ? '' : 's' }}: {{ plan.slice(0, 6).map((p) => p.name).join(', ') }}{{ plan.length > 6 ? '…' : '' }}</p>
      <div class="actions">
        <button type="button" @click="emit('cancel')">Cancel</button>
        <button type="submit" class="primary" :disabled="!plan.length || busy">{{ busy ? 'Exporting…' : 'Export…' }}</button>
      </div>
    </form>
  </div>
</template>

<style scoped>
.back {
  position: absolute;
  inset: 0;
  z-index: 50;
  display: grid;
  place-items: start center;
  padding-top: 80px;
  background: rgba(6, 3, 20, 0.35);
}

.export {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: min(440px, 92vw);
  max-height: calc(100% - 100px);
  padding: 16px 18px;
  overflow-y: auto;
  border: 1px solid var(--line);
  border-radius: 16px;
  background: var(--panel-background);
  backdrop-filter: blur(var(--glass-blur));
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.55);
  color: var(--text);
}

h3 {
  margin: 0;
  color: var(--accent);
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0.08em;
}

fieldset {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 10px;
}

fieldset.row {
  flex-direction: row;
  flex-wrap: wrap;
  align-items: center;
}

legend {
  padding: 0 4px;
  color: var(--text-dim);
  font-size: 11.5px;
}

.check {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
}

input[type='checkbox'],
input[type='radio'],
input[type='range'] {
  accent-color: var(--accent);
}

.sizes {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.chip {
  display: inline-flex;
  align-items: center;
  padding: 3px 10px;
  border: 1px solid var(--line);
  border-radius: 999px;
  color: var(--text-dim);
  font-size: 12px;
  cursor: pointer;
}

.chip input {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}

.chip.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 20%, transparent);
  color: var(--text);
}

.chip:focus-within {
  outline: 2px solid var(--accent);
}

.custom {
  padding: 4px 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.quality {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
  color: var(--text-dim);
  font-size: 12px;
}

.dim {
  margin: 0;
  color: var(--text-dim);
  font-size: 12px;
  word-break: break-word;
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.actions button {
  padding: 5px 14px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.actions button.primary {
  border-color: var(--accent);
  background: var(--accent);
  color: #160f33;
}

.actions button:disabled {
  opacity: 0.45;
  cursor: default;
}
</style>
