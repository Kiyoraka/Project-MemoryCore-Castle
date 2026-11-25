// A chat table opened in its own window (TableSheet): edited, rows picked or ticked, and sent back
// to the chat box as markdown with the member's words. Pure: the sheet only shows and asks, and the
// message the table came from never changes.

const TRUE_MARKS = new Set(['x', '[x]', '✓', '✔', '☑', '✅', 'yes', 'y', 'done', 'true', 'pass', 'ok'])

/** Whether a cell's text reads as a tick ("X", "✓", "[x]", "done", "yes"). */
export function isTicked(text) {
  return TRUE_MARKS.has(String(text ?? '').trim().toLowerCase())
}

/**
 * A table from parseBlocks ({ head, rows }) as the sheet edits it:
 * { columns: [{ name, check }], rows: [{ cells, selected, ticked }], tickName, ticking }.
 * A short row is padded, a long one keeps its extra cells under unnamed columns.
 */
export function fromTable(head = [], rows = []) {
  const width = Math.max(head.length, ...rows.map((r) => r.length), 1)
  const columns = Array.from({ length: width }, (_, c) => ({ name: head[c] ?? '', check: false }))
  return {
    columns,
    rows: rows.map((r) => ({ cells: Array.from({ length: width }, (_, c) => r[c] ?? ''), selected: false, ticked: false })),
    tickName: 'Done',
    ticking: false,
  }
}

/** A copy of the model, so every edit returns a new one (the sheet keeps the old for nothing). */
function copy(model) {
  return {
    ...model,
    columns: model.columns.map((c) => ({ ...c })),
    rows: model.rows.map((r) => ({ ...r, cells: [...r.cells] })),
  }
}

export function setCell(model, row, column, value) {
  const next = copy(model)
  if (next.rows[row] && column >= 0 && column < next.columns.length) next.rows[row].cells[column] = value
  return next
}

/** A column switched to ticks reads its cells as ticks ("X" -> true); back to text writes them out. */
export function setCheck(model, column, check) {
  const next = copy(model)
  const col = next.columns[column]
  if (!col || col.check === check) return next
  col.check = check
  for (const row of next.rows) {
    row.cells[column] = check ? isTicked(row.cells[column]) : row.cells[column] ? 'x' : ''
  }
  return next
}

export function addRow(model, at = model.rows.length) {
  const next = copy(model)
  const cells = next.columns.map((c) => (c.check ? false : ''))
  next.rows.splice(Math.max(0, Math.min(at, next.rows.length)), 0, { cells, selected: false, ticked: false })
  return next
}

export function removeRow(model, row) {
  const next = copy(model)
  next.rows.splice(row, 1)
  return next
}

export function addColumn(model, name = 'New', at = model.columns.length) {
  const next = copy(model)
  const place = Math.max(0, Math.min(at, next.columns.length))
  next.columns.splice(place, 0, { name, check: false })
  for (const row of next.rows) row.cells.splice(place, 0, '')
  return next
}

/** The last column stays: a table needs one. */
export function removeColumn(model, column) {
  if (model.columns.length <= 1) return copy(model)
  const next = copy(model)
  next.columns.splice(column, 1)
  for (const row of next.rows) row.cells.splice(column, 1)
  return next
}

export function renameColumn(model, column, name) {
  const next = copy(model)
  if (next.columns[column]) next.columns[column].name = name
  return next
}

export function toggleSelected(model, row) {
  const next = copy(model)
  if (next.rows[row]) next.rows[row].selected = !next.rows[row].selected
  return next
}

/** Every row picked, or none when all were. */
export function selectAll(model) {
  const next = copy(model)
  const all = next.rows.length > 0 && next.rows.every((r) => r.selected)
  for (const row of next.rows) row.selected = !all
  return next
}

/** A tick is a row's answer ("tested", "agree"); the first one turns the tick column on. */
export function toggleTicked(model, row) {
  const next = copy(model)
  if (next.rows[row]) {
    next.rows[row].ticked = !next.rows[row].ticked
    next.ticking = true
  }
  return next
}

export function setTickName(model, name) {
  return { ...copy(model), tickName: name }
}

/** A cell as markdown: one line, its pipes escaped. Ticks read [x] / [ ] for the companion. */
export function cellText(value, check = false) {
  if (check) return value === true || isTicked(value) ? '[x]' : '[ ]'
  return String(value ?? '')
    .replace(/\r?\n/g, ' ')
    .replace(/\|/g, '\\|')
    .trim()
}

/**
 * The table as markdown. `rows`: 'all' or 'selected'. The tick column ("Done") is added at the end
 * once any row was ticked.
 */
export function toMarkdown(model, rows = 'all') {
  const picked = rows === 'selected' ? model.rows.filter((r) => r.selected) : model.rows
  const tick = model.ticking
  const head = [...model.columns.map((c) => cellText(c.name)), ...(tick ? [cellText(model.tickName || 'Done')] : [])]
  const line = (cells) => `| ${cells.join(' | ')} |`
  return [
    line(head),
    line(head.map(() => '---')),
    ...picked.map((r) => line([...r.cells.map((v, c) => cellText(v, model.columns[c]?.check)), ...(tick ? [cellText(r.ticked, true)] : [])])),
  ].join('\n')
}

/**
 * What goes into the chat box: the member's words first, then the table (only the picked rows when
 * `rows` is 'selected'). Nothing when there is nothing to send.
 */
export function message(model, context = '', rows = 'all') {
  const words = String(context ?? '').trim()
  if (rows === 'selected' && !model.rows.some((r) => r.selected)) return words
  const table = toMarkdown(model, rows)
  return words ? `${words}\n\n${table}` : table
}

/** How many rows are picked, for the Send button's label. */
export function selectedCount(model) {
  return model.rows.filter((r) => r.selected).length
}
