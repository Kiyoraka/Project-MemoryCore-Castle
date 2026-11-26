import { describe, expect, it } from 'vitest'
import { newDoc } from './engine.js'
import {
  StepHistory,
  addAdjustmentLayer,
  addFullLayer,
  affineFor,
  applyAffine,
  canvasOffset,
  defaultAdjustment,
  duplicateLayer2,
  fillPixels,
  fromTransformNumbers,
  gammaToPos,
  historyIcon,
  insertCurvePoint,
  invertAffine,
  keepRatio,
  localMask,
  moveCurvePoint,
  moveLayerTo,
  normaliseAdjustment,
  normaliseDoc2,
  placeMask,
  posToGamma,
  removeCurvePoint,
  rotateAround,
  scaleFromCorner,
  setAdjustment,
  setMask,
  shiftMask,
  startTransform,
  stripForSave,
  transformCorners,
  transformNumbers,
  ADJUSTMENT_NAMES,
  ADJUSTMENT_SPECS,
  ADJUSTMENT_TYPES,
  BLEND_MODES_3,
  BRUSH_PRESETS,
  addArtboard,
  addGuide,
  addPath,
  blendIds,
  blendLabel,
  blockRange,
  cleanActions,
  cleanSpecParams,
  defaultStyle,
  deleteLayer3,
  depthOf,
  descendantIds,
  duplicateLayer3,
  exportName,
  exportPlan,
  fixGroupOrder,
  groupLayers,
  hasStyles,
  layerRows,
  lineStart,
  moveGuide,
  normaliseDoc3,
  normaliseStyle,
  placeLayer,
  presetDab,
  pressured,
  removeArtboard,
  removeGuide,
  removePath,
  renamePath,
  rulerTicks,
  setGroupFolded,
  setLookOptions,
  specFields,
  setPath,
  getPath,
  setStyles,
  snapBox,
  snapLines,
  snapValue,
  styleList,
  toggleClip,
  toggleStyle,
  ungroupLayers,
  uniqueActionName,
  viewRect,
  wrapText,
} from './viewHelpers.js'

const near = (a, b, e = 1e-6) => Math.abs(a - b) < e

describe('the document model v2', () => {
  it('keeps masks and adjustment layers when a doc is read back', () => {
    const doc = normaliseDoc2({
      width: 4,
      height: 2,
      layers: [
        { id: 'a', name: 'Photo', png: 'data:image/png;base64,AA', mask: 'data:image/png;base64,BB' },
        { id: 'b', name: 'Curves 1', kind: 'adjustment', adjustment: { type: 'curves', params: { rgb: [[255, 255], [0, 10], [0, 0]] } } },
        { id: 'c', name: 'Bad', mask: 'nope' },
      ],
    })
    expect(doc.layers[0].mask).toBe('data:image/png;base64,BB')
    expect(doc.layers[1].kind).toBe('adjustment')
    expect(doc.layers[1].png).toBeUndefined()
    expect(doc.layers[1].adjustment.params.rgb).toEqual([[0, 10], [255, 255]])
    expect(doc.layers[2].mask).toBeNull()
  })

  it('cleans every adjustment type', () => {
    expect(normaliseAdjustment({ type: 'levels', params: { black: 300, white: 0, gamma: 50 } }).params).toEqual({ black: 253, white: 255, gamma: 9.99 })
    const hsl = normaliseAdjustment({ type: 'hsl', params: { ranges: { reds: { hue: 400, sat: -200, light: 5 } } } })
    expect(hsl.params.ranges.reds).toEqual({ hue: 180, sat: -100, light: 5 })
    expect(hsl.params.ranges.all).toEqual({ hue: 0, sat: 0, light: 0 })
    expect(normaliseAdjustment({ type: 'weird' }).type).toBe('adjust')
    expect(defaultAdjustment('curves').params.rgb).toEqual([[0, 0], [255, 255]])
  })

  it('adds adjustment layers above the chosen one with numbered names', () => {
    let doc = addFullLayer(newDoc(4, 4), 'data:image/png;base64,AA', 'Photo').doc
    doc = addFullLayer(doc, 'data:image/png;base64,AA', 'Top').doc
    const first = addAdjustmentLayer(doc, 'curves', doc.layers[0].id)
    expect(first.doc.layers[1].id).toBe(first.id)
    expect(first.doc.layers[1].name).toBe('Curves 1')
    const second = addAdjustmentLayer(first.doc, 'curves')
    expect(second.doc.layers.at(-1).name).toBe('Curves 2')
    const set = setAdjustment(second.doc, second.id, { rgb: [[0, 20], [255, 255]] })
    expect(set.layers.at(-1).adjustment.params.rgb[0]).toEqual([0, 20])
  })

  it('moves, duplicates, masks and strips layers', () => {
    let doc = newDoc(2, 2)
    for (const n of ['a', 'b', 'c']) doc = addFullLayer(doc, 'data:image/png;base64,AA', n).doc
    const top = doc.layers[2].id
    expect(moveLayerTo(doc, top, 0).layers[0].id).toBe(top)
    const masked = setMask(doc, top, 'data:image/png;base64,MM')
    const dup = duplicateLayer2(masked, top)
    expect(dup.doc.layers[3].mask).toBe('data:image/png;base64,MM')
    expect(dup.doc.layers[3].name).toBe('c copy')
    const saved = stripForSave({ ...dup.doc, selection: { kind: 'rect' } })
    expect(saved.selection).toBeNull()
    expect(saved.layers.every((l) => !('png' in l))).toBe(true)
    expect(saved.layers[3].mask).toBe('data:image/png;base64,MM')
  })
})

