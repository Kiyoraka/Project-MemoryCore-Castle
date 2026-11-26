// PHOTO's PSD reader: opens a Photoshop file (8BPS, version 1, 8-bit RGB / RGBA) into plain layers
// with RGBA pixels, plus the merged picture saved at the end of the file. Raw and PackBits (RLE)
// channel data are read; anything else (ZIP data, masks, effects, text, smart objects, adjustment
// data) is skipped and named in `notes`. Big-endian throughout, like the format.

import { makePixels } from './engine.js'
import { BLEND_MODES } from './engine3.js'

// Photoshop's four-letter blend keys to our blend names; 'pass' is a group's pass through.
export const PSD_BLEND_KEYS = {
  pass: 'passThrough',
  norm: 'normal',
  diss: 'dissolve',
  dark: 'darken',
  'mul ': 'multiply',
  idiv: 'colorBurn',
  lbrn: 'linearBurn',
  dkCl: 'darkerColor',
  lite: 'lighten',
  scrn: 'screen',
  'div ': 'colorDodge',
  lddg: 'linearDodge',
  lgCl: 'lighterColor',
  over: 'overlay',
  sLit: 'softLight',
  hLit: 'hardLight',
  vLit: 'vividLight',
  lLit: 'linearLight',
  pLit: 'pinLight',
  hMix: 'hardMix',
  diff: 'difference',
  smud: 'exclusion',
  fsub: 'subtract',
  fdiv: 'divide',
  'hue ': 'hue',
  'sat ': 'saturation',
  colr: 'color',
  'lum ': 'luminosity',
}

export function blendFromKey(key) {
  const name = PSD_BLEND_KEYS[key]
  return name === 'passThrough' || BLEND_MODES.includes(name) ? name : 'normal'
}

// What a tagged block we do not read means for him.
const SKIPPED_KEYS = [
  [['TySh', 'tySh'], 'text layers are read as pixels; their text is not editable'],
  [['SoLd', 'SoLE', 'PlLd', 'plLd'], 'smart objects are read as pixels'],
  [['lrFX', 'lfx2', 'lmfx'], 'layer effects (styles) are skipped'],
  [['vmsk', 'vsms', 'vscg', 'vogk'], 'vector masks and shapes are skipped'],
  [['levl', 'curv', 'brit', 'blnc', 'hue2', 'hue ', 'selc', 'mixr', 'grdm', 'phfl', 'expA', 'vibA', 'thrs', 'nvrt', 'post', 'blwh', 'clrL'], 'adjustment layers are skipped'],
  [['SoCo', 'GdFl', 'PtFl'], 'fill layers are skipped'],
]

class Reader {
  constructor(bytes) {
    this.b = bytes
    this.v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
    this.p = 0
  }
  need(n) {
    if (n < 0 || this.p + n > this.b.length) throw new Error('This PSD file is cut short or damaged')
  }
  u8() {
    this.need(1)
    return this.b[this.p++]
  }
  u16() {
    this.need(2)
    const v = this.v.getUint16(this.p)
    this.p += 2
    return v
  }
  i16() {
    this.need(2)
    const v = this.v.getInt16(this.p)
    this.p += 2
    return v
  }
  u32() {
    this.need(4)
    const v = this.v.getUint32(this.p)
    this.p += 4
    return v
  }
  i32() {
    this.need(4)
    const v = this.v.getInt32(this.p)
    this.p += 4
    return v
  }
  bytes(n) {
    this.need(n)
    const out = this.b.subarray(this.p, this.p + n)
    this.p += n
    return out
  }
  str(n) {
    return String.fromCharCode(...this.bytes(n))
  }
  skip(n) {
    this.need(n)
    this.p += n
  }
}

// PackBits rows: counts[y] bytes each, decoded into one w x h plane.
function readRle(r, w, h, counts, plane = new Uint8Array(w * h)) {
  const b = r.b
  for (let y = 0; y < h; y++) {
    const end = r.p + counts[y]
    if (end > b.length) throw new Error('This PSD file is cut short or damaged')
    let o = y * w
    const rowEnd = o + w
    while (r.p < end && o < rowEnd) {
      let n = b[r.p++]
      if (n > 127) n -= 256
      if (n >= 0) {
        const take = Math.min(n + 1, end - r.p, rowEnd - o)
        plane.set(b.subarray(r.p, r.p + take), o)
        o += take
        r.p += n + 1
      } else if (n !== -128) {
        const v = b[r.p++]
        const run = Math.min(1 - n, rowEnd - o)
        plane.fill(v, o, o + run)
        o += run
      }
    }
    r.p = end
  }
  return plane
}

