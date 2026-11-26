// Scroll's ribbon and keys. The Scroll lane owns this file; the room reads it through ribbon.js.
//
//   tabs     [{ name, groups: [{ label, buttons: [button] }] }]   the ribbon (Classic) and the toolbar (Studio)
//   panels   [{ id, icon, label, groups: [{ label, buttons }] }]  Studio's inspector cards
//   intents  { 'edit.insert_text': { command: 'scroll.insert_text', args: {} }, ... }
//            the neutral key intents (input.js) this editor answers, as its own bus commands; the room
//            adds the selection as args.sel. caret.* and select.all need no entry (the room moves the
//            caret through the editing ground) unless the editor wants its own command for them.
//
//   button   { icon, title, command, args, stateKey }
//            icon: a name in icons.js or inline SVG markup (24x24, currentColor); command: a bus command
//            ('scroll.toggle'), or `intent` in its place for a neutral one ('find.open'); args: merged with
//            { sel }; stateKey: the key in the editor's state JSON that shows the button pressed.
//
// Every command here is one the Rust side lists (scriptorium/src/scroll/mod.rs: write_specs), so the
// companion can press the same buttons through the AI's door.

// --- the editor's own icons (24x24, currentColor) ------------------------------------------------
const text = (t, size = 11, weight = 700) =>
  `<text x="12" y="16" text-anchor="middle" font-size="${size}" font-weight="${weight}" font-family="Georgia, serif" fill="currentColor" stroke="none">${t}</text>`

