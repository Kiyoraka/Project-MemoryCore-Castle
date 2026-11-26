<script setup>
// SCRIPTORIUM's room shell: the editors (Scroll, Ledger, Stage, Folio), the page and the AI's changes, in
// the Classic or the Studio layout. It knows no host: the host passes `api`, an object of async functions
// over its own edge to the `scriptorium` crate, and its chat panel in the #chat slot.
//
//   api.newDoc(kind)              -> doc id                 kind: 'scroll' | 'ledger' | 'stage' | 'folio'
//   api.display(doc, page)        -> display list JSON      (DisplayList::to_json)
//   api.glyphs(doc, font, gids)   -> glyphs JSON            api.image(doc, id) -> image JSON
//   api.command(doc, name, args)  -> { ok, step, pages }    the member's own command (one undo step)
//   api.undo(doc), api.redo(doc)  -> { ok, step, pages }    api.history(doc) -> [{ id, origin, label, undone }]
//   api.open(path?)               -> { doc, editor } | null (no path: the host's picker)
//   api.save(doc, name)           -> anything | null        (the host's picker, then the file)
//   api.recent()                  -> [{ path, name, editor }]
//   api.onChanged(handler)        -> stop function          (optional) handler({ doc }) when the AI changed one
import { computed, markRaw, onBeforeUnmount, onMounted, reactive, ref, shallowRef } from 'vue'
import ClassicLayout from './layout/ClassicLayout.vue'
import StudioLayout from './layout/StudioLayout.vue'
import ScriptoriumPage from './ScriptoriumPage.vue'
import { EARLY_BADGE, aiPills, editorOf, isTypingTarget, layoutOf, plainError, readHistory, readList, recentRows, saveName, shortcutOf, titleFor, undoState } from './room.js'
import { icon } from './icons.js'

const props = defineProps({
  layout: { type: String, default: 'classic' },
  api: { type: Object, required: true },
  // how the AI's changes are named in the strip ("Lyra's change")
  aiName: { type: String, default: 'AI' },
})

const editor = ref('scroll')
// each editor's open document, so switching back finds it as it was
const docs = reactive({})
const doc = ref(null)
const list = shallowRef(null)
const history = ref([])
const recent = ref([])
const status = ref('')
const busy = ref(false)
const zoom = ref(1)
let stopChanged = null

const Layout = computed(() => (layoutOf(props.layout) === 'studio' ? StudioLayout : ClassicLayout))
const undo = computed(() => undoState(history.value))
const pills = computed(() => aiPills(history.value))
const title = computed(() => titleFor(editor.value))
// the painter asks for outlines and pixels of the document on the page
const loadGlyphs = computed(() => {
  const d = doc.value
  return (font, gids) => props.api.glyphs(d, font, gids)
})
const loadImage = computed(() => {
  const d = doc.value
  return (id) => props.api.image(d, id)
})

async function step(work) {
  busy.value = true
  try {
    await work()
  } catch (reason) {
    status.value = plainError(reason)
  } finally {
    busy.value = false
  }
}

async function refresh() {
  if (doc.value === null) return
  const d = doc.value
  const [shown, steps] = await Promise.all([props.api.display(d, 0), props.api.history(d)])
  if (d !== doc.value) return
  const next = readList(shown)
  list.value = next ? markRaw(next) : null
  history.value = readHistory(steps)
}

async function show(id) {
  const kind = editorOf(id).id
  editor.value = kind
  if (docs[kind] === undefined) docs[kind] = await props.api.newDoc(kind)
  doc.value = docs[kind]
  await refresh()
}

const switchEditor = (id) => step(async () => {
  status.value = ''
  await show(id)
})

const newDoc = () => step(async () => {
  status.value = ''
  docs[editor.value] = await props.api.newDoc(editor.value)
  doc.value = docs[editor.value]
  await refresh()
})

const open = (path) => step(async () => {
  status.value = ''
  const opened = await props.api.open(path)
  if (!opened) return
  const kind = editorOf(opened.editor).id
  docs[kind] = opened.doc
  editor.value = kind
  doc.value = opened.doc
  await refresh()
  await loadRecent()
})