describe('the step history', () => {
  it('labels steps, marks hers and jumps', () => {
    const h = new StepHistory(3)
    h.reset('s0', { label: 'Open' })
    h.push('s1', { label: 'Brush stroke' })
    h.push('s2', { label: 'Curves', hers: true })
    expect(h.entries().map((e) => e.icon)).toEqual(['open', 'brush', 'curves'])
    expect(h.entries()[2].hers).toBe(true)
    expect(h.jump(0)).toBe('s0')
    expect(h.canRedo).toBe(true)
    h.push('s3', { label: 'Erased' })
    expect(h.steps.length).toBe(2)
    h.push('s4')
    h.push('s5')
    expect(h.steps.length).toBe(3)
    expect(h.jump(9)).toBeNull()
    expect(historyIcon('Masked sky')).toBe('mask')
  })
})

describe('masks', () => {
  it('reads the part under a layer, shifts and places', () => {
    const W = 3
    const H = 2
    const mask = Uint8Array.from([1, 2, 3, 4, 5, 6])
    expect([...localMask(mask, W, H, 1, 0, 2, 3)]).toEqual([2, 3, 5, 6, 0, 0])
    expect([...shiftMask(mask, W, H, 1, 0)]).toEqual([0, 1, 2, 0, 4, 5])
    expect([...placeMask(mask, W, H, 4, 2, 1, 0, 9)]).toEqual([9, 1, 2, 3, 9, 4, 5, 6])
    expect(canvasOffset(10, 10, 20, 14)).toEqual({ dx: 5, dy: 2 })
    expect(canvasOffset(10, 10, 20, 14, 'bottom-right')).toEqual({ dx: 10, dy: 4 })
  })

  it('fills inside the mask only', () => {
    const p = { data: new Uint8ClampedArray(8), width: 2, height: 1 }
    const out = fillPixels(p, [255, 0, 0, 255], Uint8Array.from([255, 0]))
    expect([...out.data]).toEqual([255, 0, 0, 255, 0, 0, 0, 0])
  })
})

