// PAINT's SVG allow-list, in the page (the same rules as paint.rs, which cleans first): every
// drawing of the companion's is rebuilt from the allowed elements and presentational attributes
// before it is shown. `url(#id)` is the only url; scripts, handlers, links, images, foreignObject
// and styles never survive; the root's viewBox is the canvas's. Pure, so it is tested without a page.

export const SVG_CAP = 400 * 1024

const ELEMENTS = ['svg', 'g', 'path', 'rect', 'circle', 'ellipse', 'line', 'polyline', 'polygon', 'defs', 'linearGradient', 'radialGradient', 'stop', 'title', 'desc']
const DRAWN = new Set(['path', 'rect', 'circle', 'ellipse', 'line', 'polyline', 'polygon'])
const ATTRIBUTES = [
  'd', 'points', 'x', 'y', 'x1', 'y1', 'x2', 'y2', 'cx', 'cy', 'r', 'rx', 'ry', 'width', 'height', 'transform', 'fill', 'stroke', 'stroke-width', 'stroke-linecap',
  'stroke-linejoin', 'stroke-dasharray', 'opacity', 'fill-opacity', 'stroke-opacity', 'fill-rule', 'offset', 'stop-color', 'stop-opacity', 'gradientUnits',
  'gradientTransform', 'fx', 'fy', 'id', 'viewBox',
]
const MAX_DEPTH = 48

const byLower = (list) => new Map(list.map((n) => [n.toLowerCase(), n]))
const ELEMENT_OF = byLower(ELEMENTS)
const ATTRIBUTE_OF = byLower(ATTRIBUTES)

const ENTITIES = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'" }

export function decodeEntities(text) {
  return String(text).replace(/&(#x[0-9a-f]+|#[0-9]+|[a-z]+);/gi, (whole, name) => {
    if (name[0] === '#') {
      const code = name[1] === 'x' || name[1] === 'X' ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10)
      return Number.isFinite(code) && code >= 0 && code <= 0x10ffff ? String.fromCodePoint(code) : whole
    }
    return ENTITIES[name] ?? whole
  })
}