const I = {
  cut: '<circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M8 16 18 4M16 16 6 4"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="1.5"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/>',
  strike: '<path d="M5 12h14M16 6.5A4 4 0 0 0 12 5c-2.2 0-4 1.2-4 3 0 1.3.9 2.2 2.4 2.8M8 17.5c.8 1 2.2 1.5 4 1.5 2.2 0 4-1.2 4-3"/>',
  sup: '<path d="m4 8 8 10M12 8l-8 10"/><path d="M15 9.5a2 2 0 1 1 3.4 1.4L15 14h4.5" stroke-width="1.5"/>',
  sub: '<path d="m4 6 8 10M12 6l-8 10"/><path d="M15 15.5a2 2 0 1 1 3.4 1.4L15 20h4.5" stroke-width="1.5"/>',
  grow: '<path d="M4 19 9 5l5 14M6 14h6M17 9V3M14 6h6"/>',
  shrink: '<path d="M4 19 9 5l5 14M6 14h6M14 6h6"/>',
  fontColor: '<path d="M6 16 11 4l5 12M8 11h6"/><path d="M4 20h16" stroke-width="3" stroke="#C00000"/>',
  highlight: '<path d="m9 15 7-11 3 2-7 11Z"/><path d="M9 15l-2 4h4"/><path d="M4 21h16" stroke-width="3" stroke="#E6C200"/>',
  clearFmt: '<path d="M5 5h11M10 5 7 19M15 13l6 6M21 13l-6 6"/>',
  underlineDouble: '<path d="M7 4v6a5 5 0 0 0 10 0V4M5 18h14M5 21h14"/>',
  justify: '<path d="M4 6h16M4 10h16M4 14h16M4 18h16"/>',
  indentMore: '<path d="M10 6h10M10 10h10M10 14h10M4 18h16M4 9l3 2.5L4 14"/>',
  indentLess: '<path d="M10 6h10M10 10h10M10 14h10M4 18h16M7 9l-3 2.5L7 14"/>',
  lineSpacing: '<path d="M11 6h9M11 12h9M11 18h9M6 4v16M3.5 6.5 6 4l2.5 2.5M3.5 17.5 6 20l2.5-2.5"/>',
  spaceAfter: '<path d="M4 5h16M4 9h16M4 13h10M12 16v5M9.5 18.5 12 21l2.5-2.5"/>',
  keepNext: '<path d="M4 5h16M4 9h12M4 15h16M4 19h12"/><path d="M20 9v6" stroke-dasharray="1.5 2"/>',
  multilevel: '<path d="M8 6h12M11 12h9M14 18h6"/><circle cx="4.5" cy="6" r="1"/><circle cx="7.5" cy="12" r="1"/><circle cx="10.5" cy="18" r="1"/>',
  restart: '<path d="M4 12a8 8 0 1 0 2.5-5.8M4 4v4h4"/>' + text('1', 9),
  paste: '<rect x="6" y="5" width="12" height="16" rx="1.5"/><path d="M9 5V3h6v2M9 11h6M9 15h4"/>',
  pageBreak: '<path d="M6 3v6h12V3M6 21v-6h12v6"/><path d="M3 12h3M9 12h2M13 12h2M18 12h3"/>',
  sectionBreak: '<path d="M6 3v6h12V3M6 21v-6h12v6"/><path d="M3 12h18" stroke-dasharray="2 2"/>',
  continuous: '<path d="M5 4h14M5 8h14M5 16h14M5 20h14"/><path d="M3 12h18" stroke-dasharray="2 2"/>',
  header: '<rect x="4" y="3" width="16" height="18" rx="1.5"/><path d="M4 8h16" /><path d="M7 5.5h6" stroke-width="2"/>',
  footer: '<rect x="4" y="3" width="16" height="18" rx="1.5"/><path d="M4 16h16"/><path d="M7 18.5h6" stroke-width="2"/>',
  pageNumber: '<rect x="4" y="3" width="16" height="18" rx="1.5"/>' + text('#', 10),
  firstPage: '<rect x="3" y="5" width="11" height="15" rx="1"/><path d="M8 3h11a1 1 0 0 1 1 1v13"/><path d="M5 9h7"/>',
  margins: '<rect x="4" y="3" width="16" height="18" rx="1"/><rect x="7.5" y="6.5" width="9" height="11" stroke-dasharray="1.5 1.5"/>',
  portrait: '<rect x="6" y="3" width="12" height="18" rx="1"/>',
  landscape: '<rect x="3" y="6" width="18" height="12" rx="1"/>',
  size: '<rect x="5" y="3" width="14" height="18" rx="1"/><path d="M9 8h6M9 12h6"/>' + text('A4', 6, 600).replace('y="16"', 'y="19"'),
  rowAbove: '<rect x="3" y="12" width="18" height="8" rx="1"/><path d="M12 3v6M9 6h6"/>',
  rowBelow: '<rect x="3" y="4" width="18" height="8" rx="1"/><path d="M12 15v6M9 18h6"/>',
  colLeft: '<rect x="12" y="3" width="8" height="18" rx="1"/><path d="M3 12h6M6 9v6"/>',
  colRight: '<rect x="4" y="3" width="8" height="18" rx="1"/><path d="M15 12h6M18 9v6"/>',
  rowDel: '<rect x="3" y="8" width="18" height="8" rx="1"/><path d="M9 12h6"/>',
  colDel: '<rect x="8" y="3" width="8" height="18" rx="1"/><path d="M12 9v6"/>',
  tableDel: '<rect x="3" y="4" width="18" height="16" rx="1.5"/><path d="M3 9.5h18M9 4v16M14 13l5 5M19 13l-5 5"/>',
  merge: '<rect x="3" y="6" width="18" height="12" rx="1"/><path d="M7 12h10M9 10l-2 2 2 2M15 10l2 2-2 2"/>',
  fitWindow: '<path d="M4 4v16M20 4v16M7 12h10M10 9l-3 3 3 3M14 9l3 3-3 3"/>',
  fitContents: '<path d="M9 4v16M15 4v16M4 12h3M17 12h3M6 10l1.5 2L6 14M18 10l-1.5 2 1.5 2"/>',
  headerRow: '<rect x="3" y="4" width="18" height="16" rx="1.5"/><path d="M3 9h18M9 4v16M15 4v16"/><path d="M4 6.5h16" stroke-width="3"/>',
  vTop: '<path d="M4 4h16M12 8v12M8 12l4-4 4 4"/>',
  vCenter: '<path d="M4 12h16M12 3v5M12 16v5M9 6l3 2 3-2M9 18l3-2 3 2"/>',
  vBottom: '<path d="M4 20h16M12 4v12M8 12l4 4 4-4"/>',
  shade: '<path d="m5 11 6-6 7 7-6 6Z"/><path d="M19 15s2 2.2 2 3.5a2 2 0 0 1-4 0c0-1.3 2-3.5 2-3.5Z"/>',
  noShade: '<path d="m5 11 6-6 7 7-6 6Z"/><path d="M3 3l18 18"/>',
  bordersAll: '<rect x="3" y="3" width="18" height="18"/><path d="M3 12h18M12 3v18"/>',
  bordersOut: '<rect x="3" y="3" width="18" height="18"/><path d="M3 12h18M12 3v18" stroke-dasharray="1.5 2" stroke-width="1"/>',
  bordersNone: '<rect x="3" y="3" width="18" height="18" stroke-dasharray="1.5 2" stroke-width="1"/><path d="M3 12h18M12 3v18" stroke-dasharray="1.5 2" stroke-width="1"/>',
  rowHeight: '<path d="M4 4h16M4 20h16M12 7v10M9 10l3-3 3 3M9 14l3 3 3-3"/>',
  unlink: '<path d="M9 15 7 17a3.5 3.5 0 0 1-5-5l3-3M15 9l2-2a3.5 3.5 0 0 1 5 5l-3 3M4 4l16 16"/>',
  pictureSmaller: '<rect x="3" y="5" width="18" height="14" rx="1.5"/><path d="M9 12h6"/>',
  pictureLarger: '<rect x="3" y="5" width="18" height="14" rx="1.5"/><path d="M9 12h6M12 9v6"/>',
  selectAll: '<rect x="4" y="4" width="16" height="16" rx="1" stroke-dasharray="2 2"/>',
  h1: text('H1'),
  h2: text('H2'),
  h3: text('H3'),
  title: text('T', 15),
  normal: text('¶', 14, 400),
  quote: text('“”', 14, 400),
  strong: text('S', 13, 800),
  emphasis: text('E', 13, 400).replace('font-weight', 'font-style="italic" font-weight'),
}

