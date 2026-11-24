import { describe, expect, it } from 'vitest'
import { layoutForKey, layoutHint } from './windowkeys.js'

const key = (key, mods = {}) => ({ key, code: mods.code || '', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, repeat: false, defaultPrevented: false, ...mods })
const mac = { mac: true, layout: 'full' }

describe('window layout keys', () => {
  it('Mac: Ctrl+Cmd+F toggles full screen, Ctrl+Opt keys take full width and the halves', () => {
    expect(layoutForKey(key('f', { code: 'KeyF', ctrlKey: true, metaKey: true }), mac)).toBe('toggle-fullscreen')
    expect(layoutForKey(key('Enter', { ctrlKey: true, altKey: true }), mac)).toBe('full')
    expect(layoutForKey(key('ArrowLeft', { ctrlKey: true, altKey: true }), mac)).toBe('left')
    expect(layoutForKey(key('ArrowRight', { ctrlKey: true, altKey: true }), mac)).toBe('right')
  })
  it('leaves other keys alone: plain Opt+arrows move a tab, Cmd+F finds, F11 is not a Mac key', () => {
    expect(layoutForKey(key('ArrowLeft', { altKey: true }), mac)).toBeNull()
    expect(layoutForKey(key('f', { code: 'KeyF', metaKey: true }), mac)).toBeNull()
    expect(layoutForKey(key('F11'), mac)).toBeNull()
    expect(layoutForKey(key('ArrowLeft', { ctrlKey: true, altKey: true, shiftKey: true }), mac)).toBeNull()
  })
  it('Esc leaves full screen only when Castle is in it and nothing else took the key', () => {
    expect(layoutForKey(key('Escape'), { mac: true, layout: 'fullscreen' })).toBe('leave-fullscreen')
    expect(layoutForKey(key('Escape'), mac)).toBeNull()
    expect(layoutForKey(key('Escape', { defaultPrevented: true }), { mac: true, layout: 'fullscreen' })).toBeNull()
  })
  it('Windows: F11 toggles full screen, Win+arrows stay with Windows', () => {
    const win = { mac: false, layout: 'full' }
    expect(layoutForKey(key('F11'), win)).toBe('toggle-fullscreen')
    expect(layoutForKey(key('ArrowLeft', { ctrlKey: true, altKey: true }), win)).toBeNull()
    expect(layoutForKey(key('Escape'), { mac: false, layout: 'fullscreen' })).toBe('leave-fullscreen')
  })
  it('a held key does not toggle again and again', () => {
    expect(layoutForKey(key('f', { code: 'KeyF', ctrlKey: true, metaKey: true, repeat: true }), mac)).toBeNull()
  })
  it('shows the shortcut beside each menu item', () => {
    expect(layoutHint('fullscreen', true)).toBe('⌃⌘F')
    expect(layoutHint('right', true)).toBe('⌃⌥→')
    expect(layoutHint('free', true)).toBe('')
    expect(layoutHint('fullscreen', false)).toBe('F11')
    expect(layoutHint('left', false)).toBe('')
  })
})
