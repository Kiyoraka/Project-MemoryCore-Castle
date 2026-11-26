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
//
// The editing ground (all optional; without them the page shows no caret). A pos is the editor's own
// place, a selection { anchor, focus }:
//   api.hit(doc, page, x, y)      -> pos | null             the place under a page point
//   api.caret(doc, pos)           -> { page, x, y, height } | null
//   api.selection(doc, sel)       -> [{ page, x, y, w, h }]
//   api.state(doc, sel)           -> the editor's state     the ribbon's pressed buttons (stateKey)
//   api.copy(doc, sel)            -> { text, rich }         api.writeClipboard(text, rich) puts it on the clipboard
//   api.move(doc, pos, motion)    -> pos | null             left, right, up, down, word_left, word_right,
//                                                           line_start, line_end, doc_start, doc_end
//   api.exportPdf(doc)            -> path | null            the host's save picker, then every page as a PDF
//   api.print(doc)                -> words                  the document handed to the system's printing
import { computed, markRaw, onBeforeUnmount, onMounted, reactive, ref, shallowRef } from 'vue'
import FindBar from './FindBar.vue'
import ClassicLayout from './layout/ClassicLayout.vue'
import StudioLayout from './layout/StudioLayout.vue'
import PageNav from './PageNav.vue'
import ScriptoriumPage from './ScriptoriumPage.vue'
import { isPageInput } from './input.js'
import { FIND_SOON, buttonCommand, commandNames, editorUi, findSupport, intentFor, notYet } from './ribbon.js'
import { extendTo, isCollapsed, motionOf, readCaret, readClip, readPos, readRects, readSelection, selectionAt } from './selection.js'
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
  readDone,
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

// the editing ground: the selection kept here, the caret and boxes it paints, the ribbon's state
const sel = shallowRef(null)
const caret = shallowRef(null)
const rects = shallowRef([])
const editState = shallowRef({})
// the open document's command names (describe) and whether it takes a caret
const commands = shallowRef([])
const editable = ref(false)
const findOpen = ref(false)
const findReplace = ref(false)
const pageView = ref(null)
// the last copy, so a paste back into the same editor keeps its rich form
let lastClip = null
const has = (name) => typeof props.api[name] === 'function'
const ui = computed(() => editorUi(editor.value, commands.value))
const finding = computed(() => findSupport(editor.value, commands.value))

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
  commands.value = commandNames(described)
  editable.value = described?.editable === true
  if (!editable.value) clearSelection()
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
  clearSelection()
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
  if (doc.value !== docs[kind]) clearSelection()
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
  await refreshSelection()
})

const redoStep = () => step(async () => {
  if (doc.value === null || !undo.value.canRedo) return
  status.value = ''
  await props.api.redo(doc.value)
  await refresh()
  await refreshSelection()
})

async function loadRecent() {
  if (typeof props.api.recent !== 'function') return
  recent.value = recentRows(await props.api.recent().catch(() => []))
}

// --- export and print ---

const exportPdf = () => step(async () => {
  if (doc.value === null || !has('exportPdf')) return
  status.value = ''
  const where = await props.api.exportPdf(doc.value)
  if (where) status.value = `Exported ${fileNameOf(where)}.`
})

const print = () => step(async () => {
  if (doc.value === null || !has('print')) return
  status.value = 'Making it ready for the printer…'
  const said = await props.api.print(doc.value)
  status.value = typeof said === 'string' && said ? said : 'Sent to the printer.'
})

// --- the caret and the selection ---

function clearSelection() {
  sel.value = null
  caret.value = null
  rects.value = []
  editState.value = {}
}

// one at a time: each intent reads the selection the one before it left
let queue = Promise.resolve()
function enqueue(work) {
  queue = queue.then(work).catch((reason) => {
    status.value = plainError(reason)
  })
  return queue
}

