// The room as a chat: whose line is the member's own, who has read it (the ticks), and the
// letter drawn in a face circle when a speaker has no picture.

const AI_KINDS = ['assistant', 'member_assistant']
export const isAi = (author) => AI_KINDS.includes(author?.kind)

// Presence is not proof that this Castle still admits its companion: revoked keys can
// leave rows behind. Never interpret an unknown/stale status as online.
export function roomPresenceState(row, { me = null, companion = null, enabled = false, verified = false, fresh = false } = {}) {
  const owner = String(row.owner || row.ownerName || String(row.name ?? '').match(/^(.+)['\u2019]s\s+(.+)$/)?.[1] || '').trim()
  const own = isAi(row) && ((companion?.name && row.name === companion.name && row.kind === companion.kind) || (me?.name && owner === me.name))
  if (own && !enabled) return 'disabled'
  if (own && !verified) return 'unknown'
  if (!fresh) return 'unknown'
  return ['online', 'here', 'active', 'idle', 'away', 'offline'].includes(row.status) ? row.status : 'unknown'
}

// The service can retain several presence rows for revoked/recreated companion keys.
// Merge only the same raw companion identity, never the shortened display name.
export function roomMembers(rows = []) {
  const result = [], positions = new Map()
  const rank = (status) => ({ online: 3, here: 3, active: 3, idle: 2, away: 1, offline: 0 })[status] ?? 0
  for (const row of rows) {
    if (!isAi(row)) { result.push(row); continue }
    const key = JSON.stringify([row.kind, row.name])
    if (!positions.has(key)) { positions.set(key, result.length); result.push({ ...row }); continue }
    const index = positions.get(key), previous = result[index]
    const preferred = rank(row.status) > rank(previous.status) || (rank(row.status) === rank(previous.status) && Number(row.seen ?? 0) > Number(previous.seen ?? 0)) ? row : previous
    const other = preferred === row ? previous : row
    result[index] = { ...other, ...preferred, avatar: preferred.avatar || other.avatar, seen: Math.max(Number(previous.seen ?? 0), Number(row.seen ?? 0)) }
  }
  return result
}

// Display only: never rewrite the server identity used for ownership, avatars or replies.
export function speakerPresentation(author, hostName = '') {
  const raw = String(author?.name ?? '').trim()
  if (!isAi(author)) return { name: raw, owner: '', title: '' }
  const pair = raw.match(/^(.+)['’]s\s+(.+)$/)
  const name = String(author?.assistantName || pair?.[2] || raw).trim()
  const owner = String(author?.owner || author?.ownerName || pair?.[1] || (author?.kind === 'assistant' ? hostName : '')).trim()
  return { name, owner, title: owner ? `Master: ${owner}` : 'Companion' }
}

// The member's own line: one Castle posted for them (by id), or one matching their /me name and
// kind. A line from someone else who happens to share the name is never theirs by name alone
// unless the kind matches too.
export function isMine(line, myIds = [], me = null) {
  if (!line || line.author?.kind === 'system') return false
  if (myIds.includes(line.id)) return true
  return !!me?.name && line.author?.name === me.name && line.author?.kind === me.kind
}

// Who has read a line: every speaker whose seen reached its id, the member excluded.
export function readers(line, seen = [], me = null) {
  if (!line?.id) return []
  return seen
    .filter((s) => (s.seen ?? 0) >= line.id)
    .filter((s) => !(me && s.name === me.name && s.kind === me.kind))
    .map((s) => s.name)
}

// ✓ once the line is in the room, ✓✓ once someone else has read it.
export function ticks(line, seen, me) {
  if (!line?.id) return { mark: '', read: false, title: 'Sending…' }
  const who = readers(line, seen, me)
  return who.length ? { mark: '✓✓', read: true, title: `Read by ${who.join(', ')}` } : { mark: '✓', read: false, title: 'In the room' }
}

// The letter in a face circle without a picture: ✦ for an AI, else the first letter of the name.
export function initial(author) {
  if (isAi(author)) return '✦'
  const name = String(author?.name ?? '').trim()
  return name ? name[0].toUpperCase() : '?'
}

// Local calendar days, rather than elapsed 24-hour periods (including DST changes).
export function dayMarker(at, now = new Date()) {
  if (!at) return null
  const d = new Date(at)
  if (Number.isNaN(d.getTime())) return null
  const stamp = (x) => Date.UTC(x.getFullYear(), x.getMonth(), x.getDate())
  const age = (stamp(now) - stamp(d)) / 86400000
  const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
  const date = d.toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' })
  const label = age === 0 ? 'Today' : age === 1 ? 'Yesterday' : age > 1 && age < 7 ? d.toLocaleDateString('en-GB', { weekday: 'long' }) : date
  return { key, label, date }
}

export function datedLines(lines, now = new Date()) {
  let previous = null
  return lines.flatMap((line) => {
    const day = dayMarker(line.at, now)
    const separator = day && day.key !== previous ? [{ kind: 'day', ...day }] : []
    if (day) previous = day.key
    return [...separator, { kind: 'message', key: `line-${line.id}`, line }]
  })
}

// A failed image is retryable; concurrent messages share one request per avatar token.
export function avatarLoader(fetchFace, onFace, { now = Date.now, retryMs = 10000 } = {}) {
  const loaded = new Set(), pending = new Map(), failed = new Map()
  return async (token) => {
    if (!token || loaded.has(token)) return
    if (pending.has(token)) return pending.get(token)
    if (failed.has(token) && now() - failed.get(token) < retryMs) return
    const request = Promise.resolve().then(() => fetchFace(token)).then((face) => {
      if (!face) throw new Error('Empty avatar')
      onFace(token, face)
      loaded.add(token)
      failed.delete(token)
    }).catch(() => { failed.set(token, now()) }).finally(() => pending.delete(token))
    pending.set(token, request)
    return request
  }
}

export function currentAvatar(author, presence = []) {
  if (author?.avatar) return author.avatar
  const matches = presence.filter((p) => author?.name && p.name === author.name && p.kind === author.kind)
  return matches.length === 1 ? matches[0].avatar : null
}
