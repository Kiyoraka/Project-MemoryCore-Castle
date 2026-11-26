import { describe, expect, it } from 'vitest'
import { ADJUST_KEYS, adjustLabel, adjustPixels, applyAdjustOp, cleanAdjust, isNeutral, neutralAdjust, sharpen } from './adjust.js'
import { cleanLasso, dragRect, floodMask, lassoBounds, maskBounds, maskOutline, maskRuns, selectionBox } from './select.js'
import { cleanOp, cleanOps, compositeOf, cropDoc, docWithoutPixels, flipDoc, imageLayer, insertLayer, moveLayer, newDoc, readDoc, redrawLine, redrawSize, resizeDoc, rotateDoc, textLayer, turnBox } from './doc.js'
import { MAX_PILLS, addPill, pillLabel, undoLast, undoPill } from './history.js'

const px = (...rgba) => new Uint8ClampedArray(rgba)

describe('ADJUST', () => {
  it('has eight sliders, all at 0 to begin with, clamped to -100..100', () => {
    expect(ADJUST_KEYS).toHaveLength(8)
    expect(isNeutral(neutralAdjust())).toBe(true)
    expect(cleanAdjust({ exposure: 250, contrast: -12.6, evil: 3, warmth: 'x' })).toEqual({ ...neutralAdjust(), exposure: 100, contrast: -13 })
  })
  it('an adjust op adds or sets, and remembers only the sliders it changed', () => {
    const now = { ...neutralAdjust(), brightness: 90 }
    const rel = applyAdjustOp(now, { brightness: 30, contrast: 10, saturation: 0 })
    expect(rel.next.brightness).toBe(100)
    expect(rel.next.contrast).toBe(10)
    expect(rel.before).toEqual({ brightness: 90, contrast: 0 })
    const set = applyAdjustOp(now, { brightness: -20 }, 'set')
    expect(set.next.brightness).toBe(-20)
    expect(adjustLabel(rel.before, rel.next)).toBe('Brightness +10, Contrast +10')
  })
  it('neutral values leave every pixel as it was', () => {
    const data = px(10, 120, 250, 255, 0, 0, 0, 0)
    expect([...adjustPixels(new Uint8ClampedArray(data), 2, 1, neutralAdjust())]).toEqual([...data])
  })
  it('exposure and brightness lighten, and alpha never changes', () => {
    const out = adjustPixels(px(100, 100, 100, 77), 1, 1, { exposure: 50 })
    expect(out[0]).toBe(200)
    expect(out[3]).toBe(77)
    expect(adjustPixels(px(100, 100, 100, 255), 1, 1, { brightness: 40 })[0]).toBeGreaterThan(100)
    expect(adjustPixels(px(100, 100, 100, 255), 1, 1, { brightness: -40 })[0]).toBeLessThan(100)
  })
  it('contrast spreads from the middle and saturation -100 is grey', () => {
    const c = adjustPixels(px(64, 192, 128, 255), 3, 1, { contrast: 50 })
    expect(c[0]).toBeLessThan(64)
    expect(c[1]).toBeGreaterThan(192)
    const g = adjustPixels(px(200, 50, 10, 255), 1, 1, { saturation: -100 })
    expect(g[0]).toBe(g[1])
    expect(g[1]).toBe(g[2])
  })
  it('warmth moves toward red and away from blue; highlights touch the bright, shadows the dark', () => {
    const w = adjustPixels(px(100, 100, 100, 255), 1, 1, { warmth: 100 })
    expect(w[0]).toBeGreaterThan(100)
    expect(w[2]).toBeLessThan(100)
    const dark = adjustPixels(px(20, 20, 20, 255), 1, 1, { highlights: 100 })
    expect(dark[0]).toBe(20)
    const lifted = adjustPixels(px(20, 20, 20, 255), 1, 1, { shadows: 100 })
    expect(lifted[0]).toBeGreaterThan(60)
  })
  it('sharpness pushes an edge apart, and below 0 softens it', () => {
    const row = () => px(50, 50, 50, 255, 50, 50, 50, 255, 200, 200, 200, 255, 200, 200, 200, 255)
    const sharp = sharpen(row(), 4, 1, 1)
    expect(sharp[4]).toBeLessThan(50)
    expect(sharp[8]).toBeGreaterThan(200)
    const soft = sharpen(row(), 4, 1, -1)
    expect(soft[4]).toBeGreaterThan(50)
    expect(soft[8]).toBeLessThan(200)
    const flat = px(80, 80, 80, 255, 80, 80, 80, 255)
    expect([...sharpen(flat, 2, 1, 1)].slice(0, 3)).toEqual([80, 80, 80])
  })
})