// --- buttons -------------------------------------------------------------------------------------
const toggle = (icon, title, prop) => ({ icon, title, command: 'scroll.toggle', args: { prop }, stateKey: prop })
const fmt = (icon, title, args) => ({ icon, title, command: 'scroll.format', args })
const para = (icon, title, args, stateKey) => ({ icon, title, command: 'scroll.paragraph', args, ...(stateKey ? { stateKey } : {}) })
const align = (icon, title, value) => ({ icon, title, command: 'scroll.align', args: { value }, stateKey: `align_${value}` })
const style = (icon, title, name) => ({ icon, title, command: 'scroll.style', args: { style: name } })
const font = (name) => ({ icon: '<text x="12" y="16" text-anchor="middle" font-size="12" fill="currentColor" stroke="none" font-family="' + name + '">Aa</text>', title: name, command: 'scroll.format', args: { font: name } })
const size = (pt) => ({ icon: text(String(pt), 9, 600), title: `${pt} pt`, command: 'scroll.format', args: { size: pt } })

const CLIPBOARD = {
  label: 'Clipboard',
  buttons: [
    { icon: I.cut, title: 'Cut', intent: 'clip.cut' },
    { icon: I.copy, title: 'Copy', intent: 'clip.copy' },
    { icon: 'undo', title: 'Undo', intent: 'history.undo' },
    { icon: 'redo', title: 'Redo', intent: 'history.redo' },
  ],
}