describe('curves and levels', () => {
  it('adds, moves and removes points in order', () => {
    const { points, index } = insertCurvePoint([[0, 0], [255, 255]], 128, 160)
    expect(points).toEqual([[0, 0], [128, 160], [255, 255]])
    expect(index).toBe(1)
    expect(moveCurvePoint(points, 1, 300, -5)).toEqual([[0, 0], [254, 0], [255, 255]])
    expect(removeCurvePoint(points, 1)).toEqual([[0, 0], [255, 255]])
    expect(removeCurvePoint([[0, 0], [255, 255]], 0)).toHaveLength(2)
  })

  it('puts the grey triangle half way at gamma 1 and back', () => {
    expect(gammaToPos(0, 255, 1)).toBeCloseTo(127.5)
    expect(posToGamma(0, 255, gammaToPos(0, 255, 2))).toBeCloseTo(2)
    expect(gammaToPos(0, 255, 2)).toBeLessThan(127.5)
  })
})

describe('free transform', () => {
  it('starts where the layer is and maps its corners', () => {
    const t = startTransform(10, 20, 100, 50)
    const c = transformCorners(t)
    expect(near(c[0].x, 10) && near(c[0].y, 20)).toBe(true)
    expect(near(c[2].x, 110) && near(c[2].y, 70)).toBe(true)
    const m = affineFor({ ...t, rotate: 0.7, sx: 2 })
    const back = applyAffine(invertAffine(m), ...Object.values(applyAffine(m, 3, 4)))
    expect(near(back.x, 3) && near(back.y, 4)).toBe(true)
  })

  it('scales from a corner with the opposite one still', () => {
    const t = startTransform(0, 0, 100, 50)
    const s = scaleFromCorner(t, 2, { x: 200, y: 100 })
    expect(s.sx).toBeCloseTo(2)
    expect(s.sy).toBeCloseTo(2)
    const c = transformCorners(s)
    expect(near(c[0].x, 0) && near(c[0].y, 0)).toBe(true)
    const u = scaleFromCorner(t, 0, { x: 50, y: 40 }, true)
    expect(u.sx).toBeCloseTo(u.sy)
    const cu = transformCorners(u)
    expect(near(cu[2].x, 100) && near(cu[2].y, 50)).toBe(true)
  })

  it('rotates around the centre with snapping, and round-trips its numbers', () => {
    const t = startTransform(0, 0, 100, 100)
    const r = rotateAround(t, 0, { x: 100, y: 50 }, { x: 50, y: 100 })
    expect(r.rotate).toBeCloseTo(Math.PI / 2)
    const snapped = rotateAround(t, 0, { x: 100, y: 50 }, { x: 100, y: 58 }, 15)
    expect(snapped.rotate).toBeCloseTo(Math.PI / 12)
    const n = transformNumbers({ ...t, sx: 1.5, rotate: Math.PI / 4 })
    expect(n.w).toBe(150)
    expect(n.angle).toBe(45)
    const back = fromTransformNumbers(t, n)
    expect(back.sx).toBeCloseTo(1.5)
    expect(back.rotate).toBeCloseTo(Math.PI / 4)
  })

  it('keeps a ratio', () => {
    expect(keepRatio(400, 200, 100, 0, 'w')).toEqual({ w: 100, h: 50 })
    expect(keepRatio(400, 200, 0, 100, 'h')).toEqual({ w: 200, h: 100 })
  })
})

describe('the transform box hit test', () => {
  it('tells corners, inside and outside apart', async () => {
    const { transformHit } = await import('./viewHelpers.js')
    const t = startTransform(0, 0, 100, 50)
    expect(transformHit(t, { x: 101, y: 51 }, 4)).toEqual({ kind: 'scale', corner: 2 })
    expect(transformHit(t, { x: 50, y: 25 }, 4)).toEqual({ kind: 'move' })
    expect(transformHit(t, { x: 150, y: 25 }, 4)).toEqual({ kind: 'rotate' })
  })
})

