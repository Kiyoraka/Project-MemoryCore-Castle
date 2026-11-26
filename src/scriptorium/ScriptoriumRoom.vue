<script setup>
// SCRIPTORIUM's room shell: the editors (Scroll, Ledger, Stage, Folio), the page and the AI's changes, in
// the Classic or the Studio layout. It knows no host: the host passes `api`, an object of async functions
// over its own edge to the `scriptorium` crate, and its chat panel in the #chat slot.
//
//   api.newDoc(kind)              -> doc id                 kind: 'scroll' | 'ledger' | 'stage' | 'folio'
//   api.closeDoc(doc)             -> anything               (optional) the page let it go
//   api.describe(doc)             -> describe JSON          (optional) the pages, sheets or slides
//   api.display(doc, page)        -> display list JSON      (DisplayList::to_json)
//   api.glyphs(doc, font, gids)   -> glyphs JSON            api.image(doc, id) -> image JSON
//   api.command(doc, name, args)  -> { ok, step, pages }    the member's own command (one undo step)
//   api.undo(doc), api.redo(doc)  -> { ok, step, pages }    api.history(doc) -> [{ id, origin, label, undone }]
//   api.pick()                    -> path | null            the host's open picker
//   api.open(path)                -> { doc, editor, pages, title, path }
//   api.pickSave(name)            -> path | null            the host's save picker
//   api.save(doc, path | null)    -> the path saved to      (null: back to the file it came from)
//   api.recent()                  -> [{ path, name, editor }]
//   api.onChanged(handler)        -> stop function          (optional) handler({ doc }) when the AI changed one
import { computed, markRaw, onBeforeUnmount, onMounted, reactive, ref, shallowRef } from 'vue'
import ClassicLayout from './layout/ClassicLayout.vue'
import StudioLayout from './layout/StudioLayout.vue'
import PageNav from './PageNav.vue'
import ScriptoriumPage from './ScriptoriumPage.vue'
import {
  EARLY_BADGE,
  aiPills,
  clampPage,
  editorForPath,
  editorOf,
  fileNameOf,
  isDirty,
  isTypingTarget,
  layoutOf,
  newestDone,
  pageKeyOf,
  pageNav,
  parseJson,
  plainError,
  readHistory,
  readList,
  recentRows,
  saveAsName,
  shortcutOf,
  titleFor,
  undoState,
} from './room.js'
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
// per document: { title, path, mark (the newest done step when opened or saved), dirty, page }
const files = reactive({})
const doc = ref(null)
const page = ref(0)
const nav = shallowRef({ kind: 'pages', count: 1 })
const list = shallowRef(null)
const history = ref([])
const recent = ref([])
const status = ref('')
const busy = ref(false)
const zoom = ref(1)
// an action waiting for its second press (it would let unsaved changes go): { action, path }
const armed = ref(null)
let stopChanged = null

const Layout = computed(() => (layoutOf(props.layout) === 'studio' ? StudioLayout : ClassicLayout))
const undo = computed(() => undoState(history.value))
const pills = computed(() => aiPills(history.value))
const file = computed(() => {
  const f = files[doc.value]
  return f ? `${f.title}${f.dirty ? ' •' : ''}` : ''
})
const title = computed(() => titleFor(editor.value, file.value))
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

async function describe(d) {
  if (typeof props.api.describe !== 'function') return null
  return parseJson(await props.api.describe(d).catch(() => null))
}

async function paint() {
  if (doc.value === null) return
  const d = doc.value
  const shown = await props.api.display(d, page.value)
  if (d !== doc.value) return
  const next = readList(shown)
  list.value = next ? markRaw(next) : null
}

// the document on the page read again: its pages, its history and the page shown
async function refresh() {
  if (doc.value === null) return
  const d = doc.value
  const [described, steps] = await Promise.all([describe(d), props.api.history(d)])
  if (d !== doc.value) return
  nav.value = pageNav(editor.value, described)
  const f = files[d]
  // a workbook opens on the sheet it was saved on
  const first = f && f.page === undefined && editor.value === 'ledger' ? Number(described?.active_sheet) || 0 : f?.page ?? 0
  page.value = clampPage(first, nav.value.count)
  if (f) f.page = page.value
  history.value = readHistory(steps)
  if (f) f.dirty = isDirty(history.value, f.mark)
  await paint()
}

const turn = (p) => step(async () => {
  if (doc.value === null) return
  const next = clampPage(p, nav.value.count)
  if (next === page.value) return
  page.value = next
  if (files[doc.value]) files[doc.value].page = next
  await paint()
})

