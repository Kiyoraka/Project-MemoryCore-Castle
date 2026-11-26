// Settings' sidebar map: every page sits in one of five fixed groups (You · Home · Connect · Data ·
// App), always in this order. A page's subtitle is one line saying what lives there; `{companion}`
// is replaced with the companion's name. See docs/settings-standard.md.

export const SETTINGS_GROUPS = [
  {
    id: 'you',
    label: 'You',
    pages: [
      { id: 'companion', label: 'Companion', subtitle: 'Who {companion} is, what it thinks with, and its daily life.' },
      { id: 'chat', label: 'Chat', subtitle: 'The CHAT room, and {companion} in it.' },
      { id: 'rooms', label: 'Rooms', subtitle: 'Which rooms show, how they look, and the Neural Link.' },
      { id: 'quick', label: 'Quick actions', subtitle: "Your own buttons beside Save diary, typed into {companion}'s session." },
    ],
  },
  {
    id: 'home',
    label: 'Home',
    pages: [
      { id: 'widgets', label: 'Widgets', subtitle: 'What HOME shows, and how big.' },
      { id: 'alerts', label: 'Alerts', subtitle: 'What Castle tells you about while you look elsewhere.' },
      { id: 'sentinel', label: 'Sentinel', subtitle: '{companion} keeping watch over this computer.' },
    ],
  },
  {
    id: 'connect',
    label: 'Connect',
    pages: [{ id: 'keys', label: 'Keys', subtitle: 'The keys for pictures, voice, songs and video.' },
      { id: 'comfy', label: 'ComfyUI', subtitle: 'Draw pictures with your own ComfyUI.' },
      { id: 'automations', label: 'Automations', subtitle: 'Programs your scheduled jobs may run.' }],
  },
  {
    id: 'data',
    label: 'Data',
    pages: [
      { id: 'database', label: 'Database', subtitle: "Where {companion}'s memory lives, and the helpers." },
      { id: 'backup', label: 'Backup', subtitle: "{companion}'s memory kept safe on GitHub." },
    ],
  },
  {
    id: 'app',
    label: 'App',
    pages: [
      { id: 'startup', label: 'Start-up', subtitle: 'How Castle starts, minimises, draws and shows the time.' },
      { id: 'about', label: 'About', subtitle: 'The version, the licence and the people who made Castle.' },
    ],
  },
]

export const SETTINGS_PAGES = SETTINGS_GROUPS.flatMap((group) => group.pages.map((page) => ({ ...page, group: group.id })))

export const FIRST_PAGE = SETTINGS_PAGES[0].id

export function pageOf(id) {
  return SETTINGS_PAGES.find((page) => page.id === id) ?? SETTINGS_PAGES[0]
}

export function subtitleOf(id, companion = 'your companion') {
  return pageOf(id).subtitle.replaceAll('{companion}', companion)
}

/**
 * A deep link into Settings: 'chat#pictures' is the Chat page, scrolled to the row or group with the
 * id 'pictures'. A plain page id ('keys') opens the page at its top; anything unknown opens the
 * first page.
 */
export function parseTarget(target) {
  if (typeof target !== 'string' || !target.trim()) return { page: FIRST_PAGE, anchor: '' }
  const [rawPage, ...rest] = target.trim().split('#')
  const known = SETTINGS_PAGES.some((page) => page.id === rawPage)
  return { page: known ? rawPage : FIRST_PAGE, anchor: known ? rest.join('#') : '' }
}

/** The page the sidebar moves to for a key (ArrowUp/ArrowDown wrap, Home/End), or null. */
export function stepPage(current, key) {
  const ids = SETTINGS_PAGES.map((page) => page.id)
  const index = Math.max(0, ids.indexOf(current))
  if (key === 'ArrowDown' || key === 'ArrowRight') return ids[(index + 1) % ids.length]
  if (key === 'ArrowUp' || key === 'ArrowLeft') return ids[(index + ids.length - 1) % ids.length]
  if (key === 'Home') return ids[0]
  if (key === 'End') return ids[ids.length - 1]
  return null
}