// the caret, the boxes and the ribbon's state for the selection; `follow` turns to the caret's page
async function refreshSelection(follow = false) {
  const d = doc.value
  const s = sel.value
  if (d === null || !s || !has('caret')) {
    if (!s) clearSelection()
    return
  }
  const [c, r, st] = await Promise.all([
    props.api.caret(d, s.focus),
    isCollapsed(s) || !has('selection') ? [] : props.api.selection(d, s),
    has('state') ? props.api.state(d, s) : {},
  ])
  if (d !== doc.value || s !== sel.value) return
  const box = readCaret(c)
  // the place is gone (an undo took it away)
  if (!box) return clearSelection()
  caret.value = box
  rects.value = readRects(r)
  const state = parseJson(st)
  editState.value = state && typeof state === 'object' ? state : {}
  if (follow && box.page !== page.value) {
    const next = clampPage(box.page, nav.value.count)
    if (next !== page.value) {
      page.value = next
      if (files[d]) files[d].page = next
      await paint()
    }
  }
}

let dragging = false
let dragAt = null
function onPointer(p) {
  if (!p || doc.value === null || !editable.value || !has('hit')) return
  if (p.type === 'down') {
    if (p.button !== 0 || !p.inside) return
    dragging = true
    const at = { x: p.x, y: p.y, shift: !!p.shiftKey }
    enqueue(async () => {
      const pos = readPos(await props.api.hit(doc.value, page.value, at.x, at.y))
      if (!pos) {
        if (!at.shift) clearSelection()
        return
      }
      sel.value = at.shift && sel.value ? extendTo(sel.value, pos) : selectionAt(pos)
      await refreshSelection()
    })
  } else if (p.type === 'move') {
    if (!dragging || !(p.buttons & 1)) return
    const waiting = dragAt !== null
    dragAt = { x: p.x, y: p.y }
    if (waiting) return
    enqueue(async () => {
      const at = dragAt
      dragAt = null
      if (!at || !sel.value) return
      const pos = readPos(await props.api.hit(doc.value, page.value, at.x, at.y))
      if (!pos) return
      sel.value = extendTo(sel.value, pos)
      await refreshSelection()
    })
  } else if (p.type === 'up' || p.type === 'leave') {
    dragging = false
  }
}

// the member's command on the document, with the selection; the answer's selection is where it landed
async function runCommand(name, args) {
  const d = doc.value
  if (d === null || !name) return null
  busy.value = true
  try {
    const done = readDone(await props.api.command(d, name, args))
    if (done.ok === false) {
      status.value = plainError(done.error)
      return done
    }
    const landed = readSelection(done.selection)
    if (landed) sel.value = landed
    await refresh()
    await refreshSelection(true)
    return done
  } finally {
    busy.value = false
  }
}

const withSel = (args) => (sel.value ? { ...args, sel: sel.value } : { ...args })

async function moveCaret(motion, extend) {
  const s = sel.value
  if (!s || !has('move')) return
  const pos = readPos(await props.api.move(doc.value, s.focus, motion))
  if (!pos) {
    if (!extend && !isCollapsed(s)) {
      sel.value = selectionAt(s.focus)
      await refreshSelection(true)
    }
    return
  }
  sel.value = extend ? extendTo(s, pos) : selectionAt(pos)
  await refreshSelection(true)
}

async function selectAll() {
  const s = sel.value
  if (!s || !has('move')) return
  const d = doc.value
  const [a, b] = await Promise.all([props.api.move(d, s.focus, 'doc_start'), props.api.move(d, s.focus, 'doc_end')])
  sel.value = { anchor: readPos(a) ?? s.focus, focus: readPos(b) ?? s.focus }
  await refreshSelection()
}

async function copySelection() {
  const s = sel.value
  if (!s || isCollapsed(s) || !has('copy')) return false
  const clip = readClip(await props.api.copy(doc.value, s))
  if (!clip.text && !clip.rich) return false
  lastClip = clip
  if (has('writeClipboard')) await props.api.writeClipboard(clip.text, clip.rich)
  return true
}

