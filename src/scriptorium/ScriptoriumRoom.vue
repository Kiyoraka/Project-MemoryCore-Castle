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
//   api.open(path, password?)     -> { doc, editor, pages, title, path }
//                                                           a locked PDF fails with the crate's words
//                                                           (room.js pdfLock); the room asks for the
//                                                           password in its own dialog and tries again
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
//   api.read(doc, what)           -> read JSON              (optional) the door's read (Folio's outline)
//   api.openUrl(uri)              -> anything               (optional) a web link opened in the system's
//                                                           browser, after the member said yes
//
// An editor may bring page-side work (`actions` in its index.js, run for a button's intent or command
// with { api, doc, page, count, sel, args, pickFile }) and components (`views`): a full-screen one in
// the room's overlay, a panel under the page or beside it (ribbon.js editorViews). Intents that ask the
// member first (Scroll's insert.picture / insert.link) get the room's file picker or its prompt.
import { computed, defineAsyncComponent, markRaw, nextTick, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue'
import FindBar from './FindBar.vue'
import ClassicLayout from './layout/ClassicLayout.vue'
import StudioLayout from './layout/StudioLayout.vue'
import PageNav from './PageNav.vue'
import ScriptoriumPage from './ScriptoriumPage.vue'
import { isPageInput } from './input.js'
import { FIND_SOON, actionFor, askingFor, buttonCommand, commandNames, editorUi, editorViews, findSupport, intentFor, notYet, overlayFor, viewProps, viewToggle } from './ribbon.js'
import { extendTo, isCollapsed, motionOf, readCaret, readClip, readPos, readRects, readSelection, selectionAt } from './selection.js'
import {
  EARLY_BADGE,
  aiPills,
  clampPage,
  editorForPath,
  editorOf,
  fileNameOf,
  fitZoomFor,
  isDirty,
  isTypingTarget,
  layoutOf,
  linkTarget,
  newestDone,
  pageKeyOf,
  pageNav,
  pageSizeOf,
  parseJson,
  passwordPrompt,
  pdfLock,
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
// every document opens at its own size: a zoom chosen for another one (or fitted while the page area
// was squeezed) never carries over
watch(doc, () => {
  zoom.value = 1
})
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

// the room's own small dialog (a question, a line of text or a password):
// { kind: 'confirm' | 'text' | 'password', title, message, detail?, ok, wrong?, placeholder?, resolve }
const dialog = shallowRef(null)
const dialogText = ref('')
const dialogInput = ref(null)

// an editor's views: the full-screen one on show, the panels switched on, Folio's outline
const overlay = shallowRef(null)
const panelsOn = reactive({})
const outline = shallowRef([])
const loaded = new Map()
function viewComponent(v) {
  const key = `${editor.value}:${v.name}`
  if (!loaded.has(key)) loaded.set(key, markRaw(defineAsyncComponent(v.load)))
  return loaded.get(key)
}
const views = computed(() => editorViews(editor.value))
// a panel beside the page is on until switched off (the bookmarks only when there are some); one under
// the page waits for its button
function panelOn(v) {
  const set = panelsOn[`${editor.value}:${v.name}`]
  if (set !== undefined) return set
  if (v.place !== 'side') return false
  return v.name !== 'bookmarks' || (Array.isArray(outline.value) && outline.value.length > 0)
}
function placed(place) {
  if (doc.value === null) return null
  const v = views.value.find((x) => x.place === place && panelOn(x))
  if (!v) return null
  return { name: v.name, comp: viewComponent(v), props: viewProps(v.name, { api: props.api, doc: doc.value, page: page.value, count: nav.value.count, outline: outline.value }) }
}
const belowView = computed(() => placed('below'))
const sideView = computed(() => placed('side'))
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
  const wantsOutline = has('read') && editorViews(editor.value).some((v) => v.name === 'bookmarks')
  const [described, steps, tree] = await Promise.all([
    describe(d),
    props.api.history(d),
    wantsOutline ? Promise.resolve(props.api.read(d, { outline: true })).catch(() => null) : null,
  ])
  if (d !== doc.value) return
  const read = parseJson(tree)
  outline.value = Array.isArray(read?.outline) ? read.outline : Array.isArray(read) ? read : []
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
  const opened = await openLocked(target)
  status.value = ''
  if (!opened) return
  place(editorOf(opened.editor).id, opened.doc, { title: opened.title || fileNameOf(opened.path ?? target), path: opened.path ?? target })
  await refresh()
  await loadRecent()
})

