// The member's theme for the browser's windows that have no App.vue around them: its own window, a
// tab's window of its own and the drag's card. Castle's default colours sit on the root first
// (browser-root.css); the member's theme, applied on the root, and App.vue's own theme both win.
import { getSettings, userThemes } from '../api/castle.js'
import { DEFAULT_THEME_ID, applyTheme, builtInThemes, chooseTheme, fromUserFolders } from '../themes/loader.js'
import './browser-root.css'

/** A theme by its id on this window's root: the built-in themes and the ones dropped into Castle's
 * folder (a dropped-in theme's folders are read only when the chosen one is not built in: they carry
 * pictures). */
export async function wearTheme(id) {
  const wanted = id ?? DEFAULT_THEME_ID
  const builtIn = builtInThemes()
  const folders = builtIn.some((t) => t.id === wanted) ? [] : await userThemes().catch(() => [])
  applyTheme(chooseTheme([...builtIn, ...fromUserFolders(folders).themes], wanted).theme)
}

/** The member's chosen theme read from the settings and worn; returns the settings view (or null). */
export async function wearMemberTheme() {
  const view = await getSettings().catch(() => null)
  await wearTheme(view?.settings?.theme).catch(() => {})
  return view
}
