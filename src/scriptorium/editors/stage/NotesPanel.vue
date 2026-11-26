<script setup>
// Stage's notes pane: the speaker notes of one slide, typed in a plain box and written back as one
// bus command (stage.set_notes) when the box is left or Ctrl+Enter is pressed, so it is an ordinary
// undo step. It takes the room's api (describe, command) and the document; nothing of any host.
import { onMounted, ref, watch } from 'vue'

const props = defineProps({
  api: { type: Object, required: true },
  doc: { type: [Number, String], required: true },
  slide: { type: Number, default: 0 },
})
const emit = defineEmits(['changed'])

const text = ref('')
const saved = ref('')
const busy = ref(false)

async function load() {
  if (typeof props.api.describe !== 'function') return
  let d = await props.api.describe(props.doc).catch(() => null)
  if (typeof d === 'string') {
    try {
      d = JSON.parse(d)
    } catch {
      d = null
    }
  }
  const notes = d?.document?.slides?.[props.slide]?.notes ?? d?.slides?.[props.slide]?.notes ?? ''
  text.value = typeof notes === 'string' ? notes : ''
  saved.value = text.value
}

async function save() {
  if (text.value === saved.value || busy.value) return
  busy.value = true
  try {
    await props.api.command(props.doc, 'stage.set_notes', { slide: props.slide, text: text.value })
    saved.value = text.value
    emit('changed')
  } finally {
    busy.value = false
  }
}

function onKey(e) {
  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
    e.preventDefault()
    save()
  }
  e.stopPropagation()
}

watch(() => [props.doc, props.slide], load)
onMounted(load)
</script>

<template>
  <div class="notes">
    <textarea v-model="text" class="box" placeholder="Click to add notes" aria-label="Speaker notes" @blur="save" @keydown="onKey"></textarea>
  </div>
</template>

<style scoped>
.notes {
  display: flex;
  min-height: 72px;
}
.box {
  flex: 1;
  min-height: 72px;
  padding: 8px 10px;
  border: 1px solid color-mix(in srgb, currentColor 22%, transparent);
  border-radius: 6px;
  background: transparent;
  color: inherit;
  font: 13px/1.45 system-ui, sans-serif;
  resize: vertical;
}
</style>
