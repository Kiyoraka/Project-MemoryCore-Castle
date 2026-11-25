// The member's messages' ticks in Chat, like a messenger:
//   sending - typed into the terminal, not in the companion's record yet      one grey tick
//   queued  - waiting in the queue while the companion works ("ctrl+x ctrl+s to send now")   one tick
//   read    - the companion took it in (mid-turn, or as the companion's next turn's words)               two ticks
//   notSent - the terminal kept the words in its box after a second Enter: "Not sent · Send again"

const norm = (text) => String(text ?? '').replace(/\s+/g, ' ').trim()
// A queued message the member took back (edited, cleared) leaves Chat instead of being read.
const TAKEN_BACK = /cancel|edit|pop|clear|discard/i

function lastIndex(list, test) {
  for (let i = list.length - 1; i >= 0; i -= 1) if (test(list[i])) return i
  return -1
}

// Folds a poll's new entries into the list.
export function mergeEntries(list, incoming) {
  const out = [...list]
  for (const e of incoming) {
    if (e.role === 'receipt') {
      const i = lastIndex(out, (m) => m.role === 'member' && m.state === 'queued' && norm(m.text) === norm(e.text))
      if (i >= 0) {
        if (TAKEN_BACK.test(e.state ?? '')) out.splice(i, 1)
        else out[i] = { ...out[i], state: 'read' }
      }
      continue
    }
    if (e.role === 'member') {
      // the line in the companion's record answers a message the member just sent from here
      const local = out.findIndex((m) => m.local && norm(m.text) === norm(e.text))
      if (local >= 0) out.splice(local, 1)
      if (e.state === 'queued') {
        out.push({ ...e, fromQueue: true })
        continue
      }
      // the companion's next turn's words: when they came from the queue they are already shown
      const queued = lastIndex(out, (m) => m.fromQueue && !m.answered && norm(m.text) === norm(e.text))
      if (queued >= 0) {
        out[queued] = { ...out[queued], state: 'read', answered: true }
        continue
      }
      out.push({ ...e, state: 'read' })
      continue
    }
    out.push(e)
  }
  return out
}

// A full reload: the companion's record again, with the member's just-sent messages it does not hold yet kept at the end.
export function resetEntries(old, incoming) {
  const merged = mergeEntries([], incoming)
  const held = new Set(merged.filter((m) => m.role === 'member').map((m) => norm(m.text)))
  return [...merged, ...old.filter((m) => m.local && !held.has(norm(m.text)))]
}

let localCount = 0
// The bubble shown the moment the member presses send.
export function localSend(text, now = new Date()) {
  localCount += 1
  return { id: `local-${localCount}`, role: 'member', text, at: now.toISOString(), state: 'sending', local: true }
}

// ---------- A message that did not get through ----------
// Claude Code can take the words and their Enter as one paste and keep the words in its box unsent.
// After one more Enter, a bubble whose words still sit there is marked notSent ("Not sent · Send
// again"); Send again clears the mark. A read entry from the record replaces the bubble, mark and all.

/** The member's bubble with this id marked as not sent. */
export function markNotSent(list, id) {
  return list.map((m) => (m.id === id && m.local ? { ...m, notSent: true } : m))
}

/** The member's bubble with this id travelling again (Send again). */
export function clearNotSent(list, id) {
  return list.map((m) => {
    if (m.id !== id || !m.notSent) return m
    const rest = { ...m }
    delete rest.notSent
    return rest
  })
}

/** Whether the bubble with this id is still on its way: shown here, not yet in the record, not marked. */
export function stillSending(list, id) {
  return list.some((m) => m.id === id && m.local && m.state === 'sending' && !m.notSent)
}
