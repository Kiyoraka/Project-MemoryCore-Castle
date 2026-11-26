<script setup>
import { computed, nextTick, ref } from 'vue'

// PHOTO v3's ACTIONS (photo-v3-1): his recorded actions (and hers), each a folder of steps with a play
// button. Record (●) asks a name, then every step he or she takes is kept until Stop (■); Play (▶) runs the
// chosen action on the picture as one history step; Batch runs it on every picture of a folder (into its
// PHOTO out, the originals never touched); the bin deletes it.
const props = defineProps({
  actions: { type: Array, default: () => [] }, // [{ name, steps }]
  recording: { type: String, default: '' }, // the action being recorded, '' when not
  recorded: { type: Number, default: 0 }, // steps kept so far
  busy: { type: String, default: '' }, // '' | 'play' | 'batch'
  hasDoc: { type: Boolean, default: false },
})
const emit = defineEmits(['record', 'stop', 'play', 'remove', 'batch'])

const chosen = ref('')
const open = ref(new Set())
const naming = ref(false)
const draft = ref('')
const nameBox = ref(null)

const current = computed(() => props.actions.find((a) => a.name === chosen.value) || null)

const STEP_WORDS = {
  adjustment: (s) => `Adjustment: ${s.type}`,
  filter: (s) => `Filter: ${s.name}`,
  style: (s) => `Style: ${s.effect}`,
  brush: (s) => `Brush stroke (${s.points?.length || 0} points)`,
  select: () => 'Select',
  transform: () => 'Transform layer',
}
const stepLabel = (s) => (STEP_WORDS[s.op] ? STEP_WORDS[s.op](s) : String(s.op).replace(/_/g, ' ').replace(/^\w/, (c) => c.toUpperCase()))

function toggle(name) {
  const next = new Set(open.value)
  if (next.has(name)) next.delete(name)
  else next.add(name)
  open.value = next
}

async function startNaming() {
  if (props.recording) return
  naming.value = true
  draft.value = ''
  await nextTick()
  nameBox.value?.focus()
}

function startRecording() {
  const name = draft.value.trim()
  naming.value = false
  if (name) {
    emit('record', name)
    chosen.value = name
  }
}
</script>

<template>
  <section class="actions" aria-label="Actions">
    <p v-if="!actions.length && !naming" class="dim">No actions yet. Press ● to record what you do, ■ to stop.</p>
    <ul>
      <li v-for="a in actions" :key="a.name">
        <div class="row" :class="{ on: a.name === chosen, rec: a.name === recording }" role="button" tabindex="0" @click="chosen = a.name" @keydown.enter="chosen = a.name">
          <button type="button" class="arrow" :aria-label="open.has(a.name) ? 'Hide the steps' : 'Show the steps'" :aria-expanded="open.has(a.name)" @click.stop="toggle(a.name)">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="open.has(a.name) ? 'm6 9 6 6 6-6' : 'm9 6 6 6-6 6'" /></svg>
          </button>
          <svg class="folder" viewBox="0 0 24 24" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" /></svg>
          <span class="name">{{ a.name }}</span>
          <span v-if="a.name === recording" class="dot" title="Recording">● {{ recorded }}</span>
          <button v-else type="button" class="play" :disabled="!hasDoc || !!busy || !a.steps.length" :aria-label="`Play ${a.name}`" title="Play on this picture" @click.stop="emit('play', a.name)">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 7v10l8-5-8-5Z" fill="currentColor" /></svg>
          </button>
        </div>
        <ol v-if="open.has(a.name)" class="steps">
          <li v-for="(s, i) in a.steps" :key="i">{{ stepLabel(s) }}</li>
          <li v-if="!a.steps.length" class="dim">No steps yet</li>
        </ol>
      </li>
    </ul>
    <form v-if="naming" class="naming" @submit.prevent="startRecording">
      <input ref="nameBox" v-model="draft" maxlength="60" placeholder="Name the new action" aria-label="New action name" @keydown.esc.prevent="naming = false" />
      <button type="submit" class="go" :disabled="!draft.trim()">Record</button>
    </form>
    <p v-if="busy === 'batch'" class="dim">Running on the folder…</p>
    <footer>
      <button type="button" title="Stop recording" aria-label="Stop" :disabled="!recording" @click="emit('stop')">
        <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="1" fill="currentColor" /></svg>
      </button>
      <button type="button" title="Play the chosen action" aria-label="Play" :disabled="!current || !hasDoc || !!busy || !!recording" @click="emit('play', chosen)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 6v12l10-6-10-6Z" fill="currentColor" /></svg>
      </button>
      <button type="button" class="record" :class="{ on: !!recording }" title="Record a new action" aria-label="Record" :disabled="!!recording" @click="startNaming">
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="6" /></svg>
      </button>
      <button type="button" title="Batch: run the chosen action on every picture in a folder" aria-label="Batch on folder" :disabled="!current || !!busy || !!recording" @click="emit('batch', chosen)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" /></svg>
      </button>
      <button type="button" class="bad" title="Delete the chosen action" aria-label="Delete action" :disabled="!current || current.name === recording" @click="emit('remove', chosen), (chosen = '')">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M10 11v6m4-6v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3" /></svg>
      </button>
    </footer>
  </section>
</template>

<style scoped>
.actions {
  display: flex;
  flex-direction: column;
  gap: 6px;
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
  padding: 5px 6px;
  border: 1px solid transparent;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  cursor: pointer;
}

.row.on {
  border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  background: color-mix(in srgb, var(--accent) 18%, transparent);
}

.row.rec {
  border-color: color-mix(in srgb, var(--bad, #ff6b8b) 60%, transparent);
}

.arrow,
.play,
footer button {
  display: grid;
  flex: none;
  place-items: center;
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-dim);
}

.arrow {
  width: 18px;
  height: 22px;
}

.play {
  width: 26px;
  height: 26px;
  margin-left: auto;
  border-radius: 50%;
  background: color-mix(in srgb, var(--accent) 30%, transparent);
  color: var(--text);
}

svg {
  width: 17px;
  height: 17px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.folder {
  flex: none;
  color: var(--accent);
}

.name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: 13.5px;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.dot {
  color: var(--bad, #ff6b8b);
  font-size: 12px;
}

.steps {
  margin: 2px 0 4px 48px;
  padding: 0;
  color: var(--text-dim);
  font-size: 12px;
  list-style: decimal;
}

.naming {
  display: flex;
  gap: 6px;
}

.naming input {
  flex: 1;
  min-width: 0;
  padding: 4px 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.go {
  padding: 4px 12px;
  border: 1px solid var(--accent);
  border-radius: 999px;
  background: var(--accent);
  color: #160f33;
  font-size: 12px;
}

footer {
  display: flex;
  justify-content: center;
  gap: 10px;
  padding-top: 4px;
  border-top: 1px solid var(--line);
}

footer button {
  width: 32px;
  height: 28px;
  border-radius: 8px;
}

footer button:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.06);
  color: var(--text);
}

.record.on,
.record:hover:not(:disabled) {
  color: var(--bad, #ff6b8b);
}

footer button.bad:hover:not(:disabled) {
  color: var(--bad, #ff6b8b);
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

button:disabled {
  opacity: 0.35;
  cursor: default;
}

button:focus-visible,
.row:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
</style>
