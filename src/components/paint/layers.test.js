import { describe, expect, it } from 'vitest'
import { applyOps, canDelete, companionLayerName, companionOf, contextOf, defaultName, deleteLayer, insertAbove, moveLayer, nextLayerName, renameLayer, replyLine } from './layers.js'

const base = () => [
  { id: 'a', name: 'Background', owner: 'member', visible: true },
  { id: 'b', name: 'Alice - moon', owner: 'companion', visible: true, svg: '<svg>m</svg>' },
  { id: 'c', name: 'Layer 1', owner: 'member', visible: true },
]
const keep = (svg) => (svg && !svg.includes('bad') ? `clean:${svg}` : null)

describe("PAINT's layers", () => {
  it("never deletes the member's last own layer", () => {
    const layers = base()
    expect(canDelete(layers, 'b')).toBe(true)
    expect(canDelete(layers, 'c')).toBe(true)
    const one = deleteLayer(layers, 'c')
    expect(canDelete(one, 'a')).toBe(false)
    expect(deleteLayer(one, 'a')).toBe(one)
    expect(canDelete(one, 'zz')).toBe(false)
  })

  it('moves a layer up and down within the stack', () => {
    expect(moveLayer(base(), 'a', 1).map((l) => l.id)).toEqual(['b', 'a', 'c'])
    expect(moveLayer(base(), 'c', 1).map((l) => l.id)).toEqual(['a', 'b', 'c'])
    expect(moveLayer(base(), 'a', -1).map((l) => l.id)).toEqual(['a', 'b', 'c'])
  })

  it("renames: the member's freely, the companion's keeping its name", () => {
    expect(renameLayer(base(), 'c', '  Sky  ', 'Alice')[2].name).toBe('Sky')
    expect(renameLayer(base(), 'b', 'big moon', 'Alice')[1].name).toBe('Alice - big moon')
    expect(renameLayer(base(), 'c', '   ', 'Alice')[2].name).toBe('Layer 1')
  })

  it('names and places a new layer', () => {
    expect(nextLayerName(base())).toBe('Layer 2')
    const layer = { id: 'd', name: 'Layer 2', owner: 'member' }
    expect(insertAbove(base(), layer, 'a').map((l) => l.id)).toEqual(['a', 'd', 'b', 'c'])
    expect(insertAbove(base(), layer, 'none').map((l) => l.id)).toEqual(['a', 'b', 'c', 'd'])
  })

  it("makes the companion's name the way paint.rs does", () => {
    expect(companionOf('  Alice  ')).toBe('Alice')
    expect(companionOf('')).toBe('Companion')
    expect(companionOf('<b>/')).toBe('b')
    expect(companionOf('x'.repeat(60))).toHaveLength(40)
  })

  it('names a companion layer as its own', () => {
    expect(companionLayerName('Alice', 'moon')).toBe('Alice - moon')
    expect(companionLayerName('Alice', 'Alice - moon')).toBe('Alice - moon')
    expect(companionLayerName('Alice', '')).toBe('')
  })

  it("applies the companion's ops to its own layers only", () => {
    const layers = [...base(), { id: 'e', name: 'Alice - sea', owner: 'member', visible: true }]
    const { layers: next, changed, dropped } = applyOps(
      layers,
      [
        { op: 'replace', layer: 'moon', svg: '<big/>' },
        { op: 'add', layer: 'Alice - star', svg: '<star/>' },
        { op: 'add', layer: 'cloud', svg: 'bad' },
        { op: 'remove', layer: 'sea' },
        { op: 'paint', layer: 'x', svg: '<x/>' },
      ],
      'Alice',
      keep,
    )
    expect(next.find((l) => l.id === 'b').svg).toBe('clean:<big/>')
    expect(next.at(-1)).toMatchObject({ name: 'Alice - star', owner: 'companion', svg: 'clean:<star/>' })
    expect(next.some((l) => l.id === 'e')).toBe(true) // the member's "Alice - sea" is never removed
    expect(changed).toHaveLength(2)
    expect(changed[0]).toBe('b')
    expect(dropped).toEqual(['Alice - cloud'])
    const removed = applyOps(next, [{ op: 'remove', layer: 'Alice - moon' }], 'Alice', keep)
    expect(removed.layers.some((l) => l.id === 'b')).toBe(false)
  })

  it('takes at most twelve ops', () => {
    const ops = Array.from({ length: 20 }, (_, i) => ({ op: 'add', layer: `dot ${i}`, svg: '<c/>' }))
    expect(applyOps([], ops, 'Alice', keep).changed).toHaveLength(12)
  })

  it("tells the hand every layer, and the drawings of the companion's only", () => {
    const ctx = contextOf([{ id: 'a', name: 'Background', owner: 'member', visible: true, svg: 'x' }, ...base().slice(1)])
    expect(ctx[0]).toEqual({ name: 'Background', owner: 'member', visible: true, svg: '' })
    expect(ctx[1].svg).toBe('<svg>m</svg>')
  })

  it('says when a drawing was left out', () => {
    expect(replyLine('A moon!', [])).toBe('A moon!')
    expect(replyLine('', [])).toBe('Done.')
    expect(replyLine('A moon!', ['Alice - star'])).toBe('A moon! I could not keep the drawing for "Alice - star", so I left it out.')
    const said = 'Here. I could not keep the drawing for "Alice - star", so I left it out.'
    expect(replyLine(said, ['Alice - star'])).toBe(said)
  })

  it('names a painting from the date', () => {
    expect(defaultName(new Date(2026, 10, 26, 9, 5))).toBe('Painting 2026-11-26 0905')
  })
})