describe('PHOTO v3: the document model', () => {
  it('reads groups, clipping, styles, text boxes, guides, artboards, the grid and paths', () => {
    const doc = normaliseDoc3({
      width: 100,
      height: 50,
      layers: [
        { id: 'a', name: 'Photo', png: 'data:image/png;base64,AA' },
        { id: 't', name: 'BALI', kind: 'text', parent: 'g', text: { value: 'BALI' }, textBox: { w: 80, align: 'center', lineHeight: 9 }, styles: { dropShadow: { on: true, opacity: 60, size: 999 }, nope: {} } },
        { id: 'h', name: 'Hue', kind: 'adjustment', parent: 'g', clipped: true, adjustment: { type: 'vibrance', params: { vibrance: 500 } } },
        { id: 'g', name: 'Title', kind: 'group', collapsed: true, blend: 'softLight' },
        { id: 'p', name: 'Pass', kind: 'group', blend: 'passThrough' },
        { id: 'x', name: 'Orphan', parent: 'missing', png: 'data:image/png;base64,AA' },
      ],
      guides: [{ axis: 'x', at: 10.4 }, { axis: 'z', at: 3 }, { axis: 'y', at: 99999 }, { axis: 'h', at: 7 }],
      artboards: [{ name: 'Icon', x: 0, y: 0, w: 64, h: 64 }, { w: 0 }],
      grid: { size: 1, show: true },
      paths: [{ name: 'P', points: [[0, 0], { x: 5, y: 5 }], closed: 1 }, { points: [[1, 1]] }],
    })
    const g = doc.layers.find((l) => l.id === 'g')
    expect(g.kind).toBe('group')
    expect(g.png).toBeUndefined()
    expect(g.collapsed).toBe(true)
    expect(g.blend).toBe('softLight')
    const t = doc.layers.find((l) => l.id === 't')
    expect(t.parent).toBe('g')
    expect(t.textBox).toEqual({ w: 80, h: 0, align: 'center', lineHeight: 5, tracking: 0 })
    expect(t.styles.dropShadow.opacity).toBe(0.6)
    expect(t.styles.dropShadow.size).toBe(250)
    expect(Object.keys(t.styles)).toEqual(['dropShadow'])
    const h = doc.layers.find((l) => l.id === 'h')
    expect(h.clipped).toBe(true)
    expect(h.adjustment.params.vibrance).toBe(100)
    expect(h.adjustment.params.saturation).toBe(0)
    expect(doc.layers.find((l) => l.id === 'x').parent).toBeUndefined()
    // the group sits right above its members
    expect(doc.layers.map((l) => l.id)).toEqual(['a', 't', 'h', 'g', 'p', 'x'])
    expect(doc.layers.find((l) => l.id === 'p').blend).toBe('passThrough')
    expect(doc.guides).toEqual([{ axis: 'v', at: 10 }, { axis: 'h', at: 100 }, { axis: 'h', at: 7 }])
    expect(doc.artboards).toHaveLength(1)
    expect(doc.grid).toEqual({ size: 2, show: true, snap: true })
    expect(doc.paths).toHaveLength(1)
    expect(doc.paths[0].points[1]).toEqual([5, 5])
    expect(doc.paths[0].closed).toBe(true)
  })

  it('still opens a v2 document as it was', () => {
    const doc = normaliseDoc3({ width: 2, height: 2, layers: [{ id: 'a', name: 'A', png: 'data:image/png;base64,AA', mask: 'data:image/png;base64,MM' }] })
    expect(doc.layers[0]).toMatchObject({ id: 'a', kind: 'image', mask: 'data:image/png;base64,MM' })
    expect(doc.guides).toEqual([])
    expect(doc.artboards).toEqual([])
    expect(doc.paths).toEqual([])
  })

  it('cleans the table adjustments and knows their names', () => {
    expect(defaultAdjustment('posterize').params).toEqual({ levels: 4 })
    expect(cleanSpecParams('colorBalance', { tone: 'weird', shadows: [400, 'x'] })).toMatchObject({ tone: 'midtones', shadows: [100, 0, 0], midtones: [0, 0, 0], preserveLuminosity: true })
    expect(cleanSpecParams('channelMixer', {})).toMatchObject({ red: [100, 0, 0, 0], green: [0, 100, 0, 0], blue: [0, 0, 100, 0] })
    expect(cleanSpecParams('selectiveColor', { blues: { c: 20 } }).blues).toEqual({ c: 20, m: 0, y: 0, k: 0 })
    expect(cleanSpecParams('blackWhite', { tintOn: true }).tint).toBe('#e1d3b3')
    expect(cleanSpecParams('gradientMap', { to: '#ff0000' }).stops[1].color).toBe('#ff0000')
    expect(specFields('colorBalance', { tone: 'shadows' })[1].key).toBe('shadows.0')
    expect(specFields('channelMixer', { output: 'green' })[2]).toMatchObject({ key: 'green.1', value: 100 })
    expect(setPath({ a: [1, 2] }, 'a.1', 5)).toEqual({ a: [1, 5] })
    expect(getPath({ a: { b: 3 } }, 'a.b')).toBe(3)
    expect(cleanSpecParams('photoFilter', { color: 'red' }).color).toBe('#ec8a00')
    expect(ADJUSTMENT_NAMES.blackWhite).toBe('Black & White')
    expect(ADJUSTMENT_TYPES.length).toBeGreaterThanOrEqual(18)
    const before = [...ADJUSTMENT_SPECS.colorLookup.fields[0].options]
    expect(setLookOptions([{ id: 'kodak' }, 'fuji'])).toEqual(['kodak', 'fuji'])
    setLookOptions(before)
  })
})

