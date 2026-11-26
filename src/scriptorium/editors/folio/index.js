// Folio's ribbon and keys. The Folio lane owns this file; the room reads it through ribbon.js.
//
//   tabs     [{ name, groups: [{ label, buttons: [button] }] }]   the ribbon (Classic) and the toolbar (Studio)
//   panels   [{ id, icon, label, groups: [{ label, buttons }] }]  Studio's inspector cards
//   intents  { 'edit.insert_text': { command: 'folio.insert_text', args: {} }, ... }
//            the neutral key intents (input.js) this editor answers, as its own bus commands; the room
//            adds the selection as args.sel. caret.* and select.all need no entry (the room moves the
//            caret through the editing ground) unless the editor wants its own command for them.
//
//   button   { icon, title, command, args, stateKey }
//            icon: a name in icons.js or inline SVG markup (24x24, currentColor); command: a bus command
//            ('folio.bold'), or `intent` in its place for a neutral one ('find.open'); args: merged with
//            { sel }; stateKey: the key in the editor's state JSON that shows the button pressed.
//
// What the page does in Folio: a click puts the caret in the page's text (drag or shift-click selects,
// Ctrl+C copies in reading order) or, on a form field, in that field. Typing goes into the field the
// caret is in (a space ticks a check box or picks a radio button, letters pick a choice); Tab walks the
// fields. The ways around (pages, links, bookmarks) change nothing in the file: they land the caret at
// the top of the target page and the room turns to it. A web link's address comes back in the editor's
// state (`link.uri`) and in read { link }, for the page to ask the member before the host opens it.
//
// The room does not yet answer `view.fit_page` / `view.fit_width` (the zoom is the room's); `fitZoom`
// below is the arithmetic for it, and the state carries the page's `width` and `height` in points.
// The bookmarks list (BookmarksPanel.vue, fed by read { outline: true }) waits for the room to mount an
// editor's own panel component; until then the Bookmarks card steps through them.

// --- inline icons (24x24, currentColor) ---
const I = {
  first: '<path d="M6 5v14"/><path d="M18 6l-7 6 7 6"/>',
  prev: '<path d="M15 6l-6 6 6 6"/>',
  next: '<path d="M9 6l6 6-6 6"/>',
  last: '<path d="M18 5v14"/><path d="M6 6l7 6-7 6"/>',
  fitPage: '<rect x="6" y="3" width="12" height="18" rx="1.5"/><path d="M9 8h6M9 12h6M9 16h4"/>',
  fitWidth: '<rect x="3" y="5" width="18" height="14" rx="1.5"/><path d="M7 12h10M7 12l2.5-2.5M7 12l2.5 2.5M17 12l-2.5-2.5M17 12l-2.5 2.5"/>',
  follow: '<path d="M10 14a4 4 0 0 0 5.66 0l3-3a4 4 0 0 0-5.66-5.66l-1 1"/><path d="M14 10a4 4 0 0 0-5.66 0l-3 3a4 4 0 0 0 5.66 5.66l1-1"/>',
  bookmark: '<path d="M7 3h10v18l-5-4-5 4z"/>',
  bookmarkPrev: '<path d="M8 3h9v18l-4.5-3.6L8 21z"/><path d="M5 9l-2 3 2 3"/>',
  bookmarkNext: '<path d="M7 3h9v18l-4.5-3.6L7 21z"/><path d="M19 9l2 3-2 3"/>',
  deletePage: '<path d="M7 3h7l4 4v14H7z"/><path d="M10 12l5 5M15 12l-5 5"/>',
  rotateLeft: '<path d="M4 12a8 8 0 1 0 2.34-5.66"/><path d="M4 4v4h4"/>',
  rotateRight: '<path d="M20 12a8 8 0 1 1-2.34-5.66"/><path d="M20 4v4h-4"/>',
  rotateHalf: '<path d="M5 12a7 7 0 0 1 14 0"/><path d="M19 12a7 7 0 0 1-14 0"/><path d="M16 9l3 3 3-3"/>',
  field: '<rect x="3" y="7" width="18" height="10" rx="1.5"/><path d="M7 10v4"/>',
  fieldPrev: '<rect x="7" y="7" width="14" height="10" rx="1.5"/><path d="M5 9l-2 3 2 3"/>',
  fieldNext: '<rect x="3" y="7" width="14" height="10" rx="1.5"/><path d="M19 9l2 3-2 3"/>',
  tick: '<rect x="4" y="4" width="16" height="16" rx="2"/><path d="M8 12l3 3 5-6"/>',
  erase: '<path d="M21 6H9l-6 6 6 6h12z"/><path d="M13 10l4 4M17 10l-4 4"/>',
  note: '<path d="M5 4h14v11l-5 5H5z"/><path d="M14 20v-5h5"/><path d="M8 9h8M8 12h5"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="1.5"/><path d="M16 8V5.5A1.5 1.5 0 0 0 14.5 4h-9A1.5 1.5 0 0 0 4 5.5v9A1.5 1.5 0 0 0 5.5 16H8"/>',
  selectAll: '<path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3"/><path d="M8 9h8M8 12h8M8 15h5"/>',
}

const pages = {
  label: 'Pages',
  buttons: [
    { icon: I.first, title: 'First page', command: 'folio.go_to_page', args: { to: 'first' } },
    { icon: I.prev, title: 'Previous page', command: 'folio.go_to_page', args: { to: 'previous' } },
    { icon: I.next, title: 'Next page', command: 'folio.go_to_page', args: { to: 'next' } },
    { icon: I.last, title: 'Last page', command: 'folio.go_to_page', args: { to: 'last' } },
  ],
}

