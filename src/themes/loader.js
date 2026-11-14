// Themes: built-in folders beside this file, and folders the member drops into Castle's settings folder.
// A theme is data only - colours, a wallpaper, and where the panel and each widget sit.

const manifests = import.meta.glob('./*/theme.json', { eager: true, import: 'default' })
const pictures = import.meta.glob('./*/*.{png,jpg,jpeg,webp}', { eager: true, query: '?url', import: 'default' })

export const DEFAULT_THEME_ID = 'constellation'

// Castle's own plain look, used only when no theme can be read.
export const PLAIN = { id: 'plain', name: 'Plain', colors: {}, panel: {}, widgets: {}, portrait: 'panel' }

const PLACE_KEYS = ['left', 'top', 'right', 'bottom', 'width', 'height']
const PANEL_VARS = {
  left: '--panel-left',
  top: '--panel-top',
  right: '--panel-right',
  bottom: '--panel-bottom',
  width: '--panel-width',
  height: '--panel-height',
}

export function builtInThemes() {
  return Object.entries(manifests).map(([path, theme]) => {
    const folder = path.split('/')[1]
    // A built-in theme may borrow another built-in's picture, so the art ships once.
    const pictureFolder = theme.wallpaperFrom ?? folder
    const wallpaperUrl = theme.wallpaper ? pictures[`./${pictureFolder}/${theme.wallpaper}`] ?? null : null
    // the picture HOME shows under the wallpaper while the week is not yet used (usagereveal.js)
    const usageReveal = theme.usageReveal?.low ? { ...theme.usageReveal, lowUrl: pictures[`./${pictureFolder}/${theme.usageReveal.low}`] ?? null } : theme.usageReveal
    return { ...theme, id: theme.id ?? folder, source: 'built-in', wallpaperUrl, usageReveal }
  })
}

// Turns the core's list of dropped-in folders into themes and the reasons some could not be read.
export function fromUserFolders(entries) {
  const themes = []
  const broken = []
  for (const entry of entries) {
    if (entry.error || !entry.theme) {
      broken.push({ folder: entry.folder, error: entry.error ?? 'The theme could not be read.' })
    } else {
      const usageReveal = entry.theme.usageReveal?.low ? { ...entry.theme.usageReveal, lowUrl: entry.lowUrl ?? null } : entry.theme.usageReveal
      themes.push({ ...entry.theme, id: entry.theme.id ?? entry.folder, source: 'yours', wallpaperUrl: entry.wallpaperUrl, usageReveal })
    }
  }
  return { themes, broken }
}

// A plain reason when a theme cannot be used, else null.
export function problemWith(theme) {
  if (!theme || typeof theme !== 'object') return 'The theme is empty.'
  if (!theme.id || !theme.name) return 'A theme needs an id and a name.'
  for (const key of Object.keys(theme.colors ?? {})) {
    if (!key.startsWith('--')) return `Colour names start with --, and "${key}" does not.`
  }
  if (theme.wallpaper && !theme.wallpaperUrl) return `The wallpaper ${theme.wallpaper} was not found.`
  if (theme.usageReveal?.low && !theme.usageReveal.lowUrl) return `The picture ${theme.usageReveal.low} was not found.`
  return null
}

// Picks the theme by id. A missing or broken one falls back to the default, then to plain, with a reason.
export function chooseTheme(all, id) {
  const wanted = all.find((t) => t.id === id)
  if (wanted && !problemWith(wanted)) return { theme: wanted, notice: '' }
  const reason = wanted ? problemWith(wanted) : id ? `The theme "${id}" was not found.` : ''
  const fallback = all.find((t) => t.id === DEFAULT_THEME_ID && !problemWith(t))
  if (fallback) return { theme: fallback, notice: reason ? `${reason} Showing ${fallback.name}.` : '' }
  return { theme: PLAIN, notice: reason || 'No theme could be read, so the plain look is showing.' }
}

// Only position keys pass, so a theme can place a widget but not restyle the page.
export function placeStyle(place = {}) {
  const style = {}
  for (const key of PLACE_KEYS) if (place[key] != null) style[key] = String(place[key])
  return style
}

let applied = []

export function applyTheme(theme) {
  const root = document.documentElement.style
  applied.forEach((name) => root.removeProperty(name))
  applied = []
  const set = (name, value) => {
    root.setProperty(name, String(value))
    applied.push(name)
  }
  for (const [name, value] of Object.entries(theme.colors ?? {})) set(name, value)
  for (const [key, name] of Object.entries(PANEL_VARS)) {
    if (theme.panel?.[key] != null) set(name, theme.panel[key])
  }
  set('--wallpaper', theme.wallpaperUrl ? `url("${theme.wallpaperUrl}")` : 'none')
  set('--wallpaper-position', theme.wallpaperPosition ?? 'center')
}