describe('PHOTO v3: the layer tree', () => {
  const base = () => {
    let doc = newDoc(10, 10)
    for (const n of ['a', 'b', 'c']) doc = addFullLayer(doc, 'data:image/png;base64,AA', n).doc
    return doc
  }

  it('groups, folds, places, clips and ungroups', () => {
    let doc = base()
    const [a, b, c] = doc.layers.map((l) => l.id)
    const grouped = groupLayers(doc, b)
    doc = grouped.doc
    const g = grouped.id
    expect(doc.layers.map((l) => l.id)).toEqual([a, b, g, c])
    expect(doc.layers[1].parent).toBe(g)
    // c dropped into the group: on top inside it
    doc = placeLayer(doc, c, g, 'into')
    expect(doc.layers.map((l) => l.id)).toEqual([a, b, c, g])
    expect(doc.layers[2].parent).toBe(g)
    expect(layerRows(doc.layers).map((r) => [r.layer.id, r.depth])).toEqual([[g, 0], [c, 1], [b, 1], [a, 0]])
    expect(layerRows(setGroupFolded(doc, g, true).layers).map((r) => r.layer.id)).toEqual([g, a])
    // the whole group moved below a
    const moved = placeLayer(doc, g, a, 'below')
    expect(moved.layers.map((l) => l.id)).toEqual([b, c, g, a])
    // a group never goes into itself
    expect(placeLayer(doc, g, c, 'into')).toBe(doc)
    // b out of the group, above a at the top level
    const out = placeLayer(doc, b, a, 'above')
    expect(out.layers.find((l) => l.id === b).parent).toBeUndefined()
    expect(toggleClip(doc, c).layers.find((l) => l.id === c).clipped).toBe(true)
    expect(toggleClip(toggleClip(doc, c), c).layers.find((l) => l.id === c).clipped).toBeUndefined()
    const un = ungroupLayers(doc, g)
    expect(un.layers.map((l) => l.id)).toEqual([a, b, c])
    expect(un.layers.every((l) => !l.parent)).toBe(true)
    expect(blockRange(doc.layers, g)).toEqual([1, 3])
    expect([...descendantIds(doc.layers, g)].sort()).toEqual([b, c].sort())
    expect(depthOf(doc.layers, c)).toBe(1)
  })

  it('deletes and duplicates a group with all inside it', () => {
    let doc = base()
    const [a, b] = doc.layers.map((l) => l.id)
    const grouped = groupLayers(doc, b)
    doc = grouped.doc
    const dup = duplicateLayer3(doc, grouped.id)
    expect(dup.doc.layers).toHaveLength(6)
    const copy = dup.doc.layers.find((l) => l.id === dup.id)
    expect(copy.name).toBe('Group 1 copy')
    expect(dup.doc.layers.filter((l) => l.parent === dup.id)).toHaveLength(1)
    expect(deleteLayer3(doc, grouped.id).layers.map((l) => l.id)).toEqual([a, doc.layers.at(-1).id])
    const lone = { ...newDoc(2, 2), layers: [{ id: 'g', kind: 'group', name: 'G' }, { id: 'p', kind: 'image', name: 'P' }] }
    expect(deleteLayer3(lone, 'p')).toBe(lone)
  })

  it('mends a broken group order', () => {
    const fixed = fixGroupOrder([{ id: 'm', parent: 'g' }, { id: 'a' }, { id: 'g', kind: 'group' }, { id: 'n', parent: 'g' }])
    expect(fixed.map((l) => l.id)).toEqual(['a', 'm', 'n', 'g'])
  })

  it('sets and toggles styles', () => {
    let doc = base()
    const id = doc.layers[0].id
    doc = setStyles(doc, id, { stroke: { ...defaultStyle('stroke'), on: true } })
    expect(hasStyles(doc.layers[0])).toBe(true)
    expect(styleList(doc.layers[0])).toEqual([{ key: 'stroke', name: 'Stroke', on: true }])
    doc = toggleStyle(doc, id, 'stroke')
    expect(hasStyles(doc.layers[0])).toBe(false)
    expect(setStyles(doc, id, null).layers[0].styles).toBeUndefined()
    expect(normaliseStyle('gradientOverlay', { stops: [{ at: 0, color: '#ff0000' }] }).stops).toHaveLength(2)
    expect(blendLabel('soft-light')).toBe('Soft Light')
    expect(blendLabel('colorDodge')).toBe('Color Dodge')
    expect(blendIds([{ id: 'normal' }, 'multiply'])).toEqual(['normal', 'multiply'])
    expect(BLEND_MODES_3).toHaveLength(27)
  })
})

