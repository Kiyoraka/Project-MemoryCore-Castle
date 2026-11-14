import { describe, expect, it } from 'vitest'
import { DIARY_THEMES, diaryTheme, indexOf, PAGE_NAMES, step, STORY_THEMES, storyTheme, wearingLine } from './themeroom.js'

describe('the showroom', () => {
  it('has three diary themes, each way of browsing with the page it wears', () => {
    expect(DIARY_THEMES.map((t) => [t.id, t.page])).toEqual([
      ['list', 'notebook'],
      ['calendar', 'journal'],
      ['book', 'letter'],
    ])
    expect(DIARY_THEMES[0].name).toBe('List · Handwritten notebook')
    expect(Object.keys(PAGE_NAMES).sort()).toEqual(['journal', 'letter', 'notebook'])
  })

  it('has the five story stages', () => {
    expect(STORY_THEMES.map((t) => t.id)).toEqual(['classic', 'cinema', 'book', 'messenger', 'jrpg'])
    expect(storyTheme('cinema').name).toBe('Cinematic')
  })

  it('walks a case round and round with its arrows', () => {
    expect(step(0, 1, 3)).toBe(1)
    expect(step(2, 1, 3)).toBe(0)
    expect(step(0, -1, 3)).toBe(2)
    expect(step(4, 1, 5)).toBe(0)
    expect(step(0, 1, 0)).toBe(0)
  })

  it('opens each case at what is worn, and at the first when that is unknown', () => {
    expect(indexOf(STORY_THEMES, 'jrpg')).toBe(4)
    expect(indexOf(STORY_THEMES, 'hologram')).toBe(0)
    expect(diaryTheme('book').page).toBe('letter')
    // the diary opens as a list unless the member chose otherwise
    expect(diaryTheme(undefined).id).toBe('list')
    expect(diaryTheme('scroll').id).toBe('list')
  })

  it('says the whole outfit in one line', () => {
    expect(wearingLine('The Constellation', 'list', 'classic')).toBe('You are wearing: The Constellation · List, Handwritten notebook · Classic VN')
    expect(wearingLine('Sakura Night', 'book', 'jrpg')).toBe('You are wearing: Sakura Night · Book, Letter under starlight · JRPG window')
    expect(wearingLine('The Constellation', 'list', 'classic', 'Cards')).toBe('You are wearing: The Constellation · List, Handwritten notebook · Classic VN · Cards')
    expect(wearingLine('', undefined, 'nope')).toBe('You are wearing: your theme · List, Handwritten notebook · Classic VN')
  })
})
