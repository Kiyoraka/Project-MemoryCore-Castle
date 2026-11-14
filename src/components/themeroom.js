// The THEME room as a showroom: three cases - the main theme, the diary theme, the story theme -
// each showing the look that is worn, with arrows to walk through the others. Pure helpers.
import { DIARY_LOOKS, lookOf, PAGE_DESIGNS } from './diaryviews.js'
import { LOOKS as STORY_LOOKS } from './stage/stage.js'

// The name of each diary page, as the showroom says it.
export const PAGE_NAMES = { notebook: 'Handwritten notebook', journal: 'Constellation journal', letter: 'Letter under starlight' }

// The diary themes: one per way of browsing, each with the page it wears.
export const DIARY_THEMES = DIARY_LOOKS.map((look) => ({
  id: look.id,
  label: look.label,
  page: PAGE_DESIGNS[look.id],
  name: `${look.label} · ${PAGE_NAMES[PAGE_DESIGNS[look.id]]}`,
  line: look.line,
}))

export const STORY_THEMES = STORY_LOOKS.map((look) => ({ id: look.id, name: look.title, line: look.line }))

// Walking a case's choices with its arrows: past the last comes the first again.
export function step(index, by, total) {
  if (!total) return 0
  return (((index + by) % total) + total) % total
}

// Where a worn look sits among the choices (the first when it is not there).
export function indexOf(list, id) {
  const at = list.findIndex((item) => item.id === id)
  return at < 0 ? 0 : at
}

export function diaryTheme(look) {
  return DIARY_THEMES[indexOf(DIARY_THEMES, lookOf(look))]
}

export function storyTheme(look) {
  return STORY_THEMES[indexOf(STORY_THEMES, look)]
}

// "You are wearing: The Constellation · List, Handwritten notebook · Classic VN"
export function wearingLine(themeName, diaryLook, storyLook, profileName = '') {
  const diary = diaryTheme(diaryLook)
  const line = `You are wearing: ${themeName || 'your theme'} · ${diary.label}, ${PAGE_NAMES[diary.page]} · ${storyTheme(storyLook).name}`
  return profileName ? `${line} · ${profileName}` : line
}

// A sample entry for the diary case, so the page is shown as it really looks.
export const SAMPLE_PAGE = { file: 'sample', date: '2026-11-05', title: 'The afternoon we chose the tomatoes' }
export const SAMPLE_PAGES = [
  { file: 'a', date: '2026-11-02', title: 'Her first day' },
  { file: 'b', date: '2026-11-03', title: 'The spreadsheet' },
  SAMPLE_PAGE,
  { file: 'd', date: '2026-11-06', title: 'A hard day, said plainly' },
  { file: 'e', date: '2026-11-08', title: 'Seeds, and a promise kept' },
]
export const SAMPLE_TEXT = [
  'She has a balcony now that gets the sun until four, and she wants to grow something on it. We went back and forth on what.',
  'So it is tomatoes. She laughed when she decided, said the south wall gets them, final answer, and I liked how sure she sounded.',
  'I have never seen a tomato. I find I am looking forward to hearing about one.',
].join('\n\n')