// a file opened, and a locked PDF's password asked for (again after a wrong one) until it opens or
// the member gives up (null). The password lives only in this call: never logged, never kept.
async function openLocked(target) {
  let password = null
  for (;;) {
    try {
      return await (password === null ? props.api.open(target) : props.api.open(target, password))
    } catch (reason) {
      const lock = pdfLock(reason)
      if (!lock) throw reason
      status.value = ''
      password = await ask({ kind: 'password', ...passwordPrompt(password === null ? lock : 'wrong', fileNameOf(target)), ok: 'Open' })
      if (password === null) return null
      status.value = `Opening ${fileNameOf(target)}…`
    }
  }
}

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
    // the editor decides: a single cell or a single shape has boxes though it is collapsed
    has('selection') ? props.api.selection(d, s) : [],
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
  // collapsed is the editor's to judge (one cell, one shape copies); an empty clip is nothing to copy
  if (!s || !has('copy')) return false
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
  if (!own) {
    const asking = askingFor(editor.value, command)
    if (asking) return runAsking(asking)
    const act = actionFor(editor.value, command)
    if (act) return runAction(act, args)
    return void (status.value = notYet(editor.value, command))
  }
  const extra = { ...args }
  // a paste of our own last copy keeps its rich form
  if (command === 'clip.paste' && lastClip?.rich && lastClip.text === extra.text) extra.rich = lastClip.rich
  return runCommand(own.command, withSel({ ...own.args, ...extra }))
}

// a file chosen on the page (one picker for every editor): a File, or null when none was chosen
function pickFile(accept = '') {
  return new Promise((resolve) => {
    if (typeof document === 'undefined') return resolve(null)
    const input = document.createElement('input')
    input.type = 'file'
    if (accept) input.accept = accept
    let settled = false
    const done = (file) => {
      if (settled) return
      settled = true
      window.removeEventListener('focus', back)
      resolve(file)
    }
    // the picker closed without a file: the window gets its focus back and no change follows
    const back = () => setTimeout(() => done(input.files?.[0] ?? null), 600)
    input.onchange = () => done(input.files?.[0] ?? null)
    window.addEventListener('focus', back)
    input.click()
  })
}

// an intent that asks the member first (a picture file, a link's address), then runs its command
async function runAsking(asking) {
  if (asking.asks === 'file') {
    const file = await pickFile(asking.accept)
    if (!file) return
    const c = await asking.build(file)
    if (c) await runCommand(c.command, withSel(c.args))
    return
  }
  if (asking.asks === 'url') {
    const url = await ask({ kind: 'text', title: 'Insert a link', message: 'The address to link to (a web address, an email address or #bookmark).', placeholder: 'https://', ok: 'Link' })
    if (url === null) return
    const c = asking.build(url)
    if (!c) return void (status.value = 'That is not an address to link to.')
    await runCommand(c.command, withSel(c.args))
  }
}

// an editor's page-side work: its answer may carry where the selection landed; the room reads the
// document again afterwards
async function runAction(act, args = {}) {
  const d = doc.value
  if (d === null) return
  const result = await act({ api: props.api, doc: d, page: page.value, count: nav.value.count, sel: sel.value, args: { ...args }, pickFile })
  if (d !== doc.value) return
  const done = parseJson(result)
  if (done && typeof done === 'object' && done.ok === false) status.value = plainError(done.error)
  const landed = readSelection(done?.selection)
  if (landed) sel.value = landed
  await refresh()
  await refreshSelection(true)
}

// --- the editor's views ---

function togglePanel(name) {
  const v = views.value.find((x) => x.name === name)
  if (!v) return
  panelsOn[`${editor.value}:${name}`] = !panelOn(v)
}

function openOverlay(o) {
  if (doc.value === null) return
  overlay.value = { name: o.name, comp: viewComponent(o), props: viewProps(o.name, { api: props.api, doc: doc.value, page: page.value, start: o.start, count: nav.value.count }) }
}

function closeOverlay() {
  if (!overlay.value) return
  overlay.value = null
  enqueue(async () => {
    await refresh()
    await refreshSelection()
  })
  nextTick(() => pageView.value?.focusInput?.())
}

// a panel changed the document itself (the notes pane's own command): read it again
function onViewChanged() {
  enqueue(async () => {
    await refresh()
    await refreshSelection()
  })
}

// a panel asks for a command (a bookmark row turns to its page)
function onViewCommand(c) {
  if (!c || !c.command) return
  enqueue(() => runCommand(c.command, withSel({ ...(c.args ?? {}) })))
}

// --- the room's own dialog ---

function ask(d) {
  return new Promise((resolve) => {
    // a dialog still open gives way (answered no)
    if (dialog.value) dialog.value.resolve(null)
    dialogText.value = ''
    dialog.value = { ...d, resolve }
    nextTick(() => dialogInput.value?.focus?.())
  })
}

function answer(yes) {
  const d = dialog.value
  if (!d) return
  dialog.value = null
  const text = dialogText.value
  dialogText.value = ''
  d.resolve(yes ? (d.kind === 'confirm' ? true : text) : null)
  nextTick(() => pageView.value?.focusInput?.())
}

// --- links on the page ---

function onLink(l) {
  if (!l || doc.value === null) return
  const t = linkTarget(l.target)
  if (!t) return
  if (t.kind === 'web') return void openWeb(t)
  enqueue(async () => {
    const follow = `${editor.value}.follow_link`
    if (commands.value.includes(follow) && Number.isFinite(l.x) && Number.isFinite(l.y)) return void (await runCommand(follow, { page: page.value, x: l.x, y: l.y }))
    await turn(t.page)
  })
}