const FONT = {
  label: 'Font',
  buttons: [
    toggle('bold', 'Bold (Ctrl+B)', 'bold'),
    toggle('italic', 'Italic (Ctrl+I)', 'italic'),
    toggle('underline', 'Underline (Ctrl+U)', 'underline'),
    fmt(I.underlineDouble, 'Double underline', { underline: 'double' }),
    toggle(I.strike, 'Strikethrough', 'strike'),
    toggle(I.sub, 'Subscript', 'subscript'),
    toggle(I.sup, 'Superscript', 'superscript'),
    { icon: I.grow, title: 'Grow font', command: 'scroll.font_size_step', args: { delta: 1 } },
    { icon: I.shrink, title: 'Shrink font', command: 'scroll.font_size_step', args: { delta: -1 } },
    fmt(I.fontColor, 'Font colour: dark red', { color: 'C00000' }),
    fmt(I.highlight, 'Highlight: yellow', { highlight: 'yellow' }),
    { icon: I.clearFmt, title: 'Clear formatting', command: 'scroll.clear_format', args: {} },
  ],
}

const FONT_FACES = {
  label: 'Typeface',
  buttons: [font('Calibri'), font('Georgia'), font('Arial'), font('Times New Roman'), size(10), size(11), size(12), size(14), size(18), size(24)],
}

const PARAGRAPH = {
  label: 'Paragraph',
  buttons: [
    { icon: 'bullets', title: 'Bullets', command: 'scroll.list', args: { kind: 'bullet' }, stateKey: 'list_bullet' },
    { icon: 'numbers', title: 'Numbering', command: 'scroll.list', args: { kind: 'number' }, stateKey: 'list_number' },
    { icon: I.multilevel, title: 'Multilevel list', command: 'scroll.list', args: { kind: 'multilevel' } },
    { icon: I.restart, title: 'Restart numbering at 1', command: 'scroll.list_restart', args: {} },
    { icon: I.indentLess, title: 'Decrease indent', command: 'scroll.indent', args: { delta: -1 } },
    { icon: I.indentMore, title: 'Increase indent', command: 'scroll.indent', args: { delta: 1 } },
    align('alignLeft', 'Align left', 'left'),
    align('alignCenter', 'Centre', 'center'),
    align('alignRight', 'Align right', 'right'),
    align(I.justify, 'Justify', 'justify'),
    para(I.lineSpacing, 'Line spacing 1.5', { line: 1.5 }),
    para(I.spaceAfter, 'Space after: 12 pt', { after: 12 }),
    para(I.keepNext, 'Keep with next', { keep_next: true }, 'keep_next'),
  ],
}

const STYLES = {
  label: 'Styles',
  buttons: [
    style(I.normal, 'Normal', 'Normal'),
    style(I.title, 'Title', 'Title'),
    style(I.h1, 'Heading 1', 'Heading 1'),
    style(I.h2, 'Heading 2', 'Heading 2'),
    style(I.h3, 'Heading 3', 'Heading 3'),
    style(I.quote, 'Quote', 'Quote'),
    style(I.strong, 'Strong', 'Strong'),
    style(I.emphasis, 'Emphasis', 'Emphasis'),
  ],
}

const EDITING = {
  label: 'Editing',
  buttons: [
    { icon: 'find', title: 'Find (Ctrl+F)', intent: 'find.open' },
    { icon: 'replace', title: 'Replace (Ctrl+H)', intent: 'find.replace' },
    { icon: I.selectAll, title: 'Select all (Ctrl+A)', intent: 'select.all' },
  ],
}

const TABLES = {
  label: 'Tables',
  buttons: [
    { icon: 'table', title: 'Insert a 3 x 3 table', command: 'scroll.insert_table', args: { rows: 3, cols: 3 } },
    { icon: I.rowAbove, title: 'Insert row above', command: 'scroll.table_insert', args: { what: 'row_above' } },
    { icon: I.rowBelow, title: 'Insert row below', command: 'scroll.table_insert', args: { what: 'row_below' } },
    { icon: I.colLeft, title: 'Insert column left', command: 'scroll.table_insert', args: { what: 'col_left' } },
    { icon: I.colRight, title: 'Insert column right', command: 'scroll.table_insert', args: { what: 'col_right' } },
  ],
}

const ILLUSTRATIONS = {
  label: 'Pictures and links',
  buttons: [
    // These two need a file and an address from the member: the room answers the intents with a picker
    // and a prompt, and ./insert.js turns what it gets into the commands' args.
    { icon: 'image', title: 'Picture from a file', intent: 'insert.picture' },
    { icon: 'link', title: 'Link', intent: 'insert.link' },
    { icon: I.unlink, title: 'Remove link', command: 'scroll.unlink', args: {}, stateKey: 'is_link' },
  ],
}

