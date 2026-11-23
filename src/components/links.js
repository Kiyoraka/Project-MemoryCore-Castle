// A link in a chat (HOME, a DEV tab's Chat, the CHAT room) opens on click: an https page in the
// member's browser, a server on this computer through the DEV room's opener. Nothing else opens
// (no javascript:, file:, mailto: or plain-http addresses on other machines).
import { openUrl } from '../api/castle.js'
import { devOpenLink } from '../api/devgit.js'

const LONGEST = 500
const LOCAL = /^http:\/\/(localhost|127\.0\.0\.1):\d{1,5}(\/|$)/

/** 'web' for an https page, 'local' for a server on this computer, or null: it does not open. */
export function linkWay(url) {
  if (typeof url !== 'string' || !url || url.length > LONGEST || /[\s"'\u0000-\u001f]/.test(url)) return null
  if (url.startsWith('https://') && url.length > 'https://'.length) return 'web'
  if (LOCAL.test(url)) return 'local'
  return null
}

/** Opens the link the way it allows; false when it may not open (the click then does nothing). */
export async function openLink(url) {
  const way = linkWay(url)
  if (way === 'web') await openUrl(url)
  else if (way === 'local') await devOpenLink(url)
  else return false
  return true
}
