import { describe, expect, it } from 'vitest'
import {
  History,
  addImageLayer,
  addTextLayer,
  applyAdjust,
  blendChannel,
  blendPixel,
  cleanName,
  cloneDab,
  compose,
  coverDab,
  cropDoc,
  cropPixels,
  dab,
  deleteLayer,
  duplicateLayer,
  emptyAdjust,
  eraseDab,
  flipDoc,
  flipPixels,
  floodSelect,
  hexToRgba,
  isAdjustEmpty,
  makePixels,
  maskBounds,
  maskForSelection,
  maskFromLasso,
  maskFromRect,
  maskOutline,
  moveLayer,
  newDoc,
  normaliseDoc,
  paintCover,
  renameLayer,
  resizeDoc,
  resizePixels,
  restorePixels,
  rgbToHex,
  rotateDoc,
  rotatePixels,
  setLayer,
  stripPixels,
} from './engine.js'

// A tiny picture from a list of [r, g, b, a] pixels.
function px(width, height, list) {
  const p = makePixels(width, height)
  list.forEach((c, i) => p.data.set(c, i * 4))
  return p
}
const at = (p, x, y) => Array.from(p.data.slice((y * p.width + x) * 4, (y * p.width + x) * 4 + 4))

describe('the document', () => {
  it('starts empty with a zero adjust and no selection', () => {
    const d = newDoc(40.4, 30)
    expect(d).toEqual({ width: 40, height: 30, layers: [], adjust: emptyAdjust(), selection: null })
    expect(isAdjustEmpty(d.adjust)).toBe(true)
  })

  it('adds an image layer centred and a text layer on top', () => {
    let d = addImageLayer(newDoc(100, 100), 'data:x', 'Photo', 60, 40)
    expect(d.layers[0]).toMatchObject({ name: 'Photo', kind: 'image', x: 20, y: 30, opacity: 1, blend: 'normal', visible: true, png: 'data:x' })
    d = addTextLayer(d, 'Hello', 5, 6, { color: '#ff0000' })
    expect(d.layers[1]).toMatchObject({ kind: 'text', x: 5, y: 6, text: { value: 'Hello', color: '#ff0000' } })
    d = addImageLayer(d, 'data:y', 'Photo')
    expect(d.layers[2].name).toBe('Photo 2')
  })

  it('moves, duplicates, renames and sets layers', () => {
    let d = addImageLayer(addImageLayer(newDoc(10, 10), 'a', 'A'), 'b', 'B')
    const [a, b] = d.layers
    d = moveLayer(d, a.id, 1)
    expect(d.layers.map((l) => l.name)).toEqual(['B', 'A'])
    expect(moveLayer(d, a.id, 1)).toBe(d)
    d = duplicateLayer(d, b.id)
    expect(d.layers.map((l) => l.name)).toEqual(['B', 'B copy', 'A'])
    expect(d.layers[1].id).not.toBe(b.id)
    d = renameLayer(d, a.id, '  Sky  ')
    expect(d.layers[2].name).toBe('Sky')
    expect(renameLayer(d, a.id, '   ')).toBe(d)
    d = setLayer(d, a.id, { opacity: 0.5, blend: 'multiply' })
    expect(d.layers[2]).toMatchObject({ opacity: 0.5, blend: 'multiply' })
  })

  it('never deletes the last layer', () => {
    let d = addImageLayer(addImageLayer(newDoc(10, 10), 'a', 'A'), 'b', 'B')
    d = deleteLayer(d, d.layers[0].id)
    expect(d.layers.map((l) => l.name)).toEqual(['B'])
    expect(deleteLayer(d, d.layers[0].id)).toBe(d)
  })

  it('strips pixels for her and restores them from disk', () => {
    let d = addTextLayer(addImageLayer(newDoc(10, 10), 'data:image/png;base64,AAA', 'A'), 'Hi')
    d = { ...d, selection: { kind: 'mask', png: 'data:big' } }
    const s = stripPixels(d)
    expect(s.layers[0].png).toBeUndefined()
    expect(s.layers[1].text.value).toBe('Hi')
    expect(s.selection).toEqual({ kind: 'mask' })
    const back = restorePixels(normaliseDoc(s), [{ id: d.layers[0].id, png: 'BBB' }])
    expect(back.layers[0].png).toBe('data:image/png;base64,BBB')
  })

  it('normalises a document read back from disk', () => {
    const d = normaliseDoc({ width: 5, height: 4, layers: [{ id: 'x', kind: 'image', opacity: 3, blend: 'weird' }], adjust: { contrast: 400 } })
    expect(d.layers[0]).toMatchObject({ id: 'x', opacity: 1, blend: 'normal', visible: true, x: 0, y: 0 })
    expect(d.adjust.contrast).toBe(100)
    expect(d.adjust.exposure).toBe(0)
    expect(d.selection).toBeNull()
  })
})