describe('selections', () => {
  // 4 x 3: a red block at the left two columns, blue elsewhere, one near-red at (3, 2)
  const W = 4
  const H = 3
  const picture = () => {
    const d = new Uint8ClampedArray(W * H * 4)
    for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
      const i = (y * W + x) * 4
      const red = x < 2 || (x === 3 && y === 2)
      d.set(red ? [250, 10, 10, 255] : [10, 10, 250, 255], i)
    }
    d.set([240, 20, 15, 255], (2 * W + 3) * 4)
    return d
  }
  it('the wand floods joined pixels within the tolerance', () => {
    const m = floodMask(picture(), W, H, 0, 0, 32)
    expect([...m]).toEqual([1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0])
    expect(floodMask(picture(), W, H, 0, 0, 32, false)[2 * W + 3]).toBe(1)
    expect(floodMask(picture(), W, H, 0, 0, 5)[2 * W + 3]).toBe(0)
    expect(floodMask(picture(), W, H, 9, 9, 32).every((v) => v === 0)).toBe(true)
  })
  it('a mask has a box, rows of runs and edges', () => {
    const m = floodMask(picture(), W, H, 0, 0, 32)
    expect(maskBounds(m, W, H)).toEqual({ x: 0, y: 0, w: 2, h: 3 })
    expect(maskBounds(new Uint8Array(4), 2, 2)).toBe(null)
    expect(maskRuns(m, W, H)).toEqual([[0, 0, 2, 1], [0, 1, 2, 1], [0, 2, 2, 1]])
    const segs = maskOutline(m, W, H)
    expect(segs).toContainEqual([0, 0, 2, 0])
    expect(segs).toContainEqual([0, 3, 2, 3])
    expect(segs).toContainEqual([0, 0, 0, 3])
    expect(segs).toContainEqual([2, 0, 2, 3])
    expect(segs).toHaveLength(4)
  })
  it('a dragged box is whole pixels inside the picture; a lasso needs three points', () => {
    expect(dragRect(10.5, 20, -5, 2.2, 8, 30)).toEqual({ x: 0, y: 2, w: 8, h: 18 })
    expect(dragRect(3, 3, 3.2, 9, 10, 10)).toEqual({ x: 3, y: 3, w: 1, h: 6 })
    expect(dragRect(3, 3, 3, 9, 10, 10)).toBe(null)
    expect(cleanLasso([[0, 0], [5, 'x']], 10, 10)).toBe(null)
    const lasso = cleanLasso([[-2, 1], [12, 3], [5, 9]], 10, 10)
    expect(lasso).toEqual([[0, 1], [10, 3], [5, 9]])
    expect(lassoBounds(lasso)).toEqual({ x: 0, y: 1, w: 10, h: 8 })
    expect(selectionBox({ kind: 'lasso', points: lasso })).toEqual({ kind: 'lasso', x: 0, y: 1, w: 10, h: 8 })
    expect(selectionBox({ kind: 'mask', bounds: { x: 1, y: 1, w: 2, h: 2 }, mask: new Uint8Array(1) })).toEqual({ kind: 'mask', x: 1, y: 1, w: 2, h: 2 })
    expect(selectionBox(null)).toBe(null)
  })
})