function toPixels(w, h, red, green, blue, alpha) {
  const out = makePixels(w, h)
  const d = out.data
  for (let j = 0, i = 0; j < w * h; j++, i += 4) {
    d[i] = red ? red[j] : 0
    d[i + 1] = green ? green[j] : 0
    d[i + 2] = blue ? blue[j] : 0
    d[i + 3] = alpha ? alpha[j] : 255
  }
  return out
}

function readRecord(r, notes) {
  const top = r.i32()
  const left = r.i32()
  const bottom = r.i32()
  const right = r.i32()
  const channelCount = r.u16()
  const channels = []
  for (let c = 0; c < channelCount; c++) channels.push({ id: r.i16(), length: r.u32() })
  if (r.str(4) !== '8BIM') throw new Error('A PSD layer record is damaged')
  const blendKey = r.str(4)
  const opacity = r.u8()
  const clipping = r.u8()
  const flags = r.u8()
  r.skip(1)
  const extraLength = r.u32()
  const extraEnd = r.p + extraLength
  const maskLength = r.u32()
  if (maskLength) notes.add('layer masks are skipped')
  r.skip(maskLength)
  r.skip(r.u32()) // blending ranges
  const nameLength = r.u8()
  let name = String.fromCharCode(...r.bytes(nameLength))
  r.skip((4 - ((1 + nameLength) % 4)) % 4)
  let group = null
  let collapsed = false
  let groupBlend = null
  while (r.p + 12 <= extraEnd) {
    const sig = r.str(4)
    if (sig !== '8BIM' && sig !== '8B64') break
    const key = r.str(4)
    const length = r.u32()
    const start = r.p
    if (key === 'luni') {
      const n = r.u32()
      let s = ''
      for (let k = 0; k < n; k++) s += String.fromCharCode(r.u16())
      name = s.replace(/\0+$/, '')
    } else if (key === 'lsct' || key === 'lsdk') {
      const type = r.u32()
      if (type === 1 || type === 2) {
        group = 'open'
        collapsed = type === 2
      } else if (type === 3) group = 'close'
      if (length >= 12) {
        r.skip(4) // '8BIM'
        groupBlend = r.str(4)
      }
    } else {
      for (const [keys, note] of SKIPPED_KEYS) if (keys.includes(key)) notes.add(note)
    }
    r.p = start + length + (length % 2)
  }
  r.p = extraEnd
  return { top, left, bottom, right, channels, blendKey: groupBlend || blendKey, opacity, clipping, flags, name, group, collapsed }
}

function readLayerInfo(r, sectionEnd, notes) {
  const infoLength = r.u32()
  if (!infoLength) {
    // 16-bit layers live in 'Lr16' outside the layer info; we only say so
    if (r.p + 12 <= sectionEnd) notes.add('no 8-bit layer data was found; only the merged picture is read')
    return []
  }
  const infoEnd = r.p + infoLength
  const count = Math.abs(r.i16())
  const records = []
  for (let k = 0; k < count; k++) records.push(readRecord(r, notes))
  const layers = []
  for (const rec of records) {
    const w = Math.max(0, rec.right - rec.left)
    const h = Math.max(0, rec.bottom - rec.top)
    if (w > MAX_SIDE || h > MAX_SIDE) throw new Error(`A layer in this PSD is too big to open here (over ${MAX_SIDE} pixels a side)`)
    const planes = new Map()
    let skipped = false
    for (const ch of rec.channels) {
      const start = r.p
      const next = start + ch.length
      if (ch.length < 2 || !w || !h) {
        r.p = next
        continue
      }
      const compression = r.u16()
      if (ch.id < -1) {
        r.p = next // user / vector mask: its own size, skipped
        continue
      }
      if (compression === 0) planes.set(ch.id, Uint8Array.from(r.bytes(w * h)))
      else if (compression === 1) {
        const counts = []
        for (let y = 0; y < h; y++) counts.push(r.u16())
        planes.set(ch.id, readRle(r, w, h, counts))
      } else {
        notes.add('ZIP-compressed layer data is skipped; those layers come in empty')
        skipped = true
      }
      r.p = next
    }
    layers.push({
      name: rec.name,
      opacity: rec.opacity / 255,
      blend: blendFromKey(rec.blendKey),
      visible: !(rec.flags & 2),
      x: rec.left,
      y: rec.top,
      width: w,
      height: h,
      pixels: w && h && !skipped ? toPixels(w, h, planes.get(0), planes.get(1), planes.get(2), planes.get(-1)) : null,
      group: rec.group,
      collapsed: rec.collapsed,
      clipped: rec.clipping === 1,
    })
  }
  r.p = infoEnd
  return layers
}

