// Ledger's ribbon and keys. The Ledger lane owns this file; the room reads it through ribbon.js.
//
//   tabs     [{ name, groups: [{ label, buttons: [button] }] }]   the ribbon (Classic) and the toolbar (Studio)
//   panels   [{ id, icon, label, groups: [{ label, buttons }] }]  Studio's inspector cards
//   intents  { 'edit.insert_text': { command: 'ledger.insert_text', args: {} }, ... }
//            the neutral key intents (input.js) this editor answers, as its own bus commands; the room
//            adds the selection as args.sel.
//
//   button   { icon, title, command, args, stateKey }
//            icon: a name in icons.js or inline SVG markup (24x24, currentColor); command: a bus command
//            ('ledger.toggle'), or `intent` in its place for a neutral one ('find.open'); args: merged with
//            { sel }; stateKey: the key in the editor's state JSON that shows the button pressed.
//
// The caret keys go through `ledger.move` (not the room's own caret walk) so the sheet scrolls to keep
// the active cell in view; a move changes nothing in the file and leaves no undo step.

// --- the editor's own icons (24x24, currentColor) ------------------------------------------------
const I = {
  cut: '<circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M8 16 18 4M16 16 6 4"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="1.5"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/>',
  painter: '<rect x="4" y="3" width="14" height="6" rx="1"/><path d="M18 6h2v5h-8v3M11 14h2v7h-2Z"/>',
  strike: '<path d="M5 12h14M16 6.5A4 4 0 0 0 12 5c-2.2 0-4 1.2-4 3 0 1.3.9 2.2 2.4 2.8M8 17.5c.8 1 2.2 1.5 4 1.5 2.2 0 4-1.2 4-3"/>',
  grow: '<path d="M4 19 9 5l5 14M6 14h6M17 9V3M14 6h6"/>',
  shrink: '<path d="M4 19 9 5l5 14M6 14h6M14 6h6"/>',
  fontColor: '<path d="M6 16 11 4l5 12M8 11h6"/><path d="M4 20h16" stroke-width="3"/>',
  fill: '<path d="m5 11 6-6 7 7-6 6Z"/><path d="M19 15s2 2.2 2 3.5a2 2 0 0 1-4 0c0-1.3 2-3.5 2-3.5ZM3 21h12" />',
  noFill: '<path d="m5 11 6-6 7 7-6 6Z"/><path d="M3 3l18 18"/>',
  top: '<path d="M4 4h16M12 8v12M8 12l4-4 4 4"/>',
  middle: '<path d="M4 12h16M12 3v5M12 16v5M9 6l3 2 3-2M9 18l3-2 3 2"/>',
  bottom: '<path d="M4 20h16M12 4v12M8 12l4 4 4-4"/>',
  wrap: '<path d="M4 6h16M4 12h13a3 3 0 0 1 0 6h-4M15 16l-2 2 2 2M4 18h6"/>',
  merge: '<rect x="3" y="6" width="18" height="12" rx="1"/><path d="M7 12h10M9 10l-2 2 2 2M15 10l2 2-2 2"/>',
  unmerge: '<rect x="3" y="6" width="18" height="12" rx="1"/><path d="M12 6v12M8 12H5M16 12h3"/>',
  money: '<path d="M12 3v18M16 7.5C16 6 14.2 5 12 5S8 6 8 8s2 2.6 4 3 4 1 4 3-1.8 3-4 3-4-1-4-2.5"/>',
  percent: '<path d="M19 5 5 19"/><circle cx="7" cy="7" r="2.5"/><circle cx="17" cy="17" r="2.5"/>',
  comma: '<path d="M10 16c0 2-1 3.5-3 4.5M6 9h0M14 9h0" stroke-width="3" stroke-linecap="round"/>',
  date: '<rect x="4" y="5" width="16" height="15" rx="1.5"/><path d="M4 10h16M9 3v4M15 3v4"/>',
  clock: '<circle cx="12" cy="12" r="8"/><path d="M12 8v4l3 2"/>',
  textFmt: '<path d="M5 6V4h14v2M12 4v16M9 20h6"/><path d="M3 20h3" />',
  general: '<path d="M5 18 9 6h2l4 12M7 14h6M17 10h4M17 14h4"/>',
  decMore: '<path d="M3 17h1M7 9v8M7 9a2 2 0 0 1 4 0v6a2 2 0 0 1-4 0M15 9v8M15 9a2 2 0 0 1 4 0v6a2 2 0 0 1-4 0M18 4l3 2-3 2M21 6h-6"/>',
  decLess: '<path d="M3 17h1M7 9v8M7 9a2 2 0 0 1 4 0v6a2 2 0 0 1-4 0M18 4l-3 2 3 2M15 6h6"/>',
  sum: '<path d="M17 5H7l6 7-6 7h10"/>',
  fillDown: '<rect x="5" y="3" width="14" height="6" rx="1"/><path d="M12 11v9M8 16l4 4 4-4"/>',
  fillRight: '<rect x="3" y="5" width="6" height="14" rx="1"/><path d="M11 12h9M16 8l4 4-4 4"/>',
  eraser: '<path d="m4 15 9-9 7 7-6 6H8Z"/><path d="M14 19h7"/>',
  sortUp: '<path d="M7 20V4M3 8l4-4 4 4M14 6h3l-3 5h3M14 14h3v6M14 17h3"/>',
  sortDown: '<path d="M7 4v16M3 16l4 4 4-4M14 6h3v6M14 9h3M14 15h3l-3 5h3"/>',
  filter: '<path d="M4 5h16l-6 8v6l-4-2v-4Z"/>',
  filterClear: '<path d="M4 5h16l-6 8v6l-4-2v-4Z"/><path d="M16 15l5 5M21 15l-5 5"/>',
  rowAdd: '<rect x="3" y="4" width="18" height="6" rx="1"/><path d="M12 13v8M8 17h8"/>',
  colAdd: '<rect x="4" y="3" width="6" height="18" rx="1"/><path d="M13 12h8M17 8v8"/>',
  rowDel: '<rect x="3" y="4" width="18" height="6" rx="1"/><path d="M8 17h8"/>',
  colDel: '<rect x="4" y="3" width="6" height="18" rx="1"/><path d="M13 12h8"/>',
  cellsDown: '<rect x="5" y="3" width="14" height="6" rx="1"/><path d="M12 12v8M9 17l3 3 3-3"/>',
  cellsRight: '<rect x="3" y="5" width="6" height="14" rx="1"/><path d="M12 12h8M17 9l3 3-3 3"/>',
  fitCol: '<path d="M4 4v16M20 4v16M7 12h10M10 9l-3 3 3 3M14 9l3 3-3 3"/>',
  fitRow: '<path d="M4 4h16M4 20h16M12 7v10M9 10l3-3 3 3M9 14l3 3 3-3"/>',
  hide: '<path d="M3 12s3.5-6 9-6 9 6 9 6-3.5 6-9 6-9-6-9-6Z"/><path d="M4 4l16 16"/>',
  show: '<path d="M3 12s3.5-6 9-6 9 6 9 6-3.5 6-9 6-9-6-9-6Z"/><circle cx="12" cy="12" r="2.5"/>',
  sheetAdd: '<path d="M6 3h8l4 4v14H6Z"/><path d="M14 3v4h4M12 11v6M9 14h6"/>',
  freeze: '<rect x="3" y="3" width="18" height="18" rx="1.5"/><path d="M3 9h18M9 3v18"/>',
  freezeRow: '<rect x="3" y="3" width="18" height="18" rx="1.5"/><path d="M3 8h18" stroke-width="2.6"/>',
  freezeCol: '<rect x="3" y="3" width="18" height="18" rx="1.5"/><path d="M8 3v18" stroke-width="2.6"/>',
  unfreeze: '<rect x="3" y="3" width="18" height="18" rx="1.5"/><path d="M3 9h18M9 3v18" stroke-dasharray="2 2"/>',
  bAll: '<rect x="4" y="4" width="16" height="16"/><path d="M4 12h16M12 4v16"/>',
  bOut: '<rect x="4" y="4" width="16" height="16"/><path d="M4 12h16M12 4v16" stroke-dasharray="1 2.5"/>',
  bThick: '<rect x="4" y="4" width="16" height="16" stroke-width="3"/>',
  bBottom: '<path d="M4 4v16M20 4v16M4 4h16" stroke-dasharray="1 2.5"/><path d="M3 20h18" stroke-width="2.6"/>',
  bNone: '<rect x="4" y="4" width="16" height="16" stroke-dasharray="1 2.5"/>',
}