describe('the document transforms', () => {
  const base = () => {
    const d = addImageLayer(newDoc(100, 50), 'a', 'A')
    return setLayer(d, d.layers[0].id, { x: 10, y: 5 })
  }

  it('crops by shifting layers and resizing', () => {
    const d = cropDoc(base(), { x: 4, y: 2, w: 30, h: 20 })
    expect(d).toMatchObject({ width: 30, height: 20, selection: null })
    expect(d.layers[0]).toMatchObject({ x: 6, y: 3 })
  })

  it('rotates a quarter turn either way and half', () => {
    const d = base()
    const sizes = { [d.layers[0].id]: { w: 20, h: 10 } }
    const cw = rotateDoc(d, 1, sizes)
    expect(cw).toMatchObject({ width: 50, height: 100 })
    expect(cw.layers[0]).toMatchObject({ x: 50 - (5 + 10), y: 10 })
    const ccw = rotateDoc(d, -1, sizes)
    expect(ccw.layers[0]).toMatchObject({ x: 5, y: 100 - (10 + 20) })
    const half = rotateDoc(d, 2, sizes)
    expect(half).toMatchObject({ width: 100, height: 50 })
    expect(half.layers[0]).toMatchObject({ x: 70, y: 35 })
    expect(rotateDoc(d, 4, sizes)).toBe(d)
  })

  it('flips layer positions', () => {
    const d = base()
    const sizes = { [d.layers[0].id]: { w: 20, h: 10 } }
    expect(flipDoc(d, 'h', sizes).layers[0]).toMatchObject({ x: 70, y: 5 })
    expect(flipDoc(d, 'v', sizes).layers[0]).toMatchObject({ x: 10, y: 35 })
  })

  it('resizes positions and text', () => {
    let d = addTextLayer(base(), 'T', 20, 10, { size: 10 })
    d = resizeDoc(d, 200, 100)
    expect(d).toMatchObject({ width: 200, height: 100 })
    expect(d.layers[0]).toMatchObject({ x: 20, y: 10 })
    expect(d.layers[1].text.size).toBe(20)
  })
})

describe('pixel transforms', () => {
  const R = [255, 0, 0, 255]
  const G = [0, 255, 0, 255]
  const B = [0, 0, 255, 255]
  const W = [255, 255, 255, 255]
  // R G B
  // W R G
  const pic = () => px(3, 2, [R, G, B, W, R, G])

  it('crops', () => {
    const c = cropPixels(pic(), { x: 1, y: 0, w: 2, h: 2 })
    expect([c.width, c.height]).toEqual([2, 2])
    expect(at(c, 0, 0)).toEqual(G)
    expect(at(c, 1, 1)).toEqual(G)
    expect(cropPixels(pic(), { x: 2, y: 1, w: 10, h: 10 }).width).toBe(1)
  })

  it('rotates clockwise, back and half', () => {
    const r = rotatePixels(pic(), 1)
    // W R
    // R G
    // G B
    expect([r.width, r.height]).toEqual([2, 3])
    expect(at(r, 0, 0)).toEqual(W)
    expect(at(r, 1, 0)).toEqual(R)
    expect(at(r, 1, 2)).toEqual(B)
    expect(Array.from(rotatePixels(r, -1).data)).toEqual(Array.from(pic().data))
    const h = rotatePixels(pic(), 2)
    expect(at(h, 0, 0)).toEqual(G)
    expect(at(h, 2, 1)).toEqual(R)
  })

  it('flips', () => {
    expect(at(flipPixels(pic(), 'h'), 0, 0)).toEqual(B)
    expect(at(flipPixels(pic(), 'v'), 0, 0)).toEqual(W)
  })

  it('resizes bilinearly', () => {
    const two = px(2, 1, [
      [0, 0, 0, 255],
      [200, 200, 200, 255],
    ])
    const big = resizePixels(two, 4, 1)
    expect(at(big, 0, 0)[0]).toBe(0)
    expect(at(big, 1, 0)[0]).toBe(50)
    expect(at(big, 2, 0)[0]).toBe(150)
    expect(at(big, 3, 0)[0]).toBe(200)
    const same = resizePixels(pic(), 3, 2)
    expect(Array.from(same.data)).toEqual(Array.from(pic().data))
  })
})

