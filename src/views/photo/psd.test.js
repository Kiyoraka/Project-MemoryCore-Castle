import { describe, expect, it } from 'vitest'
import { compose3 } from './engine3.js'
import { blendFromKey, psdToLayers, readPsd } from './psd.js'

// ---------- a tiny PSD writer, just for these tests ----------

class Bytes {
  constructor() {
    this.a = []
  }
  u8(v) {
    this.a.push(v & 255)
    return this
  }
  u16(v) {
    return this.u8(v >> 8).u8(v)
  }
  u32(v) {
    return this.u8(v >>> 24).u8(v >>> 16).u8(v >>> 8).u8(v)
  }
  str(s) {
    for (const c of s) this.u8(c.charCodeAt(0))
    return this
  }
  raw(arr) {
    for (const v of arr) this.u8(v)
    return this
  }
  get length() {
    return this.a.length
  }
}

// PackBits: runs of 2+ as repeats, everything else as literals.
function packBits(row) {
  const out = []
  let i = 0
  while (i < row.length) {
    let run = 1
    while (i + run < row.length && row[i + run] === row[i] && run < 128) run++
    if (run >= 2) {
      out.push((257 - run) & 255, row[i])
      i += run
      continue
    }
    const start = i
    while (i < row.length && i - start < 128 && !(i + 1 < row.length && row[i + 1] === row[i])) i++
    if (i === start) i++
    out.push(i - start - 1, ...row.slice(start, i))
  }
  return out
}

// A channel's data: compression, then raw bytes or row counts + PackBits rows.
function channelData(plane, w, h, compression) {
  const b = new Bytes().u16(compression)
  if (compression === 0) return b.raw(plane).a
  if (compression === 2) return b.raw([1, 2, 3, 4]).a
  const rows = []
  for (let y = 0; y < h; y++) rows.push(packBits(Array.from(plane.slice(y * w, y * w + w))))
  for (const r of rows) b.u16(r.length)
  for (const r of rows) b.raw(r)
  return b.a
}

function block(key, body) {
  const b = new Bytes().str('8BIM').str(key).u32(body.length).raw(body)
  if (body.length % 2) b.u8(0)
  return b.a
}

// layers: [{ name, rect: [top, left, bottom, right], planes: { 0, 1, 2, -1 }, compression, opacity,
// blendKey, clipping, hidden, unicode, divider, dividerBlend }] bottom to top.
function writePsd({ width, height, layers = [], merged = null, mergedCompression = 0, depth = 8, mode = 3, version = 1, channels = 4 }) {
  const b = new Bytes().str('8BPS').u16(version).raw([0, 0, 0, 0, 0, 0]).u16(channels).u32(height).u32(width).u16(depth).u16(mode)
  b.u32(0) // colour mode data
  b.u32(0) // image resources
  const info = new Bytes()
  if (layers.length) {
    info.u16(layers.length)
    const datas = []
    for (const l of layers) {
      const [top, left, bottom, right] = l.rect || [0, 0, 0, 0]
      const w = right - left
      const h = bottom - top
      const ids = Object.keys(l.planes || {}).map(Number)
      const chans = (ids.length ? ids : [0, 1, 2, -1]).map((id) => ({ id, data: w && h ? channelData(l.planes[id], w, h, l.compression ?? 0) : [0, 0] }))
      datas.push(chans)
      info.u32(top).u32(left).u32(bottom).u32(right).u16(chans.length)
      for (const c of chans) info.u16(c.id & 0xffff).u32(c.data.length)
      info.str('8BIM').str(l.blendKey || 'norm').u8(Math.round((l.opacity ?? 1) * 255)).u8(l.clipping ? 1 : 0).u8(l.hidden ? 2 : 0).u8(0)
      const extra = new Bytes().u32(0).u32(0)
      const name = l.name || ''
      extra.u8(name.length).str(name)
      while ((extra.length - 8) % 4) extra.u8(0)
      if (l.unicode) {
        const u = new Bytes().u32(l.unicode.length)
        for (const c of l.unicode) u.u16(c.charCodeAt(0))
        extra.raw(block('luni', u.a))
      }
      if (l.divider) {
        const d = new Bytes().u32(l.divider)
        if (l.dividerBlend) d.str('8BIM').str(l.dividerBlend)
        extra.raw(block('lsct', d.a))
      }
      if (l.extraKey) extra.raw(block(l.extraKey, [0, 0, 0, 0]))
      info.u32(extra.length).raw(extra.a)
    }
    for (const chans of datas) for (const c of chans) info.raw(c.data)
    if (info.length % 2) info.u8(0)
  }
  const section = new Bytes().u32(info.length).raw(info.a).u32(0) // + empty global mask info
  b.u32(layers.length ? section.length : 0)
  if (layers.length) b.raw(section.a)
  if (merged) {
    b.u16(mergedCompression)
    if (mergedCompression === 0) for (const p of merged) b.raw(p)
    else {
      const rows = []
      for (const p of merged) for (let y = 0; y < height; y++) rows.push(packBits(Array.from(p.slice(y * width, y * width + width))))
      for (const r of rows) b.u16(r.length)
      for (const r of rows) b.raw(r)
    }
  }
  return Uint8Array.from(b.a)
}