const b = (icon, title, command, args = {}, stateKey = undefined) => ({ icon, title, command, args, ...(stateKey ? { stateKey } : {}) })
const it = (icon, title, intent, args = {}) => ({ icon, title, intent, args })

// --- groups, shared by the ribbon and the inspector -----------------------------------------------
const clipboard = {
  label: 'Clipboard',
  buttons: [
    it(I.cut, 'Cut (Ctrl+X)', 'clip.cut'),
    it(I.copy, 'Copy (Ctrl+C)', 'clip.copy'),
    b(I.painter, 'Format Painter: press on the cells to copy from, then again on the cells to paint', 'ledger.format_painter', {}, 'painter'),
  ],
}

const font = {
  label: 'Font',
  buttons: [
    b('bold', 'Bold (Ctrl+B)', 'ledger.toggle', { what: 'bold' }, 'bold'),
    b('italic', 'Italic (Ctrl+I)', 'ledger.toggle', { what: 'italic' }, 'italic'),
    b('underline', 'Underline (Ctrl+U)', 'ledger.toggle', { what: 'underline' }, 'underline'),
    b(I.strike, 'Strikethrough', 'ledger.toggle', { what: 'strike' }, 'strike'),
    b(I.grow, 'Bigger font', 'ledger.format', { size_delta: 1 }),
    b(I.shrink, 'Smaller font', 'ledger.format', { size_delta: -1 }),
    b(I.fontColor, 'Red text', 'ledger.format', { color: '#C00000' }),
    b('text', 'Automatic text colour', 'ledger.format', { color: null }),
    b(I.fill, 'Yellow fill', 'ledger.format', { fill: '#FFFF00' }),
    b(I.noFill, 'No fill', 'ledger.format', { fill: null }),
  ],
}