describe('the document', () => {
  const doc = () => {
    const d = newDoc(800, 600)
    d.layers = [imageLayer('Background', { id: 'bg' }), textLayer('Hello', { id: 't1', x: 100, y: 50, size: 40 })]
    return d
  }
  it('starts clean and never carries pixels out', () => {
    expect(newDoc(99999, 0)).toMatchObject({ width: 8000, height: 1, layers: [] })
    const d = doc()
    d.layers[0].png = 'data:...'
    expect(docWithoutPixels(d).layers[0].png).toBeUndefined()
    expect(compositeOf('multiply')).toBe('multiply')
    expect(compositeOf('normal')).toBe('source-over')
    expect(compositeOf('dissolve')).toBe('source-over')
  })
  it('a saved document is read back with every field made known', () => {
    const back = readDoc({ width: 10, height: 20, adjust: { warmth: 300 }, layers: [{ id: 'a', kind: 'image', opacity: 150, blend: 'weird' }, { id: '../x' }, { id: 't', kind: 'text', text: { value: 'Hi', color: 'red' } }] })
    expect(back.layers.map((l) => l.id)).toEqual(['a', 't'])
    expect(back.layers[0]).toMatchObject({ opacity: 100, blend: 'normal', visible: true })
    expect(back.layers[1].text).toEqual({ value: 'Hi', size: 48, color: '#ffffff', font: 'system-ui' })
    expect(back.adjust.warmth).toBe(100)
  })
  it('crop shifts the layers, rotate swaps the sides, flip mirrors, resize scales', () => {
    const c = cropDoc(doc(), { x: 50, y: 20, w: 300, h: 200 })
    expect([c.width, c.height, c.layers[1].x, c.layers[1].y]).toEqual([300, 200, 50, 30])
    expect(turnBox(10, 20, 100, 50, 800, 600, 1)).toEqual({ x: 530, y: 10 })
    expect(turnBox(10, 20, 100, 50, 800, 600, 2)).toEqual({ x: 690, y: 530 })
    expect(turnBox(10, 20, 100, 50, 800, 600, 3)).toEqual({ x: 20, y: 690 })
    const r = rotateDoc(doc(), 1, { bg: { w: 800, h: 600 } })
    expect([r.width, r.height]).toEqual([600, 800])
    expect(r.layers[0]).toMatchObject({ x: 0, y: 0 })
    const f = flipDoc(doc(), 'h', { bg: { w: 800, h: 600 } })
    expect(f.layers[0].x).toBe(0)
    expect(f.layers[1].x).toBe(700)
    const s = resizeDoc(doc(), 400, 300)
    expect([s.width, s.height, s.layers[1].x, s.layers[1].text.size]).toEqual([400, 300, 50, 20])
  })
  it('layers move up and down and are put in where asked', () => {
    const d = doc()
    expect(moveLayer(d, 'bg', 1).layers.map((l) => l.id)).toEqual(['t1', 'bg'])
    expect(moveLayer(d, 'bg', -1)).toBe(d)
    expect(insertLayer(d, imageLayer('Mid', { id: 'm' }), 1).layers.map((l) => l.id)).toEqual(['bg', 'm', 't1'])
  })
  it('the companion\'s ops are checked again, like the core does', () => {
    const d = doc()
    expect(cleanOp({ op: 'adjust', values: { exposure: 250, evil: 1 } }, d)).toEqual({ op: 'adjust', values: { exposure: 100 }, mode: 'relative' })
    expect(cleanOp({ op: 'crop', x: -5, y: 10, w: 9000, h: 0 }, d)).toEqual({ op: 'crop', x: 0, y: 10, w: 800, h: 1 })
    expect(cleanOp({ op: 'rotate', turns: -1 }, d)).toEqual({ op: 'rotate', turns: 3 })
    expect(cleanOp({ op: 'rotate', turns: 4 }, d)).toBe(null)
    expect(cleanOp({ op: 'flip', axis: 'z' }, d)).toBe(null)
    expect(cleanOp({ op: 'text', text: ' Hi\u0007 ', x: 9000, color: 'red' }, d)).toEqual({ op: 'text', text: 'Hi', x: 800, y: 0, size: 48, color: '#ffffff' })
    expect(cleanOp({ op: 'layer', id: 'bg', opacity: 140, blend: 'screen' }, d)).toEqual({ op: 'layer', id: 'bg', opacity: 100, blend: 'screen' })
    expect(cleanOp({ op: 'layer', id: 'nope', opacity: 1 }, d)).toBe(null)
    expect(cleanOp({ op: 'select', all: true }, d)).toEqual({ op: 'select', all: true })
    expect(cleanOp({ op: 'replace_sky' }, d)).toBe(null)
    expect(cleanOp({ op: 'run', cmd: 'x' }, d)).toBe(null)
    const many = Array.from({ length: 14 }, () => ({ op: 'flip', axis: 'h' }))
    expect(cleanOps(many, d)).toHaveLength(10)
    expect(cleanOps([{ op: 'remove_background' }, { op: 'generate_layer', prompt: 'boat' }, { op: 'flip', axis: 'v' }], d).map((o) => o.op)).toEqual(['remove_background', 'flip'])
    expect(cleanOps('nope', d)).toEqual([])
  })
  it('a redraw asks the size nearest the picture and says its cost', () => {
    expect(redrawSize(1600, 900)).toBe('1536x1024')
    expect(redrawSize(900, 1600)).toBe('1024x1536')
    expect(redrawSize(500, 500)).toBe('1024x1024')
    expect(redrawLine('remove_background', 0.06)).toContain('about $0.06')
    expect(redrawLine('erase', null)).not.toContain('$')
  })
})