describe('PHOTO v3: guides, artboards, paths, snapping', () => {
  it('adds, moves and removes guides; lays artboards in a row; keeps paths', () => {
    let doc = { ...newDoc(100, 100), guides: [] }
    doc = addGuide(doc, 'x', 10.6)
    doc = addGuide(doc, 'q', 1)
    expect(doc.guides).toEqual([{ axis: 'v', at: 11 }])
    expect(moveGuide(doc, 0, 40).guides[0].at).toBe(40)
    expect(removeGuide(doc, 0).guides).toEqual([])
    const one = addArtboard(doc, { name: 'Icon', w: 64, h: 64 })
    const two = addArtboard(one.doc, { name: 'Big', w: 128, h: 128 })
    expect(two.board).toMatchObject({ x: 144, y: 0, w: 128 })
    expect(removeArtboard(two.doc, one.board.id).artboards).toHaveLength(1)
    const p = addPath(doc, [[0, 0], [1.234, 2]], true)
    expect(p.doc.paths[0]).toMatchObject({ name: 'Path 1', closed: true, points: [[0, 0], [1.2, 2]] })
    expect(renamePath(p.doc, p.id, ' Edge ').paths[0].name).toBe('Edge')
    expect(removePath(p.doc, p.id).paths).toEqual([])
  })

  it('snaps to guides, edges and the grid', () => {
    const doc = { ...newDoc(100, 80), guides: [{ axis: 'x', at: 30 }], grid: { size: 25, show: true, snap: true } }
    const lines = snapLines(doc, {}, { x: [61], y: [] })
    expect(lines.xs).toContain(30)
    expect(lines.xs).toContain(50)
    expect(snapValue(33, lines.xs, 4)).toBe(30)
    expect(snapValue(40, lines.xs, 4)).toBe(40)
    // a box at x 0..10 moved by 18: its right edge (28) snaps to the guide at 30
    expect(snapBox({ x: 0, y: 0, w: 10, h: 10 }, 18, 0, lines, 4).dx).toBe(20)
  })
})