const borders = {
  label: 'Borders',
  buttons: [
    b(I.bAll, 'All borders', 'ledger.borders', { preset: 'all' }),
    b(I.bOut, 'Outside borders', 'ledger.borders', { preset: 'outside' }),
    b(I.bThick, 'Thick box border', 'ledger.borders', { preset: 'thick_box' }),
    b(I.bBottom, 'Bottom border', 'ledger.borders', { preset: 'bottom' }),
    b(I.bNone, 'No border', 'ledger.borders', { preset: 'none' }),
  ],
}

const alignment = {
  label: 'Alignment',
  buttons: [
    b(I.top, 'Top', 'ledger.format', { v_align: 'top' }, 'align.top'),
    b(I.middle, 'Middle', 'ledger.format', { v_align: 'middle' }, 'align.middle'),
    b(I.bottom, 'Bottom', 'ledger.format', { v_align: 'bottom' }, 'align.bottom'),
    b('alignLeft', 'Align left', 'ledger.format', { h_align: 'left' }, 'align.left'),
    b('alignCenter', 'Centre', 'ledger.format', { h_align: 'center' }, 'align.center'),
    b('alignRight', 'Align right', 'ledger.format', { h_align: 'right' }, 'align.right'),
    b(I.wrap, 'Wrap text', 'ledger.toggle', { what: 'wrap' }, 'wrap'),
    b(I.merge, 'Merge & Center', 'ledger.merge', { mode: 'center' }, 'merged'),
    b(I.unmerge, 'Unmerge', 'ledger.unmerge'),
  ],
}

