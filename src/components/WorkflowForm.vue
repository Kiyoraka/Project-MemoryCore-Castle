<script setup>
// The chosen workflow's form (designs studio-3 and studio-workflows-2): each block drawn in the
// file's order - an upload tile, a character tile with its name, a text box, choices, the
// "Write prompt from picture" button - then the main "Create picture" button. The picture goes to the
// image queue like every STUDIO picture; the room's request line shows it drawing.
import { computed, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { pictureDescribe, workflowPickPicture, workflowRun } from '../api/castle.js'
import { createAction, describeAction, emptyValues, fieldsOf, runValues } from './workflows.js'

const props = defineProps({
  entry: { type: Object, required: true },
  /** the Pictures power: 'ready' (the key), 'builtin' (Codex), 'prompt' (none) */
  way: { type: String, default: 'prompt' },
  /** the member's own ComfyUI is on: it draws a plain text picture first */
  comfy: { type: Boolean, default: false },
  loaded: { type: Boolean, default: true },
})
const emit = defineEmits(['keys', 'queued'])

const workflow = computed(() => props.entry.workflow)
const fields = computed(() => fieldsOf(workflow.value))
const values = ref(emptyValues(workflow.value))
const running = ref(false)
const reading = ref('')
const problem = ref('')
const done = ref('')
watch(
  () => props.entry.id,
  () => {
    values.value = emptyValues(workflow.value)
    problem.value = ''
    done.value = ''
  },
)

const action = computed(() => createAction(workflow.value, values.value, props.way, props.loaded, running.value, props.comfy))
const show = (path) => (path ? convertFileSrc(path) : '')

async function pick(block) {
  problem.value = ''
  try {
    const path = await workflowPickPicture(block.label)
    if (path) values.value[block.id].picture = path
  } catch (e) {
    problem.value = String(e?.message ?? e)
  }
}

async function describe(block) {
  const a = describeAction(block, values.value, props.way, !!reading.value)
  if (a.kind === 'key') return emit('keys')
  if (a.kind !== 'ready') return
  problem.value = ''
  reading.value = block.id
  try {
    const words = await pictureDescribe(values.value[block.from].picture)
    if (values.value[block.into]) values.value[block.into].text = words
  } catch (e) {
    problem.value = String(e?.message ?? e)
  } finally {
    reading.value = ''
  }
}

async function create() {
  const a = action.value
  problem.value = ''
  done.value = ''
  if (a.kind === 'key') return emit('keys')
  if (a.kind === 'missing') {
    problem.value = a.why
    return
  }
  if (a.kind !== 'ready') return
  running.value = true
  try {
    await workflowRun(props.entry.id, runValues(values.value))
    done.value = 'Drawing it now - it appears on the canvas when it is ready.'
    emit('queued')
  } catch (e) {
    problem.value = String(e?.message ?? e)
  } finally {
    running.value = false
  }
}
</script>

<template>
  <section class="form" :aria-label="workflow?.name">
    <header>
      <h4>{{ workflow?.name }}</h4>
      <p v-if="workflow?.description" class="hint">{{ workflow.description }}</p>
    </header>

    <div class="fields">
      <template v-for="b in fields" :key="b.id">
        <button v-if="b.type === 'picture'" class="upload" :class="{ filled: values[b.id]?.picture }" @click="pick(b)">
          <img v-if="values[b.id]?.picture" :src="show(values[b.id].picture)" :alt="b.label" />
          <span v-else class="upload-word"><span class="big" aria-hidden="true">⇪</span>{{ b.label }}</span>
        </button>

        <div v-else-if="b.type === 'character'" class="character">
          <button class="portrait" :class="{ filled: values[b.id]?.picture }" :aria-label="`Picture for ${b.label}`" @click="pick(b)">
            <img v-if="values[b.id]?.picture" :src="show(values[b.id].picture)" :alt="b.label" />
            <span v-else aria-hidden="true">⇪</span>
          </button>
          <label>
            <span class="label">{{ b.label }}</span>
            <input v-model="values[b.id].name" type="text" maxlength="60" placeholder="Name (optional)" />
          </label>
        </div>

        <label v-else-if="b.type === 'text'" class="text">
          <span class="label">{{ b.label }}</span>
          <textarea v-model="values[b.id].text" rows="4" maxlength="4000" :placeholder="b.label"></textarea>
        </label>

        <div v-else-if="b.type === 'chips'" class="chips" role="radiogroup" :aria-label="b.label">
          <button
            v-for="o in b.options"
            :key="o"
            role="radio"
            :aria-checked="values[b.id].text === o"
            :class="{ on: values[b.id].text === o }"
            @click="values[b.id].text = o"
          >{{ o }}</button>
        </div>

        <button
          v-else-if="b.type === 'describe'"
          class="describe"
          :disabled="describeAction(b, values, way, !!reading).kind === 'missing' || !!reading"
          :title="describeAction(b, values, way, !!reading).why ?? ''"
          @click="describe(b)"
        >✦ {{ describeAction(b, values, way, reading === b.id).label }}</button>
      </template>
    </div>

    <button class="primary create" :disabled="action.kind === 'checking' || action.kind === 'busy'" :title="action.why ?? ''" @click="create">
      ✦ {{ action.label }}
    </button>
    <p v-if="problem" role="alert" class="problem">{{ problem }}</p>
    <p v-else-if="done" class="hint">{{ done }}</p>
  </section>
</template>

<style scoped>
.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 0;
  overflow-y: auto;
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}
h4 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
}
.fields {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.label {
  display: block;
  margin-bottom: 4px;
  font-size: 13px;
  color: var(--text-dim);
}
.upload {
  display: grid;
  place-items: center;
  min-height: 120px;
  padding: 0;
  overflow: hidden;
  border: 1px dashed var(--line);
  border-radius: 12px;
  background: color-mix(in srgb, var(--panel-background) 60%, transparent);
  color: var(--text);
  font: inherit;
  cursor: pointer;
}
.upload.filled {
  border-style: solid;
}
.upload img {
  display: block;
  width: 100%;
  max-height: 220px;
  object-fit: contain;
}
.upload-word {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  font-size: 14px;
}
.big {
  font-size: 26px;
  color: var(--accent);
}
.character {
  display: flex;
  align-items: center;
  gap: 12px;
}
.portrait {
  display: grid;
  place-items: center;
  width: 84px;
  height: 84px;
  flex-shrink: 0;
  padding: 0;
  overflow: hidden;
  border: 1px dashed var(--line);
  border-radius: 12px;
  background: color-mix(in srgb, var(--panel-background) 60%, transparent);
  color: var(--accent);
  font-size: 22px;
  cursor: pointer;
}
.portrait.filled {
  border-style: solid;
}
.portrait img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.character label {
  flex: 1;
}
input,
textarea {
  width: 100%;
  box-sizing: border-box;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 14px;
}
textarea {
  resize: vertical;
  min-height: 80px;
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.chips button {
  border-radius: 999px;
  padding: 6px 14px;
}
.chips .on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 85%, transparent);
  color: #07142e;
  font-weight: 600;
}
.describe {
  padding: 10px 12px;
  border-color: color-mix(in srgb, var(--accent) 60%, var(--line));
  background: color-mix(in srgb, var(--accent) 16%, transparent);
  font-size: 14px;
}
.create {
  padding: 12px;
  font-size: 15px;
}
.problem {
  margin: 0;
  font-size: 13px;
  color: var(--warn);
}
.hint {
  margin: 0;
  font-size: 13px;
  color: var(--text-dim);
}
button {
  padding: 6px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
button.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #07142e;
  font-weight: 600;
}
button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
</style>