// a new or opened document takes its editor's place; the one before is let go
function place(kind, id, info) {
  const old = docs[kind]
  if (old !== undefined && old !== id) {
    delete files[old]
    if (typeof props.api.closeDoc === 'function') Promise.resolve(props.api.closeDoc(old)).catch(() => {})
  }
  docs[kind] = id
  files[id] = { title: info.title || 'Untitled', path: info.path ?? null, mark: null, dirty: false, page: undefined }
  editor.value = kind
  doc.value = id
}

// true when the action may go on: nothing unsaved would be lost, or this is its second press
function mayLeave(kind, action, path = null) {
  const id = docs[kind]
  const f = id === undefined ? null : files[id]
  const again = armed.value && armed.value.action === action && armed.value.path === path
  if (!f || !f.dirty || again) {
    armed.value = null
    return true
  }
  armed.value = { action, path }
  const what = action === 'new' ? 'New' : path ? `Open ${fileNameOf(path)}` : 'Open'
  status.value = `${f.title} has unsaved changes. Save it first, or press ${what} again to let them go.`
  return false
}

async function show(id) {
  const kind = editorOf(id).id
  editor.value = kind
  if (docs[kind] === undefined) place(kind, await props.api.newDoc(kind), { title: 'Untitled' })
  doc.value = docs[kind]
  await refresh()
}

const switchEditor = (id) => step(async () => {
  status.value = ''
  await show(id)
})

const newDoc = () => step(async () => {
  const kind = editor.value
  if (!mayLeave(kind, 'new')) return
  status.value = ''
  place(kind, await props.api.newDoc(kind), { title: 'Untitled' })
  await refresh()
})

// Open: the host's picker (no path) or a recent file; the file goes to its own editor
const open = (path) => step(async () => {
  let target = path ?? (armed.value?.action === 'open' ? armed.value.path : null)
  if (!target) {
    if (typeof props.api.pick !== 'function') return
    target = await props.api.pick()
    if (!target) return
  }
  const kind = editorForPath(target)
  if (kind && !mayLeave(kind, 'open', target)) return
  status.value = `Opening ${fileNameOf(target)}…`
  const opened = await props.api.open(target)
  status.value = ''
  if (!opened) return
  place(editorOf(opened.editor).id, opened.doc, { title: opened.title || fileNameOf(opened.path ?? target), path: opened.path ?? target })
  await refresh()
  await loadRecent()
})

// Save goes back to the file it came from; Save as (and a new document's first save) asks where
const save = (as = false) => step(async () => {
  if (doc.value === null) return
  armed.value = null
  const d = doc.value
  const f = files[d]
  let target = null
  if (as || !f?.path) {
    if (typeof props.api.pickSave !== 'function') return
    target = await props.api.pickSave(saveAsName(editor.value, f?.title))
    if (!target) return
  }
  status.value = 'Saving…'
  const saved = await props.api.save(d, target)
  const where = typeof saved === 'string' && saved ? saved : target ?? f?.path
  if (f) {
    if (where) {
      f.path = where
      f.title = fileNameOf(where)
    }
    f.mark = newestDone(history.value)
    f.dirty = false
  }
  status.value = `Saved ${f?.title ?? ''}.`.replace(' .', '.')
  await loadRecent()
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

const SHORTCUTS = { undo: () => undoStep(), redo: () => redoStep(), open: () => open(), save: () => save(false), saveAs: () => save(true) }

function onKey(e) {
  if (isTypingTarget(e.target)) return
  const what = shortcutOf(e)
  if (what) {
    e.preventDefault()
    if (!busy.value) SHORTCUTS[what]()
    return
  }
  const delta = pageKeyOf(e)
  if (delta && !busy.value && nav.value.count > 1) {
    e.preventDefault()
    turn(page.value + delta)
  }
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

defineExpose({ refresh, show: switchEditor, open })
</script>

<template>
  <div class="scriptorium" @keydown="onKey">
    <component
      :is="Layout"
      :title="title"
      :file="file"
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
      @save="save(false)"
      @save-as="save(true)"
      @undo="undoStep"
      @redo="redoStep"
      @recent="open"
    >
      <template #page>
        <PageNav :nav="nav" :page="page" :busy="busy" @page="turn">
          <div class="page-focus" tabindex="0" aria-label="The page">
            <ScriptoriumPage v-model:zoom="zoom" :list="list" :load-glyphs="loadGlyphs" :load-image="loadImage" />
          </div>
        </PageNav>
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