describe('PHOTO v3: rulers, brushes, text, navigator, export, actions', () => {
  it('spaces ruler ticks at a round step', () => {
    const { step, ticks } = rulerTicks(0.1, 20, 600)
    expect(step).toBe(500)
    const zero = ticks.findIndex((t) => t.label === '0')
    expect(ticks[zero].at).toBeCloseTo(20)
    expect(ticks[zero + 1].at).toBeCloseTo(70)
    expect(rulerTicks(4, 0, 100).step).toBe(20)
  })

  it('weighs dabs by pressure, builds an airbrush, keeps the strongest touch otherwise', () => {
    expect(pressured(20, 0.5, { size: true })).toEqual({ size: 10, strength: 1 })
    expect(pressured(20, 0.5, { size: false, opacity: true })).toEqual({ size: 20, strength: 0.5 })
    expect(pressured(20, 0).size).toBe(20)
    const cover = new Float32Array(9)
    presetDab(cover, 3, 3, 1.5, 1.5, 2, 1, null, { strength: 0.5 })
    expect(cover[4]).toBeCloseTo(0.5)
    presetDab(cover, 3, 3, 1.5, 1.5, 2, 1, null, { strength: 0.3 })
    expect(cover[4]).toBeCloseTo(0.5)
    presetDab(cover, 3, 3, 1.5, 1.5, 2, 1, null, { build: true, flow: 0.25 })
    expect(cover[4]).toBeCloseTo(0.75)
    const masked = new Float32Array(9)
    presetDab(masked, 3, 3, 1.5, 1.5, 2, 1, new Uint8Array(9))
    expect(masked[4]).toBe(0)
    expect(BRUSH_PRESETS.map((b) => b.id)).toEqual(['soft', 'hard', 'airbrush', 'texture'])
  })

  it('wraps paragraph text and aligns it', () => {
    const measure = (s) => s.length * 10
    expect(wrapText('one two three', 75, measure)).toEqual(['one two', 'three'])
    expect(wrapText('a\nb', 100, measure)).toEqual(['a', 'b'])
    expect(wrapText('abcdefgh', 30, measure)).toEqual(['abc', 'def', 'gh'])
    expect(wrapText('one two', 75, measure, 5)).toEqual(['one', 'two'])
    expect(lineStart('center', 100, 40)).toBe(30)
    expect(lineStart('right', 100, 40)).toBe(60)
  })

  it('finds the part of the picture the stage shows', () => {
    expect(viewRect({ stageW: 100, stageH: 100, scale: 2, x: -50, y: 0 }, 200, 200)).toEqual({ x: 25, y: 0, w: 50, h: 50 })
  })

  it('plans Export As files with safe names', () => {
    const doc = { ...newDoc(200, 100), artboards: [{ id: 'b1', name: 'App icon', x: 0, y: 0, w: 64, h: 64 }] }
    expect(exportName('my/icon?', '16', 'jpeg')).toBe('myicon-16.jpg')
    const plan = exportPlan(doc, { boards: ['b1'], sizes: [16, 32], format: 'png' })
    expect(plan.map((p) => p.name)).toEqual(['App-icon-16.png', 'App-icon-32.png'])
    expect(plan[0]).toMatchObject({ w: 16, h: 16 })
    const whole = exportPlan(doc, { title: 'Bali', format: 'webp' })
    expect(whole).toEqual([{ name: 'Bali.webp', rect: { x: 0, y: 0, w: 200, h: 100 }, w: 200, h: 100 }])
    expect(exportPlan(doc, { title: 'x', sizes: [50] })[0]).toMatchObject({ w: 50, h: 25 })
  })

  it('cleans actions and names new ones', () => {
    const list = cleanActions([{ name: ' Warm ', steps: [{ op: 'filter' }, { nope: 1 }] }, { name: '' }, null])
    expect(list).toEqual([{ name: 'Warm', steps: [{ op: 'filter' }] }])
    expect(uniqueActionName([{ name: 'Action' }])).toBe('Action 2')
  })
})