const number = {
  label: 'Number',
  buttons: [
    b(I.general, 'General', 'ledger.format', { number_format: 'general' }, 'format.general'),
    b('numbers', 'Number', 'ledger.format', { number_format: 'number' }, 'format.number'),
    b(I.money, 'Currency', 'ledger.format', { number_format: 'currency' }, 'format.currency'),
    b(I.money, 'Accounting', 'ledger.format', { number_format: 'accounting' }, 'format.accounting'),
    b(I.percent, 'Percentage', 'ledger.format', { number_format: 'percent' }, 'format.percent'),
    b(I.comma, 'Comma style', 'ledger.format', { number_format: 'comma' }),
    b(I.date, 'Short date', 'ledger.format', { number_format: 'date' }, 'format.date'),
    b(I.date, 'Long date', 'ledger.format', { number_format: 'long_date' }),
    b(I.clock, 'Time', 'ledger.format', { number_format: 'time' }, 'format.time'),
    b(I.textFmt, 'Text', 'ledger.format', { number_format: 'text' }, 'format.text'),
    b(I.decMore, 'Increase decimal', 'ledger.format', { decimals: 1 }),
    b(I.decLess, 'Decrease decimal', 'ledger.format', { decimals: -1 }),
  ],
}

const cells = {
  label: 'Cells',
  buttons: [
    b(I.rowAdd, 'Insert rows', 'ledger.insert', { what: 'rows' }),
    b(I.colAdd, 'Insert columns', 'ledger.insert', { what: 'cols' }),
    b(I.rowDel, 'Delete rows', 'ledger.delete', { what: 'rows' }),
    b(I.colDel, 'Delete columns', 'ledger.delete', { what: 'cols' }),
    b(I.fitCol, 'AutoFit column width', 'ledger.col_width', { auto: true }),
    b(I.fitRow, 'AutoFit row height', 'ledger.row_height', { auto: true }),
  ],
}

const editing = {
  label: 'Editing',
  buttons: [
    b(I.sum, 'AutoSum', 'ledger.autosum', { fn: 'SUM' }),
    b(I.fillDown, 'Fill down (the top row across the selection)', 'ledger.fill', { direction: 'down' }),
    b(I.fillRight, 'Fill right', 'ledger.fill', { direction: 'right' }),
    b(I.eraser, 'Clear contents', 'ledger.clear', { what: 'contents' }),
    b(I.noFill, 'Clear formats', 'ledger.clear', { what: 'formats' }),
    b(I.sortUp, 'Sort A to Z', 'ledger.sort', { descending: false }),
    b(I.sortDown, 'Sort Z to A', 'ledger.sort', { descending: true }),
    b(I.filter, 'Filter', 'ledger.filter', {}, 'filter'),
    it('find', 'Find (Ctrl+F)', 'find.open'),
    it('replace', 'Replace (Ctrl+H)', 'find.replace'),
  ],
}

const insert = {
  label: 'Insert',
  buttons: [
    b(I.rowAdd, 'Insert rows', 'ledger.insert', { what: 'rows' }),
    b(I.colAdd, 'Insert columns', 'ledger.insert', { what: 'cols' }),
    b(I.cellsDown, 'Insert cells, shift down', 'ledger.insert', { what: 'cells_down' }),
    b(I.cellsRight, 'Insert cells, shift right', 'ledger.insert', { what: 'cells_right' }),
  ],
}

const remove = {
  label: 'Delete',
  buttons: [
    b(I.rowDel, 'Delete rows', 'ledger.delete', { what: 'rows' }),
    b(I.colDel, 'Delete columns', 'ledger.delete', { what: 'cols' }),
    b(I.cellsDown, 'Delete cells, shift up', 'ledger.delete', { what: 'cells_up' }),
    b(I.cellsRight, 'Delete cells, shift left', 'ledger.delete', { what: 'cells_left' }),
  ],
}

const sheets = {
  label: 'Sheets',
  buttons: [b(I.sheetAdd, 'New sheet', 'ledger.add_sheet')],
}

const rowsCols = {
  label: 'Rows and columns',
  buttons: [
    b(I.hide, 'Hide rows', 'ledger.hide', { what: 'rows', hidden: true }),
    b(I.show, 'Unhide rows', 'ledger.hide', { what: 'rows', hidden: false }),
    b(I.hide, 'Hide columns', 'ledger.hide', { what: 'cols', hidden: true }),
    b(I.show, 'Unhide columns', 'ledger.hide', { what: 'cols', hidden: false }),
    b(I.fitCol, 'AutoFit column width', 'ledger.col_width', { auto: true }),
    b(I.fitRow, 'AutoFit row height', 'ledger.row_height', { auto: true }),
    b(I.fitCol, 'Default column width', 'ledger.col_width', { width: null }),
  ],
}