async function openWeb(t) {
  if (!has('openUrl')) return
  const yes = await ask({ kind: 'confirm', title: 'Open a web link', message: `Open ${t.host} in your browser?`, detail: t.uri, ok: 'Open' })
  if (!yes) return
  try {
    await props.api.openUrl(t.uri)
  } catch (reason) {
    status.value = plainError(reason)
  }
}

// --- fit page, fit width ---

function fit(mode) {
  const z = fitZoomFor(mode, pageSizeOf(editState.value, list.value), pageView.value?.viewSize?.())
  if (z) zoom.value = z
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
  'view.fit_page': () => fit('page'),
  'view.fit_width': () => fit('width'),
}

function onIntent(it) {
  if (!it || !it.command) return
  // an editor's panel shown and hidden, or its full-screen view opened
  const toggle = viewToggle(editor.value, it.command)
  if (toggle) return togglePanel(toggle)
  const shown = overlayFor(editor.value, it.command, it.args, page.value, nav.value.count)
  if (shown) return openOverlay(shown)
  const mine = ROOM_INTENTS[it.command]
  if (mine) {
    if (!busy.value) mine()
    return
  }
  enqueue(() => runIntent(it))
}

function onButton(b) {
  if (!b) return
  const act = !b.intent && actionFor(editor.value, b.command)
  if (b.intent) onIntent({ command: b.intent, args: b.args })
  else if (act) enqueue(() => runAction(act, b.args))
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
  if (dialog.value || overlay.value) return
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
              @link="onLink"
            />
            <FindBar v-model:replace="findReplace" :open="findOpen" :support="finding" :busy="busy" @find="onFind" @replace="onReplace" @close="closeFind" />
          </div>
        </PageNav>
      </template>
      <template v-if="belowView" #below>
        <component :is="belowView.comp" :key="`${editor}:${belowView.name}:${doc}`" v-bind="belowView.props" @changed="onViewChanged" @command="onViewCommand" />
      </template>
      <template v-if="sideView" #side>
        <component :is="sideView.comp" :key="`${editor}:${sideView.name}:${doc}`" v-bind="sideView.props" @changed="onViewChanged" @command="onViewCommand" />
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
    <div v-if="overlay" class="room-overlay">
      <component :is="overlay.comp" v-bind="overlay.props" @end="closeOverlay" @close="closeOverlay" />
    </div>
    <div v-if="dialog" class="room-dialog" role="presentation" @keydown.esc.stop.prevent="answer(false)">
      <form class="dialog-card" role="dialog" aria-modal="true" :aria-label="dialog.title" @submit.prevent="answer(true)">
        <h2>{{ dialog.title }}</h2>
        <p class="dialog-message" :class="{ wrong: dialog.wrong }">{{ dialog.message }}</p>
        <p v-if="dialog.detail" class="dialog-detail" :title="dialog.detail">{{ dialog.detail }}</p>
        <input
          v-if="dialog.kind !== 'confirm'"
          ref="dialogInput"
          v-model="dialogText"
          :type="dialog.kind === 'password' ? 'password' : 'text'"
          :placeholder="dialog.placeholder || undefined"
          :aria-label="dialog.kind === 'password' ? 'Password' : dialog.title"
          autocomplete="off"
          spellcheck="false"
        />
        <div class="dialog-actions">
          <button type="button" @click="answer(false)">Cancel</button>
          <button type="submit" class="primary" :disabled="dialog.kind !== 'confirm' && !dialogText.trim()">{{ dialog.ok || 'OK' }}</button>
        </div>
      </form>
    </div>
  </div>
</template>

<style scoped>
.scriptorium {
  position: relative;
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
.room-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  background: #000;
}
.room-dialog {
  position: absolute;
  inset: 0;
  z-index: 50;
  display: grid;
  place-items: center;
  padding: 16px;
  background: rgba(6, 5, 20, 0.55);
}
.dialog-card {
  display: grid;
  gap: 10px;
  width: min(420px, 100%);
  padding: 18px 20px;
  border: 1px solid var(--scr-line);
  border-radius: 14px;
  background: var(--scr-panel);
  color: var(--scr-text);
  backdrop-filter: blur(12px);
  box-shadow: 0 18px 50px rgba(0, 0, 0, 0.45);
}
.dialog-card h2 {
  margin: 0;
  font-size: 16px;
}
.dialog-message {
  margin: 0;
  font-size: 14px;
}
.dialog-message.wrong {
  color: #ffb4b4;
}
.dialog-detail {
  margin: 0;
  overflow: hidden;
  color: var(--scr-muted);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.dialog-card input {
  padding: 8px 10px;
  border: 1px solid var(--scr-line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--scr-text);
  font: inherit;
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.dialog-actions button {
  padding: 6px 14px;
  border: 1px solid var(--scr-line);
  border-radius: 8px;
  background: transparent;
  color: var(--scr-text);
  font: inherit;
  cursor: pointer;
}
.dialog-actions button.primary {
  border-color: transparent;
  background: var(--scr-chip);
}
.dialog-actions button:disabled {
  opacity: 0.45;
  cursor: default;
}
</style>