describe('blending', () => {
  it('has the six modes on a channel', () => {
    expect(blendChannel('normal', 0.2, 0.8)).toBe(0.2)
    expect(blendChannel('multiply', 0.5, 0.5)).toBe(0.25)
    expect(blendChannel('screen', 0.5, 0.5)).toBe(0.75)
    expect(blendChannel('overlay', 0.5, 0.25)).toBe(0.25)
    expect(blendChannel('overlay', 0.5, 0.75)).toBe(0.75)
    expect(blendChannel('darken', 0.3, 0.6)).toBe(0.3)
    expect(blendChannel('lighten', 0.3, 0.6)).toBe(0.6)
  })

  it('composites a pixel with opacity and alpha', () => {
    expect(blendPixel('normal', [255, 0, 0, 255], [0, 0, 255, 255])).toEqual([255, 0, 0, 255])
    expect(blendPixel('normal', [255, 0, 0, 255], [0, 0, 255, 255], 0.5)).toEqual([128, 0, 128, 255])
    expect(blendPixel('multiply', [128, 255, 0, 255], [255, 128, 255, 255])).toEqual([128, 128, 0, 255])
    // on a transparent backdrop every mode is just the source
    expect(blendPixel('multiply', [100, 50, 25, 255], [0, 0, 0, 0])).toEqual([100, 50, 25, 255])
    expect(blendPixel('screen', [0, 0, 0, 0], [10, 20, 30, 255])).toEqual([10, 20, 30, 255])
  })

  it('composes layers bottom to top', () => {
    const bottom = px(1, 1, [[200, 100, 50, 255]])
    const top = px(1, 1, [[255, 255, 255, 255]])
    expect(at(compose([bottom, top], [1, 1], ['normal', 'multiply']), 0, 0)).toEqual([200, 100, 50, 255])
    expect(at(compose([bottom, top], [1, 0], ['normal', 'normal']), 0, 0)).toEqual([200, 100, 50, 255])
    expect(at(compose([bottom, top], [1, 1], ['normal', 'screen']), 0, 0)).toEqual([255, 255, 255, 255])
  })
})