const functions = {
  label: 'AutoSum',
  buttons: [
    b(I.sum, 'Sum', 'ledger.autosum', { fn: 'SUM' }),
    b(I.sum, 'Average', 'ledger.autosum', { fn: 'AVERAGE' }),
    b(I.sum, 'Count numbers', 'ledger.autosum', { fn: 'COUNT' }),
    b(I.sum, 'Max', 'ledger.autosum', { fn: 'MAX' }),
    b(I.sum, 'Min', 'ledger.autosum', { fn: 'MIN' }),
  ],
}

const sortFilter = {
  label: 'Sort and filter',
  buttons: [
    b(I.sortUp, 'Sort A to Z', 'ledger.sort', { descending: false }),
    b(I.sortDown, 'Sort Z to A', 'ledger.sort', { descending: true }),
    b(I.filter, 'Filter', 'ledger.filter', {}, 'filter'),
    b(I.filterClear, 'Clear the filter', 'ledger.clear_filter'),
    b('close', 'Remove the filter', 'ledger.remove_filter'),
  ],
}

const freeze = {
  label: 'Window',
  buttons: [
    b(I.freeze, 'Freeze panes at the active cell', 'ledger.freeze', {}, 'frozen'),
    b(I.freezeRow, 'Freeze top row', 'ledger.freeze', { rows: 1, cols: 0 }),
    b(I.freezeCol, 'Freeze first column', 'ledger.freeze', { rows: 0, cols: 1 }),
    b(I.unfreeze, 'Unfreeze panes', 'ledger.unfreeze'),
  ],
}

export const tabs = [
  { name: 'Home', groups: [clipboard, font, alignment, number, cells, editing] },
  { name: 'Insert', groups: [insert, remove, sheets] },
  { name: 'Formulas', groups: [functions] },
  { name: 'Data', groups: [sortFilter, { label: 'Find', buttons: editing.buttons.filter((x) => x.intent) }] },
  { name: 'View', groups: [freeze, rowsCols] },
]

// Studio's inspector: the selected cells' formats and the sheet's structure, one card each.
export const panels = [
  { id: 'cell', icon: 'text', label: 'Cell', groups: [clipboard, font] },
  { id: 'alignment', icon: 'paragraph', label: 'Alignment', groups: [alignment] },
  { id: 'number', icon: 'numbers', label: 'Number', groups: [number] },
  { id: 'borders', icon: 'frame', label: 'Borders', groups: [borders] },
  { id: 'structure', icon: 'table', label: 'Rows and columns', groups: [insert, remove, rowsCols] },
  { id: 'data', icon: 'sliders', label: 'Data', groups: [functions, sortFilter, freeze, sheets] },
]

const move = (motion) => ({ command: 'ledger.move', args: { motion } })

export const intents = {
  'caret.left': move('left'),
  'caret.right': move('right'),
  'caret.up': move('up'),
  'caret.down': move('down'),
  'caret.word_left': move('word_left'),
  'caret.word_right': move('word_right'),
  'caret.line_start': move('line_start'),
  'caret.line_end': move('line_end'),
  'caret.doc_start': move('doc_start'),
  'caret.doc_end': move('doc_end'),
  'select.all': { command: 'ledger.select_all', args: {} },
  'edit.insert_text': { command: 'ledger.insert_text', args: {} },
  'edit.backspace': { command: 'ledger.backspace', args: {} },
  'edit.delete': { command: 'ledger.clear', args: { what: 'contents' } },
  'edit.enter': { command: 'ledger.enter', args: {} },
  'edit.tab': { command: 'ledger.tab', args: {} },
  'format.bold': { command: 'ledger.toggle', args: { what: 'bold' } },
  'format.italic': { command: 'ledger.toggle', args: { what: 'italic' } },
  'format.underline': { command: 'ledger.toggle', args: { what: 'underline' } },
  'clip.cut': { command: 'ledger.cut', args: {} },
  'clip.paste': { command: 'ledger.paste', args: {} },
}

export default { tabs, panels, intents }
