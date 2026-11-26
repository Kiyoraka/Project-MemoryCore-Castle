import { firstHeading } from './markdown.js'

// What Chat draws from the session's record: the companion's words, the member's, pictures side by
// side, films, voice lines and songs, and plan cards. Tool steps and the plan-file and plan-end
// markers stay in Log; the trail to and from other sessions ("peer") only puts DEV chips on the
// companion's words (devchips.js).

const HIDDEN = new Set(['work', 'plan-file', 'plan-end', 'peer'])

// A plan card shows only its title and how many phases; the reader holds the whole plan.
export function planFace(text) {
  const title = firstHeading(text) || 'Plan'
  const phases = (text.match(/^#{2,3}\s+Phase\b/gim) || []).length
  return { title, phases }
}

// The newest plan file while it is still open: no plan card and no end of plan mode after it.
export function openPlanFile(entries) {
  const at = entries.findLastIndex((entry) => entry.role === 'plan-file')
  if (at < 0 || entries.slice(at + 1).some((entry) => entry.role === 'plan' || entry.role === 'plan-end')) return null
  return entries[at].text
}

// Whether an entry is one of the markers Chat never draws (they only say where a plan is written,
// or which session a message went to or came from).
export const isMarker = (entry) => entry?.role === 'plan-file' || entry?.role === 'plan-end' || entry?.role === 'peer'

const fileCard = (waiting, at) => ({
  kind: 'plan',
  id: `plan-file-${waiting.path}`,
  entry: { text: waiting.text, at },
  ...planFace(waiting.text),
})

// `waiting` is the plan read from its plan file while the question was up and the record had no
// plan yet ({ path, text, at }). It sits at the bottom only while that plan is still open; once plan
// mode ends with no plan in the record, it settles where the plan ended (a plan card belongs at the
// moment it was shown).
export function buildItems(entries, waiting = null) {
  const out = []
  const plansSeen = new Set()
  let cardSincePlanFile = false
  for (const entry of entries) {
    const last = out[out.length - 1]
    if (entry.role === 'plan-file') cardSincePlanFile = false
    if (entry.role === 'plan-end' && waiting && entry.text === waiting.path && !cardSincePlanFile && !plansSeen.has(waiting.text)) {
      out.push(fileCard(waiting, entry.at ?? waiting.at))
      plansSeen.add(waiting.text)
      cardSincePlanFile = true
    }
    if (HIDDEN.has(entry.role)) continue
    if (entry.role === 'plan') {
      // The same plan asked, then approved, is one card.
      if (!plansSeen.has(entry.text)) out.push({ kind: 'plan', id: entry.id, entry, ...planFace(entry.text) })
      plansSeen.add(entry.text)
      cardSincePlanFile = true
    } else if (entry.role === 'picture' && last?.kind === 'pictures') last.list.push(entry)
    else if (entry.role === 'picture') out.push({ kind: 'pictures', id: entry.id, list: [entry] })
    else if (entry.role === 'voice') out.push({ kind: 'voice', id: entry.id, entry })
    else if (entry.role === 'video') out.push({ kind: 'video', id: entry.id, entry })
    else out.push({ kind: 'message', id: entry.id, entry })
  }
  if (waiting && waiting.path === openPlanFile(entries) && !plansSeen.has(waiting.text)) {
    out.push(fileCard(waiting, waiting.at))
  }
  return out
}

/**
 * A sound the companion showed is a song, not a voice line, when it sits in a music or songs folder,
 * or runs past a minute outside a voice folder.
 */
export function songOf(entry, duration) {
  if (entry?.role !== 'voice' || !entry.path) return false
  if (/[\\/](music|songs?)[\\/]/i.test(entry.path)) return true
  if (isCompanionVoice(entry.path)) return false
  return Number.isFinite(duration) && duration > 60
}

/** Whether a sound is the companion's own voice: castle-voice.ps1 saves those in a voice folder. */
export function isCompanionVoice(path) {
  return /[\\/]voice[\\/]/i.test(String(path ?? ''))
}

/** A sound's own name: its file name without the extension, dashes and underscores as spaces. */
export function soundName(path) {
  const file = String(path ?? '').split(/[\\/]/).pop()
  return file.replace(/\.[^.]+$/, '').replace(/[-_]+/g, ' ').trim()
}

/**
 * The member's reactions from the phone ({ platform, words, emoji, at }, see bridges/reactions.rs)
 * placed on the companion's bubbles: each lands on the companion's newest message whose words open
 * with the remembered words, and a newer one on the same bubble wins. A reaction whose reply is
 * not in the chat shows nowhere. Returns new items; the given ones are left as they were.
 */
export function placeReactions(items, reactions) {
  if (!reactions?.length) return items
  const out = items.map((item) => item)
  const sorted = [...reactions].filter((r) => r?.emoji && r?.words).sort((a, b) => (a.at ?? 0) - (b.at ?? 0))
  for (const reaction of sorted) {
    const words = reaction.words.trim()
    const at = out.findLastIndex((item) => item.kind === 'message' && item.entry.role === 'companion' && (item.entry.text ?? '').trim().startsWith(words))
    if (at >= 0) out[at] = { ...out[at], reaction: reaction.emoji }
  }
  return out
}

/** A reaction that just arrived, put into the kept list: it replaces an older one on the same reply, or takes it back. */
export function withReaction(reactions, reaction) {
  const rest = (reactions ?? []).filter((r) => !(r.platform === reaction.platform && r.words === reaction.words))
  return reaction.emoji ? [...rest, reaction] : rest
}