const PAGES = {
  label: 'Pages',
  buttons: [
    { icon: I.pageBreak, title: 'Page break (Ctrl+Enter)', command: 'scroll.page_break', args: {} },
    { icon: I.header, title: 'Header', command: 'scroll.header_footer', args: { which: 'header' }, stateKey: 'in_header' },
    { icon: I.footer, title: 'Footer', command: 'scroll.header_footer', args: { which: 'footer' } },
    { icon: I.pageNumber, title: 'Page number: Page X of Y, bottom centre', command: 'scroll.page_number', args: { where: 'footer', format: 'page_x_of_y', align: 'center' } },
    { icon: I.pageNumber, title: 'Page number here', command: 'scroll.page_number', args: { where: 'here' } },
    { icon: I.firstPage, title: 'Different first page', command: 'scroll.first_page', args: { on: true } },
  ],
}

const PAGE_SETUP = {
  label: 'Page setup',
  buttons: [
    { icon: I.margins, title: 'Normal margins (2.54 cm)', command: 'scroll.page_setup', args: { top: 72, bottom: 72, left: 72, right: 72 } },
    { icon: I.margins, title: 'Narrow margins (1.27 cm)', command: 'scroll.page_setup', args: { top: 36, bottom: 36, left: 36, right: 36 } },
    { icon: I.margins, title: 'Wide margins (5.08 cm sides)', command: 'scroll.page_setup', args: { top: 72, bottom: 72, left: 144, right: 144 } },
    { icon: I.portrait, title: 'Portrait', command: 'scroll.page_setup', args: { orientation: 'portrait' }, stateKey: 'portrait' },
    { icon: I.landscape, title: 'Landscape', command: 'scroll.page_setup', args: { orientation: 'landscape' }, stateKey: 'landscape' },
    { icon: I.size, title: 'A4', command: 'scroll.page_setup', args: { size: 'A4' } },
    { icon: I.size, title: 'Letter', command: 'scroll.page_setup', args: { size: 'Letter' } },
    { icon: I.size, title: 'Legal', command: 'scroll.page_setup', args: { size: 'Legal' } },
  ],
}

const BREAKS = {
  label: 'Breaks',
  buttons: [
    { icon: I.pageBreak, title: 'Page break', command: 'scroll.page_break', args: {} },
    { icon: I.sectionBreak, title: 'Section break: next page', command: 'scroll.section_break', args: { kind: 'nextPage' } },
    { icon: I.continuous, title: 'Section break: continuous', command: 'scroll.section_break', args: { kind: 'continuous' } },
    { icon: I.sectionBreak, title: 'Section break: even page', command: 'scroll.section_break', args: { kind: 'evenPage' } },
    { icon: I.sectionBreak, title: 'Section break: odd page', command: 'scroll.section_break', args: { kind: 'oddPage' } },
  ],
}

const PARA_LAYOUT = {
  label: 'Paragraph spacing',
  buttons: [
    para(I.lineSpacing, 'Single line spacing', { line: 1 }),
    para(I.lineSpacing, 'Double line spacing', { line: 2 }),
    para(I.spaceAfter, 'No space after', { after: 0 }),
    para(I.spaceAfter, 'Space before: 12 pt', { before: 12 }),
    para(I.keepNext, 'Keep lines together', { keep_lines: true }, 'keep_lines'),
    para(I.keepNext, 'Widow and orphan control', { widow: true }, 'widow'),
  ],
}