describe('the undo pills', () => {
  const start = () => {
    const doc = newDoc(100, 100)
    doc.layers = [imageLayer('Background', { id: 'bg' })]
    return { doc, pixels: { bg: 'canvas-0' } }
  }
  it('an adjust pill puts back only its own sliders', () => {
    let state = start()
    let history = []
    state = { ...state, doc: { ...state.doc, adjust: { ...state.doc.adjust, brightness: 20 } } }
    history = addPill(history, { id: 'a', kind: 'adjust', before: { brightness: 0 } })
    state = { ...state, doc: { ...state.doc, adjust: { ...state.doc.adjust, contrast: 15 } } }
    history = addPill(history, { id: 'b', kind: 'adjust', before: { contrast: 0 } })
    const undone = undoPill(state, history, 'a')
    expect(undone.state.doc.adjust).toMatchObject({ brightness: 0, contrast: 15 })
    expect(undone.history.map((p) => p.id)).toEqual(['b'])
  })
  it('a new layer pill deletes it and shows the hidden originals again', () => {
    let { doc, pixels } = start()
    doc = { ...doc, layers: [{ ...doc.layers[0], visible: false }, imageLayer('Redrawn', { id: 'r' })] }
    pixels = { ...pixels, r: 'canvas-r' }
    let history = addPill([], { id: 'n', kind: 'layer-new', layerId: 'r', shown: ['bg'] })
    history = addPill(history, { id: 'c', kind: 'layer-change', layerId: 'r', before: { opacity: 100 } })
    history = addPill(history, { id: 'x', kind: 'select', before: null })
    const u = undoPill({ doc, pixels }, history, 'n')
    expect(u.state.doc.layers.map((l) => [l.id, l.visible])).toEqual([['bg', true]])
    expect(u.state.pixels.r).toBeUndefined()
    expect(u.history.map((p) => p.id)).toEqual(['x'])
  })
  it('a layer change puts its old fields back, a deleted layer comes back, an order is restored', () => {
    let { doc, pixels } = start()
    doc = { ...doc, layers: [{ ...doc.layers[0], opacity: 40, name: 'Sky' }] }
    const u = undoPill({ doc, pixels }, [{ id: 'c', kind: 'layer-change', layerId: 'bg', before: { opacity: 100 } }], 'c')
    expect(u.state.doc.layers[0]).toMatchObject({ opacity: 100, name: 'Sky' })
    const gone = { doc: { ...doc, layers: [] }, pixels: {} }
    const back = undoPill(gone, [{ id: 'd', kind: 'layer-delete', layer: doc.layers[0], index: 0, pixel: 'canvas-0' }], 'd')
    expect(back.state.doc.layers.map((l) => l.id)).toEqual(['bg'])
    expect(back.state.pixels.bg).toBe('canvas-0')
    const two = { doc: { ...doc, layers: [imageLayer('B', { id: 'b' }), imageLayer('A', { id: 'a' })] }, pixels: {} }
    expect(undoPill(two, [{ id: 'o', kind: 'order', before: ['a', 'b'] }], 'o').state.doc.layers.map((l) => l.id)).toEqual(['a', 'b'])
  })
  it('a stroke pill brings the layer back as it was, with the later strokes on it', () => {
    const state = { ...start(), pixels: { bg: 'canvas-2' } }
    let history = addPill([], { id: 's1', kind: 'pixels', layerId: 'bg', before: 'canvas-0' })
    history = addPill(history, { id: 'a', kind: 'adjust', before: { warmth: 0 } })
    history = addPill(history, { id: 's2', kind: 'pixels', layerId: 'bg', before: 'canvas-1' })
    const u = undoPill(state, history, 's1')
    expect(u.state.pixels.bg).toBe('canvas-0')
    expect(u.history.map((p) => p.id)).toEqual(['a'])
  })
  it('a crop, rotate, flip or resize pill restores the snapshot and drops the pills after it', () => {
    const before = start()
    const after = { doc: cropDoc(before.doc, { x: 0, y: 0, w: 50, h: 50 }), pixels: before.pixels }
    let history = addPill([], { id: 'a', kind: 'adjust', before: {} })
    history = addPill(history, { id: 'k', kind: 'snapshot', before })
    history = addPill(history, { id: 'b', kind: 'adjust', before: {} })
    const u = undoPill(after, history, 'k')
    expect(u.state.doc).toBe(before.doc)
    expect(u.state.pixels).toBe(before.pixels)
    expect(u.history.map((p) => p.id)).toEqual(['a'])
  })
  it('Ctrl+Z undoes the last pill and keeps what Ctrl+Y brings back; the oldest pill goes past the limit', () => {
    const state = start()
    const history = addPill([], { id: 'x', kind: 'select', before: null })
    const u = undoLast({ ...state, doc: { ...state.doc, selection: { kind: 'rect' } } }, history)
    expect(u.state.doc.selection).toBe(null)
    expect(u.redo.history).toBe(history)
    expect(undoLast(state, []).redo).toBe(null)
    let many = []
    for (let i = 0; i < MAX_PILLS + 5; i++) many = addPill(many, { id: `p${i}`, kind: 'select' })
    expect(many).toHaveLength(MAX_PILLS)
    expect(many[0].id).toBe('p5')
    expect(undoPill(state, many, 'missing').history).toBe(many)
    expect(pillLabel('a'.repeat(40))).toHaveLength(28)
  })
})