describe('adjustments', () => {
  const pic = () =>
    px(3, 3, [
      [10, 20, 30, 255],
      [200, 100, 50, 255],
      [128, 128, 128, 255],
      [0, 0, 0, 255],
      [255, 255, 255, 255],
      [60, 180, 90, 128],
      [90, 90, 200, 255],
      [30, 60, 90, 255],
      [240, 10, 10, 255],
    ])
  const run = (fields) => applyAdjust(pic(), { ...emptyAdjust(), ...fields })

  it('leaves pixels untouched at zero', () => {
    const p = pic()
    const out = applyAdjust(p, emptyAdjust())
    expect(Array.from(out.data)).toEqual(Array.from(p.data))
    expect(out.data).not.toBe(p.data)
  })

  it('exposure multiplies by 2^(v/100)', () => {
    expect(at(run({ exposure: 100 }), 0, 0)).toEqual([20, 40, 60, 255])
    expect(at(run({ exposure: -100 }), 1, 0)).toEqual([100, 50, 25, 255])
  })

  it('brightness adds', () => {
    expect(at(run({ brightness: 50 }), 0, 0)).toEqual([74, 84, 94, 255])
    expect(at(run({ brightness: -100 }), 1, 1)).toEqual([127, 127, 127, 255])
  })

  it('contrast works around 128', () => {
    expect(at(run({ contrast: 50 }), 2, 0)).toEqual([128, 128, 128, 255])
    expect(at(run({ contrast: -100 }), 1, 0)).toEqual([128, 128, 128, 255])
    expect(at(run({ contrast: 100 }), 0, 0)[0]).toBe(0)
  })

  it('highlights move the bright half, shadows the dark half', () => {
    expect(at(run({ highlights: -100 }), 1, 1)[0]).toBe(191)
    expect(at(run({ highlights: -100 }), 0, 1)[0]).toBe(0)
    expect(at(run({ shadows: 100 }), 0, 1)[0]).toBe(64)
    expect(at(run({ shadows: 100 }), 1, 1)[0]).toBe(255)
    expect(at(run({ shadows: 100 }), 2, 0)).toEqual([128, 128, 128, 255])
  })

  it('warmth raises red and lowers blue', () => {
    expect(at(run({ warmth: 100 }), 2, 0)).toEqual([158, 128, 98, 255])
  })

  it('saturation moves toward or away from gray', () => {
    const gray = run({ saturation: -100 })
    const [r, g, b] = at(gray, 1, 0)
    expect(r).toBe(g)
    expect(g).toBe(b)
    expect(at(run({ saturation: 100 }), 2, 0)).toEqual([128, 128, 128, 255])
    expect(at(run({ saturation: 50 }), 1, 0)[0]).toBeGreaterThan(200)
  })

  it('sharpness raises local contrast on a flat-free picture and keeps flat areas', () => {
    const flat = px(3, 3, Array(9).fill([90, 90, 90, 255]))
    expect(Array.from(applyAdjust(flat, { sharpness: 100 }).data)).toEqual(Array.from(flat.data))
    const spot = px(3, 3, [...Array(4).fill([100, 100, 100, 255]), [200, 200, 200, 255], ...Array(4).fill([100, 100, 100, 255])])
    expect(at(applyAdjust(spot, { sharpness: 100 }), 1, 1)[0]).toBeGreaterThan(200)
    expect(at(applyAdjust(spot, { sharpness: -100 }), 1, 1)[0]).toBeLessThan(200)
  })

  it('keeps alpha', () => {
    expect(at(run({ brightness: 40, saturation: 30 }), 2, 1)[3]).toBe(128)
  })
})

describe('selections', () => {
  it('flood selects connected similar pixels', () => {
    const A = [10, 10, 10, 255]
    const N = [14, 12, 10, 255]
    const X = [250, 0, 0, 255]
    // A N X
    // X A X
    // A A A
    const p = px(3, 3, [A, N, X, X, A, X, A, A, A])
    const mask = floodSelect(p, 0, 0, 8)
    expect(Array.from(mask)).toEqual([255, 255, 0, 0, 255, 0, 255, 255, 255])
    expect(Array.from(floodSelect(p, 0, 0, 2))).toEqual([255, 0, 0, 0, 0, 0, 0, 0, 0])
    expect(floodSelect(p, 9, 9, 8).every((v) => v === 0)).toBe(true)
  })

  it('makes rect and lasso masks and finds their bounds', () => {
    const r = maskFromRect(4, 3, { x: 1, y: 1, w: 2, h: 5 })
    expect(Array.from(r)).toEqual([0, 0, 0, 0, 0, 255, 255, 0, 0, 255, 255, 0])
    expect(maskBounds(r, 4, 3)).toEqual({ x: 1, y: 1, w: 2, h: 2 })
    expect(maskBounds(new Uint8Array(12), 4, 3)).toBeNull()
    // a triangle over the left half
    const l = maskFromLasso(4, 4, [
      [0, 0],
      [4, 0],
      [0, 4],
    ])
    expect(l[0]).toBe(255)
    expect(l[3]).toBe(0)
    expect(l[15]).toBe(0)
    expect(l[8]).toBe(255)
    expect(maskForSelection({ kind: 'rect', x: 0, y: 0, w: 1, h: 1 }, 2, 2)[0]).toBe(255)
    expect(maskForSelection(null, 2, 2)).toBeNull()
  })

  it('outlines a mask with merged runs', () => {
    const r = maskFromRect(4, 3, { x: 1, y: 1, w: 2, h: 2 })
    expect(maskOutline(r, 4, 3)).toBe('M1 1H3M1 3H3M1 1V3M3 1V3')
  })
})