const TABLE_LAYOUT = {
  label: 'Table',
  buttons: [
    { icon: I.rowDel, title: 'Delete rows', command: 'scroll.table_delete', args: { what: 'row' } },
    { icon: I.colDel, title: 'Delete columns', command: 'scroll.table_delete', args: { what: 'col' } },
    { icon: I.tableDel, title: 'Delete table', command: 'scroll.table_delete', args: { what: 'table' } },
    { icon: I.merge, title: 'Merge cells', command: 'scroll.merge_cells', args: {} },
    { icon: I.fitWindow, title: 'AutoFit to window', command: 'scroll.table_widths', args: { autofit: 'window' } },
    { icon: I.fitContents, title: 'AutoFit to contents', command: 'scroll.table_widths', args: { autofit: 'contents' } },
    { icon: I.headerRow, title: 'Repeat header rows', command: 'scroll.header_rows', args: { on: true } },
    { icon: I.rowHeight, title: 'Row height at least 24 pt', command: 'scroll.row_height', args: { height: 24, exact: false } },
  ],
}

const CELLS = {
  label: 'Cells',
  buttons: [
    { icon: I.vTop, title: 'Align top', command: 'scroll.cell_align', args: { v: 'top' } },
    { icon: I.vCenter, title: 'Align middle', command: 'scroll.cell_align', args: { v: 'center' } },
    { icon: I.vBottom, title: 'Align bottom', command: 'scroll.cell_align', args: { v: 'bottom' } },
    { icon: I.shade, title: 'Shading: light grey', command: 'scroll.shading', args: { color: 'D9D9D9' } },
    { icon: I.noShade, title: 'No shading', command: 'scroll.shading', args: { color: null } },
    { icon: I.bordersAll, title: 'All borders', command: 'scroll.borders', args: { which: 'all' } },
    { icon: I.bordersOut, title: 'Outside borders', command: 'scroll.borders', args: { which: 'outside' } },
    { icon: I.bordersNone, title: 'No borders', command: 'scroll.borders', args: { which: 'none' } },
  ],
}

const PICTURE = {
  label: 'Picture',
  buttons: [
    { icon: I.pictureSmaller, title: 'Picture 2 inches wide', command: 'scroll.picture_size', args: { width: 144 } },
    { icon: I.pictureLarger, title: 'Picture 4 inches wide', command: 'scroll.picture_size', args: { width: 288 } },
  ],
}

export const tabs = [
  { name: 'Home', groups: [CLIPBOARD, FONT, FONT_FACES, PARAGRAPH, STYLES, EDITING] },
  { name: 'Insert', groups: [TABLES, ILLUSTRATIONS, PAGES] },
  { name: 'Layout', groups: [PAGE_SETUP, BREAKS, PARA_LAYOUT, TABLE_LAYOUT, CELLS, PICTURE] },
  { name: 'Review', groups: [EDITING] },
  { name: 'View', groups: [] },
]

// Studio's inspector: the same commands as cards.
export const panels = [
  { id: 'text', icon: 'text', label: 'Text', groups: [FONT, FONT_FACES] },
  { id: 'paragraph', icon: 'paragraph', label: 'Paragraph', groups: [PARAGRAPH, PARA_LAYOUT] },
  { id: 'styles', icon: 'palette', label: 'Styles', groups: [STYLES] },
  { id: 'insert', icon: 'image', label: 'Insert', groups: [TABLES, ILLUSTRATIONS, PICTURE] },
  { id: 'table', icon: 'table', label: 'Table', groups: [TABLE_LAYOUT, CELLS] },
  { id: 'page', icon: 'frame', label: 'Page', groups: [PAGE_SETUP, BREAKS, PAGES] },
]

export const intents = {
  'edit.insert_text': { command: 'scroll.insert_text', args: {} },
  'edit.backspace': { command: 'scroll.delete', args: { back: true } },
  'edit.delete': { command: 'scroll.delete', args: { back: false } },
  'edit.enter': { command: 'scroll.enter', args: {} },
  'edit.tab': { command: 'scroll.tab', args: {} },
  'format.bold': { command: 'scroll.toggle', args: { prop: 'bold' } },
  'format.italic': { command: 'scroll.toggle', args: { prop: 'italic' } },
  'format.underline': { command: 'scroll.toggle', args: { prop: 'underline' } },
  'clip.cut': { command: 'scroll.delete', args: { back: false } },
  'clip.paste': { command: 'scroll.paste', args: {} },
}

export default { tabs, panels, intents }