const zoom = {
  label: 'Zoom',
  buttons: [
    { icon: I.fitPage, title: 'Fit page', intent: 'view.fit_page' },
    { icon: I.fitWidth, title: 'Fit width', intent: 'view.fit_width' },
  ],
}

const links = {
  label: 'Links',
  buttons: [{ icon: I.follow, title: 'Follow the link at the caret', command: 'folio.follow_link', args: {} }],
}

const bookmarks = {
  label: 'Bookmarks',
  buttons: [
    { icon: I.bookmarkPrev, title: 'Previous bookmark', command: 'folio.bookmark', args: { to: 'previous' } },
    { icon: I.bookmarkNext, title: 'Next bookmark', command: 'folio.bookmark', args: { to: 'next' } },
  ],
}

const organise = {
  label: 'Pages',
  buttons: [
    { icon: I.deletePage, title: 'Delete this page (or the pages the selection spans)', command: 'folio.delete_pages', args: {} },
    { icon: I.rotateLeft, title: 'Rotate left', command: 'folio.rotate_pages', args: { angle: -90 } },
    { icon: I.rotateRight, title: 'Rotate right', command: 'folio.rotate_pages', args: { angle: 90 } },
    { icon: I.rotateHalf, title: 'Turn upside down', command: 'folio.rotate_pages', args: { angle: 180 } },
  ],
}

const fields = {
  label: 'Fields',
  buttons: [
    { icon: I.fieldPrev, title: 'Previous field', command: 'folio.next_field', args: { back: true } },
    { icon: I.fieldNext, title: 'Next field', command: 'folio.next_field', args: { back: false } },
    { icon: I.tick, title: 'Tick or clear the box', command: 'folio.insert_text', args: { text: ' ' }, stateKey: 'field.checked' },
    { icon: I.erase, title: 'Erase a character', command: 'folio.backspace', args: {} },
  ],
}

const comment = {
  label: 'Notes',
  buttons: [{ icon: I.note, title: 'Sticky note at the caret', command: 'folio.add_note', args: { text: 'Note' } }],
}

const find = {
  label: 'Find',
  buttons: [
    { icon: 'find', title: 'Find', intent: 'find.open' },
    { icon: I.selectAll, title: 'Select all', intent: 'select.all' },
    { icon: I.copy, title: 'Copy', intent: 'clip.copy' },
  ],
}

// The tab names the room has always shown (Home, Annotate, Pages, Forms, View) carry the tier-1 groups:
// Home finds, selects and copies; Annotate comments; Pages organises; Forms fills; View goes around.
export const tabs = [
  { name: 'Home', groups: [find, pages] },
  { name: 'Annotate', groups: [comment] },
  { name: 'Pages', groups: [organise] },
  { name: 'Forms', groups: [fields] },
  { name: 'View', groups: [pages, zoom, links, bookmarks] },
]

export const panels = [
  { id: 'bookmarks', icon: I.bookmark, label: 'Bookmarks', groups: [bookmarks, pages] },
  { id: 'fields', icon: I.field, label: 'Fields', groups: [fields] },
  { id: 'pages', icon: I.rotateRight, label: 'Pages', groups: [organise, links] },
]

export const intents = {
  'edit.insert_text': { command: 'folio.insert_text', args: {} },
  'clip.paste': { command: 'folio.insert_text', args: {} },
  'edit.backspace': { command: 'folio.backspace', args: {} },
  'edit.delete': { command: 'folio.backspace', args: {} },
  'edit.enter': { command: 'folio.insert_text', args: { text: '\n' } },
  'edit.tab': { command: 'folio.next_field', args: {} },
}

/** The zoom that fits a page (`mode` 'page': all of it; 'width': its width) in a view, both in the
 * same units; `pad` is kept free on each side. 1 when the sizes make no sense. */
export function fitZoom(pageW, pageH, viewW, viewH, mode = 'page', pad = 16) {
  const [pw, ph, vw, vh] = [pageW, pageH, viewW, viewH].map(Number)
  if (![pw, ph, vw, vh].every((v) => Number.isFinite(v) && v > 0)) return 1
  const w = Math.max(1, vw - 2 * pad)
  const h = Math.max(1, vh - 2 * pad)
  const z = mode === 'width' ? w / pw : Math.min(w / pw, h / ph)
  return Math.round(z * 1000) / 1000
}

/** read { outline: true }'s tree as flat rows for a list: [{ index, title, page, depth }] (a row
 * without a page cannot be gone to). */
export function outlineRows(tree, depth = 0, out = []) {
  const list = Array.isArray(tree) ? tree : Array.isArray(tree?.outline) ? tree.outline : []
  for (const n of list) {
    if (!n || typeof n !== 'object') continue
    const page = Number.isInteger(n.page) && n.page >= 0 ? n.page : null
    out.push({ index: Number.isInteger(n.index) ? n.index : out.length, title: String(n.title ?? ''), page, depth })
    if (depth < 32) outlineRows(n.children, depth + 1, out)
  }
  return out
}

/** The bus command a bookmark row runs (null for a row that leads nowhere). */
export function bookmarkCommand(row) {
  if (!row || row.page === null || row.page === undefined) return null
  return { command: 'folio.bookmark', args: { index: row.index } }
}

export default { tabs, panels, intents }