describe('painting', () => {
  it('a hard dab fills its disc with colour', () => {
    const p = makePixels(5, 5)
    const box = dab(p, 2.5, 2.5, 3, 1, [255, 0, 0, 255])
    expect(at(p, 2, 2)).toEqual([255, 0, 0, 255])
    expect(at(p, 0, 0)).toEqual([0, 0, 0, 0])
    expect(box).toEqual({ x: 1, y: 1, w: 3, h: 3 })
  })

  it('a dab respects opacity and the mask', () => {
    const p = makePixels(4, 1, [0, 0, 255, 255])
    const mask = new Uint8Array([255, 255, 0, 0])
    dab(p, 2, 0.5, 10, 1, [255, 0, 0, 255], 0.5, mask)
    expect(at(p, 0, 0)).toEqual([128, 0, 128, 255])
    expect(at(p, 3, 0)).toEqual([0, 0, 255, 255])
  })

  it('a dab reads an offset mask', () => {
    const p = makePixels(2, 1)
    // the layer sits at x=1 in a 3-wide document; only document pixel 2 is selected
    dab(p, 1, 0.5, 10, 1, [0, 255, 0, 255], 1, { data: new Uint8Array([0, 0, 255]), width: 3, height: 1, ox: 1, oy: 0 })
    expect(at(p, 0, 0)[3]).toBe(0)
    expect(at(p, 1, 0)).toEqual([0, 255, 0, 255])
  })

  it('erases', () => {
    const p = makePixels(3, 3, [9, 9, 9, 255])
    eraseDab(p, 1.5, 1.5, 1, 1)
    expect(at(p, 1, 1)[3]).toBe(0)
    expect(at(p, 0, 0)[3]).toBe(255)
  })

  it('clones from a source', () => {
    const p = px(4, 1, [
      [255, 0, 0, 255],
      [0, 0, 0, 255],
      [0, 0, 0, 255],
      [0, 0, 0, 255],
    ])
    cloneDab(p, { x: 0.5, y: 0.5 }, { x: 3.5, y: 0.5 }, 1, 1, px(4, 1, [[255, 0, 0, 255], [0, 0, 0, 255], [0, 0, 0, 255], [0, 0, 0, 255]]))
    expect(at(p, 3, 0)).toEqual([255, 0, 0, 255])
    expect(at(p, 2, 0)).toEqual([0, 0, 0, 255])
  })

  it('a covered stroke never builds past its opacity', () => {
    const base = makePixels(3, 1, [0, 0, 0, 0])
    const target = makePixels(3, 1)
    const cover = new Float32Array(3)
    let box = coverDab(cover, 3, 1, 1.5, 0.5, 4, 1)
    box = coverDab(cover, 3, 1, 1.5, 0.5, 4, 1)
    paintCover(target, base, cover, box, { rgba: [255, 255, 255, 255], opacity: 0.5 })
    expect(at(target, 1, 0)).toEqual([255, 255, 255, 128])
    const erased = makePixels(3, 1)
    paintCover(erased, makePixels(3, 1, [5, 5, 5, 255]), cover, box, { erase: true, opacity: 1 })
    expect(at(erased, 1, 0)[3]).toBe(0)
  })

  it('reads and writes hex colours', () => {
    expect(hexToRgba('#f80')).toEqual([255, 136, 0, 255])
    expect(hexToRgba('#7c5cff')).toEqual([124, 92, 255, 255])
    expect(rgbToHex(124, 92, 255)).toBe('#7c5cff')
  })
})

describe('history', () => {
  it('undoes, redoes, and a push clears redo', () => {
    const h = new History()
    h.reset('a')
    h.push('b')
    h.push('c')
    expect(h.undo()).toBe('b')
    expect(h.undo()).toBe('a')
    expect(h.undo()).toBeNull()
    expect(h.redo()).toBe('b')
    h.push('d')
    expect(h.canRedo).toBe(false)
    expect(h.undo()).toBe('b')
  })

  it('keeps 40 snapshots', () => {
    const h = new History()
    h.reset(0)
    for (let i = 1; i <= 50; i++) h.push(i)
    expect(h.stack.length).toBe(40)
    let last
    while (h.canUndo) last = h.undo()
    expect(last).toBe(11)
  })
})

it('cleans a name for a file', () => {
  expect(cleanName('  my/photo:*1  ')).toBe('myphoto1')
  expect(cleanName('')).toBe('Untitled')
})
