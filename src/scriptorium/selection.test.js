import { describe, expect, it } from 'vitest'
import { caretBox, extendTo, isCollapsed, motionOf, onPage, readCaret, readClip, readPos, readRects, readSelection, selectionAt, toScreen } from './selection.js'

describe('reading the editing ground', () => {
  it('reads a pos, a caret, boxes and a copy', () => {
    expect(readPos('{"para":1,"offset":2}')).toEqual({ para: 1, offset: 2 })
    expect(readPos('null')).toBe(null)
    expect(readPos(null)).toBe(null)
    expect(readPos('0')).toBe(0)
    expect(readCaret('{"page":1,"x":10,"y":20.5,"height":12}')).toEqual({ page: 1, x: 10, y: 20.5, height: 12 })
    expect(readCaret({ page: 0, x: 1, y: 2, height: -3 }).height).toBe(0)
    expect(readCaret('null')).toBe(null)
    expect(readCaret({ page: -1, x: 1, y: 2, height: 3 })).toBe(null)
    expect(readCaret({ page: 0, x: 'a', y: 2, height: 3 })).toBe(null)
    expect(readRects('[{"page":0,"x":1,"y":2,"w":3,"h":4},{"page":0,"x":1,"y":2,"w":0,"h":4},{"x":1}]')).toEqual([{ page: 0, x: 1, y: 2, w: 3, h: 4 }])
    expect(readRects('{}')).toEqual([])
    expect(readClip('{"text":"hi","rich":{"runs":[]}}')).toEqual({ text: 'hi', rich: { runs: [] } })
    expect(readClip('oops')).toEqual({ text: '', rich: null })
    expect(readSelection('{"anchor":{"a":1}}')).toEqual({ anchor: { a: 1 }, focus: { a: 1 } })
    expect(readSelection('{"anchor":1,"focus":4}')).toEqual({ anchor: 1, focus: 4 })
    expect(readSelection('null')).toBe(null)
    expect(readSelection({ focus: 1 })).toBe(null)
  })
  it('grows and collapses a selection', () => {
    const a = { p: 1 }
    const b = { p: 5 }
    expect(selectionAt(a)).toEqual({ anchor: a, focus: a })
    expect(selectionAt(null)).toBe(null)
    expect(extendTo(selectionAt(a), b)).toEqual({ anchor: a, focus: b })
    expect(extendTo(null, b)).toEqual({ anchor: b, focus: b })
    expect(extendTo(selectionAt(a), null)).toEqual({ anchor: a, focus: a })
    expect(isCollapsed(selectionAt(a))).toBe(true)
    expect(isCollapsed({ anchor: { p: 1 }, focus: { p: 1 } })).toBe(true)
    expect(isCollapsed({ anchor: a, focus: b })).toBe(false)
    expect(isCollapsed(null)).toBe(true)
  })
  it('places boxes and the caret over the canvas', () => {
    const layout = { left: 100, top: 20, zoom: 2 }
    expect(toScreen({ x: 10, y: 5, w: 30, h: 12 }, layout)).toEqual({ left: 120, top: 30, width: 60, height: 24 })
    expect(caretBox({ page: 0, x: 10, y: 5, height: 12 }, layout)).toEqual({ left: 120, top: 30, height: 24 })
    expect(caretBox({ page: 0, x: 0, y: 0, height: 0 }, layout).height).toBe(1)
    expect(caretBox(null, layout)).toBe(null)
    expect(onPage([{ page: 0 }, { page: 1 }, { page: 0 }], 0)).toHaveLength(2)
    expect(onPage(null, 0)).toEqual([])
  })
  it('names the motion of a caret intent', () => {
    expect(motionOf('caret.left')).toBe('left')
    expect(motionOf('caret.word_right')).toBe('word_right')
    expect(motionOf('caret.doc_end')).toBe('doc_end')
    expect(motionOf('caret.sideways')).toBe(null)
    expect(motionOf('edit.delete')).toBe(null)
  })
})
