// The companion's words as markdown, read into plain blocks and spans for Chat bubbles and the plan reader.
// Never raw HTML: every piece is text the screen draws itself.

const FENCE = /^\s*```/
const HEADING = /^(#{1,6})\s+(.*)$/
const ITEM = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/
const BOX = /^\[([ xX])\]\s+/
const QUOTE = /^\s*>\s?(.*)$/
const RULE = /^\s*([-*_])(\s*\1){2,}\s*$/
const TABLE_ROW = /^\s*\|.*\|\s*$/
const TABLE_RULE = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/

// A row's cells; an escaped pipe (\|) stays inside its cell as |.
function cells(line) {
  return line
    .trim()
    .replace(/^\|/, '')
    .replace(/(?<!\\)\|$/, '')
    .split(/(?<!\\)\|/)
    .map((cell) => cell.replace(/\\\|/g, '|').trim())
}

/**
 * Blocks: { kind: 'p' | 'h1'..'h4' | 'item' | 'quote' | 'rule' | 'code' | 'table', ... }.
 * Consecutive plain lines join into one paragraph (line breaks kept); a table needs a header row
 * followed by a |---| rule.
 */
export function parseBlocks(text) {
  const lines = String(text ?? '').replace(/\r/g, '').split('\n')
  const out = []
  let paragraph = null
  const close = () => {
    if (paragraph) out.push({ kind: 'p', text: paragraph.join('\n') })
    paragraph = null
  }

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i]

    if (FENCE.test(line)) {
      close()
      const code = []
      i += 1
      while (i < lines.length && !FENCE.test(lines[i])) code.push(lines[i++])
      out.push({ kind: 'code', text: code.join('\n') })
      continue
    }

    if (TABLE_ROW.test(line) && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1])) {
      close()
      const head = cells(line)
      const rows = []
      i += 2
      while (i < lines.length && TABLE_ROW.test(lines[i])) rows.push(cells(lines[i++]))
      i -= 1
      out.push({ kind: 'table', head, rows })
      continue
    }

    const heading = line.match(HEADING)
    if (heading) {
      close()
      out.push({ kind: `h${Math.min(heading[1].length, 4)}`, text: heading[2].trim() })
      continue
    }

    if (RULE.test(line)) {
      close()
      out.push({ kind: 'rule' })
      continue
    }

    const item = line.match(ITEM)
    if (item) {
      close()
      const box = item[3].match(BOX)
      out.push({
        kind: 'item',
        depth: Math.min(3, Math.floor(item[1].replace(/\t/g, '  ').length / 2)),
        number: /\d/.test(item[2]) ? item[2].replace(')', '.') : null,
        box: Boolean(box),
        checked: Boolean(box && box[1].toLowerCase() === 'x'),
        text: box ? item[3].slice(box[0].length) : item[3],
      })
      continue
    }

    const quote = line.match(QUOTE)
    if (quote) {
      close()
      out.push({ kind: 'quote', text: quote[1] })
      continue
    }

    if (!line.trim()) {
      close()
      continue
    }
    paragraph = paragraph ?? []
    paragraph.push(line)
  }
  close()
  return out
}

// A bare web address in running text: http(s)://, up to a space or bracket, never ending on the
// punctuation that closes a sentence ("see https://x.y." links https://x.y).
const BARE_URL = /https?:\/\/[^\s<>()[\]`"']*[^\s<>()[\]`"'.,;:!?*]/
const isBareUrl = (piece) => new RegExp(`^${BARE_URL.source}$`).test(piece)

/** Spans for a plain chat line: only its web addresses become links ({ text, link? }). */
export function linkSpans(text) {
  return String(text ?? '')
    .split(new RegExp(`(${BARE_URL.source})`))
    .filter(Boolean)
    .map((piece) => (isBareUrl(piece) ? { text: piece, link: piece } : { text: piece }))
}

/** Spans inside a line: { text, bold?, em?, code?, link? }. */
// A whole bare address or [text](url) is a link, also inside **bold** or *italic*
// ("**https://claude.ai/artifact/…**" was drawn bold and could not be opened).
function linkOf(text) {
  if (isBareUrl(text)) return { text, link: text }
  const md = text.match(/^\[([^\]]+)\]\(([^)\s]+)\)$/)
  return md ? { text: md[1], link: md[2] } : { text }
}

export function parseSpans(text) {
  return String(text ?? '')
    .split(new RegExp(`(\`[^\`\\n]+\`|\\*\\*[^*\\n]+\\*\\*|\\[[^\\]\\n]+\\]\\([^)\\s]+\\)|${BARE_URL.source}|\\*[^*\\n]+\\*)`))
    .filter(Boolean)
    .map((piece) => {
      if (isBareUrl(piece)) return { text: piece, link: piece }
      if (piece.length > 2 && piece.startsWith('`') && piece.endsWith('`')) return { code: true, text: piece.slice(1, -1) }
      if (piece.length > 4 && piece.startsWith('**') && piece.endsWith('**')) return { bold: true, ...linkOf(piece.slice(2, -2)) }
      const link = linkOf(piece)
      if (link.link) return link
      if (piece.length > 2 && piece.startsWith('*') && piece.endsWith('*')) return { em: true, ...linkOf(piece.slice(1, -1)) }
      return { text: piece }
    })
}

/** The first heading of a text, or null. */
export function firstHeading(text) {
  return parseBlocks(text).find((b) => /^h\d$/.test(b.kind))?.text ?? null
}