const escape = (text) => String(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')

/** An id kept: letters, digits, - and _, starting with a letter, at most 64. */
export const plainId = (id) => /^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(id)

/** An attribute's value is kept when its only url is url(#id) and nothing could reach outside. */
export function valueOk(name, value) {
  if (name === 'id') return plainId(value)
  if (value.length > SVG_CAP || /[<>`\\:;]/.test(value) || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(value)) return false
  const lower = value.toLowerCase()
  if (lower.includes('javascript') || lower.includes('expression')) return false
  let rest = lower
  let at
  while ((at = rest.indexOf('url')) >= 0) {
    const tail = rest.slice(at + 3).trimStart()
    if (tail[0] !== '(') return false
    const close = tail.indexOf(')')
    if (close < 0) return false
    const target = tail.slice(1, close).trim()
    if (!target.startsWith('#') || !plainId(target.slice(1))) return false
    rest = tail.slice(close + 1)
  }
  return true
}

// one tag read from just after '<': { tag, used } or null when cut off
function readTag(s) {
  let i = 0
  const closing = s[0] === '/'
  if (closing) i++
  const start = i
  while (i < s.length && !/[\s>/]/.test(s[i])) i++
  const name = s.slice(start, i)
  const attrs = []
  let selfClosing = false
  for (;;) {
    while (i < s.length && /\s/.test(s[i])) i++
    if (i >= s.length) return null
    if (s[i] === '>') return { tag: { name, attrs, closing, selfClosing }, used: i + 1 }
    if (s[i] === '/') {
      selfClosing = true
      i++
      continue
    }
    selfClosing = false
    const a = i
    while (i < s.length && !/[\s=>/]/.test(s[i])) i++
    const attr = s.slice(a, i)
    while (i < s.length && /\s/.test(s[i])) i++
    let value = ''
    if (s[i] === '=') {
      i++
      while (i < s.length && /\s/.test(s[i])) i++
      if (i >= s.length) return null
      const q = s[i]
      if (q === '"' || q === "'") {
        const end = s.indexOf(q, i + 1)
        if (end < 0) return null
        value = s.slice(i + 1, end)
        i = end + 1
      } else {
        const v = i
        while (i < s.length && !/[\s>]/.test(s[i])) i++
        value = s.slice(v, i)
      }
    }
    if (!attr) {
      if (i === a) i++
      continue
    }
    attrs.push([attr, value])
  }
}

// an id given a layer's own prefix (ids are global in the page, so two layers never share one);
// a prefix this cleaner added before is taken off first, so it never grows
const prefixed = (id, prefix) => (prefix ? prefix + id.replace(/^p\d+-/, '') : id)

/**
 * One SVG rebuilt from the allow-list. Returns { svg } or { error }. `prefix` (letters, digits, -)
 * is put before every id and every url(#id), so layers never borrow each other's gradients.
 */
export function cleanSvg(svg, width, height, prefix = '') {
  const text = String(svg ?? '')
  if (text.length > SVG_CAP) return { error: 'the drawing is larger than 400 KB' }
  if (prefix && !/^[A-Za-z][A-Za-z0-9-]*$/.test(prefix)) prefix = ''
  let out = ''
  const stack = []
  let skip = 0
  let drawn = 0
  let rest = text
  const inText = () => skip === 0 && (stack.at(-1) === 'title' || stack.at(-1) === 'desc')
  while (rest.length) {
    const lt = rest.indexOf('<')
    if (lt < 0) {
      if (inText()) out += escape(decodeEntities(rest))
      break
    }
    if (inText()) out += escape(decodeEntities(rest.slice(0, lt)))
    rest = rest.slice(lt)
    const skipTo = (open, close) => {
      const e = rest.indexOf(close, open.length)
      rest = e < 0 ? '' : rest.slice(e + close.length)
    }
    if (rest.startsWith('<!--')) { skipTo('<!--', '-->'); continue }
    if (rest.startsWith('<![CDATA[')) { skipTo('<![CDATA[', ']]>'); continue }
    if (rest.startsWith('<?')) { skipTo('<?', '?>'); continue }
    if (rest.startsWith('<!')) {
      const b = rest.indexOf('[')
      const g = rest.indexOf('>')
      if (b >= 0 && g >= 0 && b < g) skipTo('<!', ']>')
      else rest = g < 0 ? '' : rest.slice(g + 1)
      continue
    }
    const read = readTag(rest.slice(1))
    if (!read) break
    const { tag } = read
    rest = rest.slice(1 + read.used)
    if (skip > 0) {
      if (tag.closing) skip--
      else if (!tag.selfClosing) skip++
      continue
    }
    const name = ELEMENT_OF.get(tag.name.toLowerCase())
    if (tag.closing) {
      if (!name) continue
      const pos = stack.lastIndexOf(name)
      if (pos >= 0) {
        while (stack.length > pos) {
          const open = stack.pop()
          if (open !== 'svg') out += `</${open}>`
        }
      }
      continue
    }
    if (!name || stack.length >= MAX_DEPTH) {
      if (!tag.selfClosing) skip = 1
      continue
    }
    if (name === 'svg') {
      // every svg is unwrapped: the one root is ours, with the canvas's viewBox
      if (!tag.selfClosing) stack.push('svg')
      continue
    }
    if (DRAWN.has(name)) drawn++
    out += `<${name}`
    for (const [attr, raw] of tag.attrs) {
      const a = ATTRIBUTE_OF.get(attr.toLowerCase())
      if (!a || a === 'viewBox') continue
      let value = decodeEntities(raw).trim()
      if (!valueOk(a, value)) continue
      if (prefix && a === 'id') value = prefixed(value, prefix)
      else if (prefix) value = value.replace(/url\(\s*#([A-Za-z][A-Za-z0-9_-]*)\s*\)/gi, (_, id) => `url(#${prefixed(id, prefix)})`)
      out += ` ${a}="${escape(value)}"`
    }
    if (tag.selfClosing) out += '/>'
    else {
      out += '>'
      stack.push(name)
    }
  }
  while (stack.length) {
    const open = stack.pop()
    if (open !== 'svg') out += `</${open}>`
  }
  if (!drawn) return { error: 'nothing drawn was left after cleaning' }
  const whole = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${Math.round(width)} ${Math.round(height)}">${out}</svg>`
  if (whole.length > SVG_CAP) return { error: 'the drawing is larger than 400 KB' }
  return { svg: whole }
}

/** A cleaned SVG as a picture's address (for thumbnails and flattening), sized to the canvas. */
export function svgDataUrl(svg, width, height) {
  const sized = String(svg).replace(/^<svg /, `<svg width="${Math.round(width)}" height="${Math.round(height)}" `)
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(sized)}`
}
