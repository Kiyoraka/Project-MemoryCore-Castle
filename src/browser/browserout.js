// A browser tab's window of its own (BrowserOut.vue, browser_out.rs): what its header shows and the
// words of its way home.
import { chipTitle } from './browserview.js'

/** The window's header from every tab: one chip per page living in this window (its title cut to 28
 * characters, its news, private, held back), the pages left to right, the page the address row acts
 * on (the one asked for while it is still here, else the first) and its address; null once none of its
 * tabs are left. */
export function outHeader(list, window, focusId = null) {
  const members = (list ?? []).filter((t) => t.window === window)
  if (!members.length) return null
  const focused = members.find((t) => t.id === focusId) ?? members[0]
  return {
    chips: members.map((t) => ({
      id: t.id,
      title: chipTitle(t.title || 'Page'),
      full: t.private ? `Private - ${t.url || t.title}` : t.url || t.title,
      unread: Number(t.unread) || 0,
      private: !!t.private,
      danger: !!t.danger,
    })),
    ids: members.map((t) => t.id),
    focus: focused.id,
    url: focused.url ?? '',
    private: !!focused.private,
  }
}

/** The pill that takes the pages home: the browser inside Castle, or its own window (Settings > Rooms
 * > BROWSER > Browser opens). */
export function backWord(opens) {
  return opens === 'castle' ? 'Back to Castle' : 'Back to the browser'
}

/** The pages' grid columns: equal shares, a 10 px gap between two. */
export function outColumns(count) {
  const n = Math.max(1, Number(count) || 1)
  return Array.from({ length: n }, () => '1fr').join(' 10px ')
}