const save = () => step(async () => {
  status.value = ''
  if (doc.value === null) return
  await props.api.save(doc.value, saveName(editor.value))
})

const undoStep = () => step(async () => {
  if (doc.value === null || !undo.value.canUndo) return
  status.value = ''
  await props.api.undo(doc.value)
  await refresh()
})

const redoStep = () => step(async () => {
  if (doc.value === null || !undo.value.canRedo) return
  status.value = ''
  await props.api.redo(doc.value)
  await refresh()
})

async function loadRecent() {
  if (typeof props.api.recent !== 'function') return
  recent.value = recentRows(await props.api.recent().catch(() => []))
}

function onKey(e) {
  if (isTypingTarget(e.target)) return
  const what = shortcutOf(e)
  if (!what) return
  e.preventDefault()
  if (what === 'undo') undoStep()
  else redoStep()
}

onMounted(async () => {
  if (typeof props.api.onChanged === 'function') {
    stopChanged = await props.api.onChanged((payload) => {
      if (payload && payload.doc === doc.value) refresh().catch(() => {})
    })
  }
  await step(() => show('scroll'))
  await loadRecent()
})

onBeforeUnmount(() => {
  if (typeof stopChanged === 'function') stopChanged()
})

defineExpose({ refresh, show: switchEditor })
</script>

<template>
  <div class="scriptorium" @keydown="onKey">
    <component
      :is="Layout"
      :title="title"
      :editor="editor"
      :badge="EARLY_BADGE"
      :can-undo="undo.canUndo"
      :can-redo="undo.canRedo"
      :busy="busy"
      :status="status"
      :recent="recent"
      @editor="switchEditor"
      @new="newDoc"
      @open="open()"
      @save="save"
      @undo="undoStep"
      @redo="redoStep"
      @recent="open"
    >
      <template #page>
        <div class="page-focus" tabindex="0" aria-label="The page">
          <ScriptoriumPage v-model:zoom="zoom" :list="list" :load-glyphs="loadGlyphs" :load-image="loadImage" />
        </div>
      </template>
      <template #strip>
        <div v-if="pills.length" class="ai-strip" role="list" :aria-label="`${aiName}'s changes`">
          <span v-for="p in pills" :key="p.id" class="pill" role="listitem">
            <span class="spark" v-html="icon('spark', 14)"></span>
            <span class="what">{{ aiName }}'s change: {{ p.label }}</span>
            <button :disabled="!p.undoable || busy" :title="p.undoable ? 'Undo this change' : 'Undo the newer changes first'" @click="undoStep">Undo</button>
          </span>
        </div>
      </template>
      <template #chat>
        <slot name="chat" />
      </template>
    </component>
  </div>
</template>

<style scoped>
.scriptorium {
  --scr-accent: var(--accent, #8b7bff);
  --scr-text: var(--text, #ece9ff);
  --scr-muted: color-mix(in srgb, var(--scr-text) 62%, transparent);
  --scr-line: color-mix(in srgb, var(--scr-accent) 26%, transparent);
  --scr-panel: color-mix(in srgb, var(--scr-accent) 9%, rgba(14, 12, 40, 0.72));
  --scr-desk: rgba(10, 9, 30, 0.55);
  --scr-chip: color-mix(in srgb, var(--scr-accent) 28%, transparent);
  --scr-hover: rgba(255, 255, 255, 0.07);
  height: 100%;
  min-height: 0;
  background: rgba(16, 13, 44, 0.82);
}
.page-focus {
  position: absolute;
  inset: 0;
  outline: none;
}
.ai-strip {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 6px;
}
.pill {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  max-width: 100%;
  padding: 5px 6px 5px 12px;
  border: 1px solid var(--scr-line);
  border-radius: 999px;
  background: var(--scr-panel);
  color: var(--scr-text);
  font-size: 13px;
  backdrop-filter: blur(8px);
}
.pill .spark {
  display: inline-flex;
  color: var(--scr-accent);
}
.pill .what {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pill button {
  padding: 3px 10px;
  border: 0;
  border-radius: 999px;
  background: var(--scr-chip);
  color: var(--scr-text);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}
.pill button:disabled {
  opacity: 0.45;
  cursor: default;
}
</style>