function readMerged(r, channels, w, h, notes) {
  if (r.p + 2 > r.b.length) {
    notes.add('the file has no merged picture')
    return null
  }
  const compression = r.u16()
  const n = w * h
  const planes = []
  if (compression === 0) {
    for (let c = 0; c < channels; c++) planes.push(Uint8Array.from(r.bytes(n)))
  } else if (compression === 1) {
    const counts = []
    for (let k = 0; k < channels * h; k++) counts.push(r.u16())
    for (let c = 0; c < channels; c++) planes.push(readRle(r, w, h, counts.slice(c * h, c * h + h)))
  } else {
    notes.add('the ZIP-compressed merged picture is skipped')
    return null
  }
  return toPixels(w, h, planes[0], planes[1], planes[2], channels >= 4 ? planes[3] : null)
}

// The editor's own largest side; a file asking for more is refused before anything is allocated.
export const MAX_SIDE = 12000

// Reads a PSD into { width, height, layers (bottom to top), merged, notes }. A group shows as two
// records: group 'close' (its bottom divider) comes before its children and group 'open' (the
// folder itself, with its name, opacity and blend) after them. Throws a plain Error for anything
// that is not an 8-bit RGB PSD.
export function readPsd(input) {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input)
  const r = new Reader(bytes)
  if (bytes.length < 26 || r.str(4) !== '8BPS') throw new Error('This is not a Photoshop (PSD) file')
  const version = r.u16()
  if (version === 2) throw new Error('Large-document (PSB) files are not supported')
  if (version !== 1) throw new Error(`Unknown PSD version ${version}`)
  r.skip(6)
  const channels = r.u16()
  const height = r.u32()
  const width = r.u32()
  const depth = r.u16()
  const mode = r.u16()
  if (depth !== 8) throw new Error(`Only 8-bit PSD files are supported; this one is ${depth}-bit`)
  if (mode !== 3) throw new Error('Only RGB PSD files are supported (this one is not RGB)')
  if (!width || !height || channels < 3) throw new Error('This PSD file has no RGB picture')
  if (width > MAX_SIDE || height > MAX_SIDE) throw new Error(`This PSD is too big to open here (over ${MAX_SIDE} pixels a side)`)
  const notes = new Set()
  r.skip(r.u32()) // colour mode data
  r.skip(r.u32()) // image resources
  const sectionLength = r.u32()
  const sectionEnd = r.p + sectionLength
  let layers = []
  if (sectionLength) {
    layers = readLayerInfo(r, sectionEnd, notes)
    r.p = sectionEnd
  }
  let merged = null
  try {
    merged = readMerged(r, channels, width, height, notes)
  } catch {
    notes.add('the merged picture is damaged and was skipped')
  }
  return { width, height, layers, merged, notes: [...notes] }
}

// The PSD's layers as compose3 layers: groups nested by parent id, dividers dropped.
export function psdToLayers(psd, idPrefix = 'psd') {
  const out = []
  const stack = []
  let groupCount = 0
  psd.layers.forEach((l, k) => {
    if (l.group === 'close') {
      stack.push(`${idPrefix}-group-${++groupCount}`)
      return
    }
    const parent = stack.length ? stack[stack.length - 1] : undefined
    const base = { name: l.name, opacity: l.opacity, blend: l.blend, visible: l.visible, clipped: l.clipped }
    if (l.group === 'open') {
      const id = stack.pop() || `${idPrefix}-group-${++groupCount}`
      out.push({ ...base, id, kind: 'group', parent: stack.length ? stack[stack.length - 1] : undefined, collapsed: l.collapsed })
      return
    }
    out.push({ ...base, id: `${idPrefix}-${k}`, kind: 'image', parent, pixels: l.pixels, x: l.x, y: l.y, blend: l.blend === 'passThrough' ? 'normal' : l.blend })
  })
  return out
}
