// The ribbon registry: each editor's tabs, groups, buttons, inspector panels and key intents, read from
// its own editors/<editor>/index.js (one file per editor lane) and made safe to draw. Pure; nothing here
// knows any host.
//
//   button   { icon, title, command, args, stateKey }   (or `intent` in place of `command`)
//   group    { label, buttons }
//   tab      { name, groups }
//   panel    { id, icon, label, groups }                 Studio's inspector
//   intents  { '<neutral intent>': { command, args } }   input.js names -> the editor's bus commands
import { editorOf } from './room.js'
import scroll from './editors/scroll/index.js'
import ledger from './editors/ledger/index.js'
import stage from './editors/stage/index.js'
import folio from './editors/folio/index.js'

export const REGISTRY = { scroll, ledger, stage, folio }

export const FIND_SOON = 'Find comes to this editor soon'

const obj = (v) => (v && typeof v === 'object' && !Array.isArray(v) ? v : {})
const list = (v) => (Array.isArray(v) ? v : [])

/** One button made safe: null when it neither names a command nor an intent. */
export function normalButton(b, commands = null) {
  const raw = obj(b)
  const command = typeof raw.command === 'string' && raw.command ? raw.command : null
  const intent = typeof raw.intent === 'string' && raw.intent ? raw.intent : null
  if (!command && !intent) return null
  return {
    icon: String(raw.icon ?? ''),
    title: String(raw.title ?? command ?? intent),
    command,
    intent,
    args: obj(raw.args),
    stateKey: typeof raw.stateKey === 'string' && raw.stateKey ? raw.stateKey : null,
    // a button whose command the open document does not know yet is shown, but cannot be pressed
    ready: !!intent || commands === null || commands.includes(command),
  }
}

function normalGroups(groups, commands) {
  return list(groups)
    .map((g) => ({ label: String(obj(g).label ?? ''), buttons: list(obj(g).buttons).map((b) => normalButton(b, commands)).filter(Boolean) }))
    .filter((g) => g.buttons.length)
}

/** An editor's whole interface, made safe: { tabs: [{ name, groups }], panels, intents }. `commands` (the
 * open document's command names, from describe) marks which buttons are ready; null marks them all. */
export function editorUi(editor, commands = null) {
  const src = obj(REGISTRY[editorOf(editor).id])
  const tabs = list(src.tabs)
    .filter((t) => t && typeof t.name === 'string' && t.name)
    .map((t) => ({ name: t.name, groups: normalGroups(t.groups, commands) }))
  const panels = list(src.panels)
    .filter((p) => p && (p.id || p.label))
    .map((p) => ({ id: String(p.id ?? p.label), icon: String(p.icon ?? ''), label: String(p.label ?? p.id), groups: normalGroups(p.groups, commands) }))
  return { tabs: tabs.length ? tabs : [{ name: 'Home', groups: [] }], panels, intents: obj(src.intents) }
}

/** The ribbon's tab names for an editor. */
export function ribbonTabs(editor) {
  return editorUi(editor).tabs.map((t) => t.name)
}

/** One tab's groups (the first tab's for a name it does not have). */
export function tabGroups(ui, name) {
  const tabs = ui?.tabs ?? []
  return (tabs.find((t) => t.name === name) ?? tabs[0])?.groups ?? []
}

/** The editor's own command for a neutral intent: { command, args }, or null when it has none. */
export function intentFor(editor, intent) {
  const entry = obj(obj(REGISTRY[editorOf(editor).id]).intents)[intent]
  if (!entry || typeof entry.command !== 'string' || !entry.command) return null
  return { command: entry.command, args: obj(entry.args) }
}

/** True when the state JSON says the button's `stateKey` is on (a dotted key reads nested objects). */
export function pressed(state, key) {
  if (!key) return false
  let v = state
  for (const part of String(key).split('.')) {
    if (!v || typeof v !== 'object') return false
    v = v[part]
  }
  return v === true
}

/** The command a button press runs: its args with the selection as `sel` (left out without one). */
export function buttonCommand(button, sel = null) {
  const args = { ...obj(button?.args) }
  if (sel) args.sel = sel
  return { command: button?.command ?? null, args }
}

/** The command names of a describe's `commands` list. */
export function commandNames(described) {
  return list(described?.commands)
    .map((c) => (typeof c === 'string' ? c : c?.name))
    .filter((n) => typeof n === 'string' && n)
}

/** What the find bar can do for an editor: { find, replace, findCommand, replaceCommand }. */
export function findSupport(editor, commands) {
  const id = editorOf(editor).id
  const names = list(commands)
  return { find: names.includes(`${id}.find`), replace: names.includes(`${id}.replace`), findCommand: `${id}.find`, replaceCommand: `${id}.replace` }
}

/** Plain words for an intent the editor does not answer yet. */
export function notYet(editor, intent) {
  const label = editorOf(editor).label
  const what = String(intent ?? '')
  if (what === 'edit.insert_text' || what.startsWith('edit.')) return `Typing comes to ${label} soon.`
  if (what.startsWith('format.')) return `Formatting comes to ${label} soon.`
  if (what === 'clip.paste' || what === 'clip.cut') return `Paste and cut come to ${label} soon.`
  return `That comes to ${label} soon.`
}