async function runIntent({ command, args = {} }) {
  if (doc.value === null) return
  const own = intentFor(editor.value, command)
  const motion = motionOf(command)
  if (motion && !own) return moveCaret(motion, args.extend === true)
  if (command === 'select.all' && !own) return selectAll()
  if (command === 'clip.copy') return copySelection()
  if (command === 'clip.cut') {
    if (!(await copySelection())) return
    const cut = own ?? intentFor(editor.value, 'edit.delete')
    if (!cut) return void (status.value = notYet(editor.value, command))
    return runCommand(cut.command, withSel({ ...cut.args }))
  }
  if (!own) return void (status.value = notYet(editor.value, command))
  const extra = { ...args }
  // a paste of our own last copy keeps its rich form
  if (command === 'clip.paste' && lastClip?.rich && lastClip.text === extra.text) extra.rich = lastClip.rich
  return runCommand(own.command, withSel({ ...own.args, ...extra }))
}

function openFind(replace) {
  findOpen.value = true
  findReplace.value = replace
}

function closeFind() {
  findOpen.value = false
  pageView.value?.focusInput?.()
}

// what the room does itself; every other intent goes to the editor (in order, through the queue)
const ROOM_INTENTS = {
  'history.undo': () => undoStep(),
  'history.redo': () => redoStep(),
  'file.save': () => save(false),
  'file.save_as': () => save(true),
  'file.open': () => open(),
  'file.print': () => print(),
  'find.open': () => openFind(false),
  'find.replace': () => openFind(true),
}

function onIntent(it) {
  if (!it || !it.command) return
  const mine = ROOM_INTENTS[it.command]
  if (mine) {
    if (!busy.value) mine()
    return
  }
  enqueue(() => runIntent(it))
}

function onButton(b) {
  if (!b) return
  if (b.intent) onIntent({ command: b.intent, args: b.args })
  else
    enqueue(async () => {
      const c = buttonCommand(b, sel.value)
      await runCommand(c.command, c.args)
    })
  pageView.value?.focusInput?.()
}

function onFind({ query, next }) {
  enqueue(async () => {
    if (!finding.value.find) return void (status.value = FIND_SOON)
    const done = await runCommand(finding.value.findCommand, withSel({ query, next }))
    if (!done || done.ok === false) return
    status.value = readSelection(done.selection) ? '' : `"${query}" was not found.`
  })
}

function onReplace({ query, with: replacement, all }) {
  enqueue(async () => {
    if (!finding.value.replace) return
    await runCommand(finding.value.replaceCommand, withSel({ query, with: replacement, all }))
  })
}

const SHORTCUTS = { undo: () => undoStep(), redo: () => redoStep(), open: () => open(), save: () => save(false), saveAs: () => save(true) }

function onKey(e) {
  // the page's own keyboard sends its intents itself; what it lets through (page keys) is ours
  if (isTypingTarget(e.target) && !isPageInput(e.target)) return
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
      :ui="ui"
      :edit-state="editState"
      :can-export="has('exportPdf')"
      :can-print="has('print')"
      @editor="switchEditor"
      @new="newDoc"
      @open="open()"
      @save="save(false)"
      @save-as="save(true)"
      @undo="undoStep"
      @redo="redoStep"
      @recent="open"
      @button="onButton"
      @export-pdf="exportPdf"
      @print="print"
      @find="openFind(false)"
    >
      <template #page>
        <PageNav :nav="nav" :page="page" :busy="busy" @page="turn">
          <div class="page-focus" tabindex="0" aria-label="The page" @focus="pageView?.focusInput?.()">
            <ScriptoriumPage
              ref="pageView"
              v-model:zoom="zoom"
              :list="list"
              :load-glyphs="loadGlyphs"
              :load-image="loadImage"
              :page="page"
              :caret="caret"
              :rects="rects"
              @pointer="onPointer"
              @intent="onIntent"
            />
            <FindBar v-model:replace="findReplace" :open="findOpen" :support="finding" :busy="busy" @find="onFind" @replace="onReplace" @close="closeFind" />
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
