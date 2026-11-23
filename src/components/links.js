// A link in a chat (HOME, a DEV tab's Chat, the CHAT room) opens on click, where Settings > Rooms >
// Browser says: the BROWSER window, the member's own browser, or a small menu to choose (LinkMenu).
// Only https pages and servers on this computer open; the core checks again (browser_tabs.rs).
import { linkOpen } from '../api/castle.js'

const LONGEST = 500
const LOCAL = /^http:\/\/(localhost|127\.0\.0\.1):\d{1,5}(\/|$)/
/** LinkMenu listens here for LINK_MENU events: detail { url, x, y }. */
export const LINK_MENU = 'castle-link-menu'
export const linkMenus = new EventTarget()

/** 'web' for an https page, 'local' for a server on this computer, or null: it does not open. */
export function linkWay(url) {
  if (typeof url !== 'string' || !url || url.length > LONGEST || /[\s"'\u0000-\u001f]/.test(url)) return null
  if (url.startsWith('https://') && url.length > 'https://'.length) return 'web'
  if (LOCAL.test(url)) return 'local'
  return null
}

/** The link menu at a point (a right-click, or a click when Settings say ask). */
export function showLinkMenu(url, at = {}) {
  if (!linkWay(url)) return false
  linkMenus.dispatchEvent(new CustomEvent(LINK_MENU, { detail: { url, x: at.x ?? 0, y: at.y ?? 0 } }))
  return true
}

/** Opens a link the way Settings say -> 'castle' | 'system' | 'ask', or false when it may not open. */
export async function openLink(url, at = {}) {
  if (!linkWay(url)) return false
  const way = await linkOpen(url)
  if (way === 'ask') showLinkMenu(url, at)
  return way
}

/** A link's click and right-click handlers for a template: <a v-bind="linkEvents(url)">. */
export function linkEvents(url) {
  return {
    onClick: (event) => {
      event.preventDefault()
      openLink(url, { x: event.clientX, y: event.clientY }).catch(() => {})
    },
    onContextmenu: (event) => {
      event.preventDefault()
      showLinkMenu(url, { x: event.clientX, y: event.clientY })
    },
  }
}