const fill = (n, v) => new Uint8Array(n).fill(v)
const pattern = (n) => Uint8Array.from({ length: n }, (_, i) => (i * 37) % 256)
const px = (p, x, y) => Array.from(p.data.slice((y * p.width + x) * 4, (y * p.width + x) * 4 + 4))

describe('readPsd', () => {
  it('reads two raster layers (raw and RLE) and the merged picture', () => {
    const W = 4
    const H = 3
    const bytes = writePsd({
      width: W,
      height: H,
      layers: [
        { name: 'Background', rect: [0, 0, H, W], planes: { 0: fill(12, 255), 1: fill(12, 0), 2: fill(12, 0), [-1]: fill(12, 255) }, compression: 0 },
        {
          name: 'green',
          unicode: 'Grün ✓',
          rect: [1, 1, 3, 4],
          planes: { [-1]: fill(6, 128), 0: fill(6, 0), 1: pattern(6), 2: fill(6, 10) },
          compression: 1,
          opacity: 0.5,
          blendKey: 'mul ',
          clipping: true,
          hidden: true,
        },
      ],
      merged: [pattern(12), fill(12, 7), fill(12, 9), fill(12, 255)],
      mergedCompression: 1,
    })
    const psd = readPsd(bytes)
    expect(psd.width).toBe(4)
    expect(psd.height).toBe(3)
    expect(psd.layers).toHaveLength(2)
    const [bg, green] = psd.layers
    expect(bg).toMatchObject({ name: 'Background', opacity: 1, blend: 'normal', visible: true, x: 0, y: 0, width: 4, height: 3, group: null, clipped: false })
    expect(px(bg.pixels, 3, 2)).toEqual([255, 0, 0, 255])
    expect(green).toMatchObject({ name: 'Grün ✓', blend: 'multiply', visible: false, x: 1, y: 1, width: 3, height: 2, clipped: true })
    expect(green.opacity).toBeCloseTo(0.5, 2)
    expect(px(green.pixels, 1, 0)).toEqual([0, 37, 10, 128])
    expect(px(green.pixels, 2, 1)).toEqual([0, (5 * 37) % 256, 10, 128])
    expect(px(psd.merged, 1, 0)).toEqual([37, 7, 9, 255])
    expect(psd.notes).toEqual([])
  })

  it('reads a raw merged picture without alpha, and a file with no layers', () => {
    const psd = readPsd(writePsd({ width: 2, height: 2, channels: 3, merged: [fill(4, 1), fill(4, 2), pattern(4)] }))
    expect(psd.layers).toEqual([])
    expect(px(psd.merged, 1, 0)).toEqual([1, 2, 37, 255])
  })

  it('reads group dividers and nests them for compose3', () => {
    const red = { 0: fill(4, 255), 1: fill(4, 0), 2: fill(4, 0), [-1]: fill(4, 255) }
    const psd = readPsd(
      writePsd({
        width: 2,
        height: 2,
        layers: [
          { name: 'white', rect: [0, 0, 2, 2], planes: { 0: fill(4, 255), 1: fill(4, 255), 2: fill(4, 255), [-1]: fill(4, 255) } },
          { name: '</Layer group>', divider: 3 },
          { name: 'red', rect: [0, 0, 2, 2], planes: red, compression: 1 },
          { name: 'Group 1', divider: 1, dividerBlend: 'pass', opacity: 0.5 },
        ],
        merged: [fill(4, 255), fill(4, 128), fill(4, 128)],
        channels: 3,
      }),
    )
    expect(psd.layers.map((l) => l.group)).toEqual([null, 'close', null, 'open'])
    expect(psd.layers[3]).toMatchObject({ name: 'Group 1', blend: 'passThrough', pixels: null })
    const layers = psdToLayers(psd)
    expect(layers.map((l) => l.kind)).toEqual(['image', 'image', 'group'])
    expect(layers[1].parent).toBe(layers[2].id)
    expect(layers[0].parent).toBeUndefined()
    const out = compose3(layers, 2, 2)
    const got = px(out, 0, 0)
    expect([got[0], got[3]]).toEqual([255, 255])
    expect(Math.abs(got[1] - 128)).toBeLessThanOrEqual(1)
  })

  it('notes what it skips: ZIP data and unreadable features', () => {
    const psd = readPsd(
      writePsd({
        width: 2,
        height: 2,
        layers: [{ name: 'zipped', rect: [0, 0, 2, 2], planes: { 0: fill(4, 1), 1: fill(4, 1), 2: fill(4, 1) }, compression: 2, extraKey: 'TySh' }],
        merged: [fill(4, 1), fill(4, 1), fill(4, 1)],
        channels: 3,
      }),
    )
    expect(psd.layers[0].pixels).toBeNull()
    expect(psd.notes.some((n) => n.includes('ZIP'))).toBe(true)
    expect(psd.notes.some((n) => n.includes('text'))).toBe(true)
    expect(psd.merged).not.toBeNull()
  })

  it('throws plain errors for what it cannot read', () => {
    expect(() => readPsd(writePsd({ width: 2, height: 2, depth: 16 }))).toThrow(/8-bit/)
    expect(() => readPsd(writePsd({ width: 2, height: 2, mode: 4 }))).toThrow(/RGB/)
    expect(() => readPsd(writePsd({ width: 2, height: 2, version: 2 }))).toThrow(/PSB/)
    expect(() => readPsd(new TextEncoder().encode('hello, this is not a psd file'))).toThrow(/not a Photoshop/)
    const cut = writePsd({ width: 2, height: 2, layers: [{ name: 'a', rect: [0, 0, 2, 2], planes: { 0: fill(4, 1), 1: fill(4, 1), 2: fill(4, 1) } }] })
    expect(() => readPsd(cut.slice(0, 60))).toThrow(/cut short/)
  })

  it('maps blend keys, unknown ones to normal', () => {
    expect(blendFromKey('scrn')).toBe('screen')
    expect(blendFromKey('fdiv')).toBe('divide')
    expect(blendFromKey('zzzz')).toBe('normal')
  })
})

describe('psd size guard', () => {
  it('refuses a file that asks for a canvas past the editor side before allocating it', async () => {
    const { readPsd, MAX_SIDE } = await import('./psd.js')
    const b = new Uint8Array(26)
    const v = new DataView(b.buffer)
    b.set([0x38, 0x42, 0x50, 0x53]) // 8BPS
    v.setUint16(4, 1)
    v.setUint16(12, 3)
    v.setUint32(14, MAX_SIDE + 1)
    v.setUint32(18, 100)
    v.setUint16(22, 8)
    v.setUint16(24, 3)
    expect(() => readPsd(b)).toThrow(/too big/)
  })
})
