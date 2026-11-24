// Settings > About's contributors wall, kept pure so it is tested: the counted people joined with
// the hand-written names and lists, in count order, the first `limit` shown until "Show all".

export const WALL_LIMIT = 8
export const NO_LIST = 'Thank you for your pull requests.'

// "Bernard Choong" -> "BC", "OneNonly148" -> "ON", "rin" -> "R".
export function initialsOf(name = '') {
  const words = String(name).trim().split(/[\s_-]+/).filter(Boolean)
  if (words.length > 1) return (words[0][0] + words[1][0]).toUpperCase()
  const word = words[0] || '?'
  const caps = word.match(/[A-Z]/g)
  return (caps && caps.length > 1 ? caps.slice(0, 2).join('') : word[0]).toUpperCase()
}

// -> { total, merged, hidden, people: [{ login, name, merged, built, initials }] }
export function wall(counts, people = {}, { all = false, limit = WALL_LIMIT } = {}) {
  const everyone = (counts?.people || [])
    .filter((p) => p?.login && p.merged > 0)
    .slice()
    .sort((a, b) => b.merged - a.merged || a.login.localeCompare(b.login))
    .map(({ login, merged, built: fromTitles }) => {
      const known = people[login] || {}
      const name = known.name || login
      // a list written by hand wins; else their own merged titles (counts.json); else the thank-you
      const built = known.built?.length ? known.built : fromTitles?.length ? fromTitles : [NO_LIST]
      return { login, name, merged, built, initials: initialsOf(name) }
    })
  const shown = all ? everyone : everyone.slice(0, limit)
  return {
    total: everyone.length,
    merged: everyone.reduce((sum, p) => sum + p.merged, 0),
    hidden: everyone.length - shown.length,
    people: shown,
  }
}

// The founder as a wall entry: no count, the Founder mark instead.
export function founderEntry(founder) {
  if (!founder?.login) return null
  const name = founder.name || founder.login
  return { login: founder.login, name, merged: 0, founder: true, built: founder.built?.length ? founder.built : [], initials: initialsOf(name) }
}

export const contributorsLine = (n) => `${n} ${n === 1 ? 'contributor' : 'contributors'}`
export const mergedLine = (n) => `${n} merged`
