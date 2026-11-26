// WORLDSMITH as the Forge: every feature is a Tool of one of three kinds. A Hammer changes a running
// game through the helper, a Lens only looks at the screen and draws on top, a Chisel changes game
// files (always backed up). The pure parts: the kinds, the tools, what a tool needs, the last one used.

export const KINDS = {
  hammer: { name: 'Hammer', plain: 'live', promise: 'Changes a running game - your own offline single-player games only' },
  lens: { name: 'Lens', plain: 'screen', promise: 'Only looks at the screen and draws on top - the game is never touched' },
  chisel: { name: 'Chisel', plain: 'files', promise: 'Changes game files - always backed up, one press to undo' },
}

export const TOOLS = [
  { id: 'values', name: 'Values', kind: 'hammer', line: 'Find and change numbers', icon: 'anvil', needs: [] },
  { id: 'translate', name: 'Translate', kind: 'lens', line: 'Read the game in your language', icon: 'bubble', needs: ['rect', 'thumb'] },
]

export const TOOL_KEY = 'castle.worldsmithTool'
export const DEFAULT_TOOL = 'values'

/** Whether the helper's features cover what a tool needs. */
export function available(tool, features) {
  const have = new Set(Array.isArray(features) ? features.map(String) : [])
  const missing = (tool?.needs ?? []).filter((need) => !have.has(need))
  return { ok: missing.length === 0, missing }
}

/** A tool's kind as its card says it: "Hammer · live". */
export function toolLabel(tool) {
  const kind = KINDS[tool?.kind]
  return kind ? `${kind.name} · ${kind.plain}` : ''
}

/** The tool with this id, or undefined. */
export const toolById = (id) => TOOLS.find((t) => t.id === id)

/** The tool the member used last (a known id), else Values. */
export function lastTool(storage) {
  try {
    const id = storage?.getItem(TOOL_KEY)
    return toolById(id) ? id : DEFAULT_TOOL
  } catch {
    return DEFAULT_TOOL
  }
}

/** Remembers the tool picked; a storage that refuses is fine. */
export function rememberTool(storage, id) {
  if (!toolById(id)) return
  try {
    storage?.setItem(TOOL_KEY, id)
  } catch {
    // private window or blocked storage: the chooser simply opens on Values next time
  }
}

/** The helper's caps answer, read: its features, or none for an old helper. */
export function readFeatures(answer) {
  return Array.isArray(answer?.features) ? answer.features.map(String) : []
}
