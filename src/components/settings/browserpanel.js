// The BROWSER row's fold-out panel in Settings > Rooms (the pick rooms-accordion-1): the chevron
// opens and closes it whether BROWSER is pinned or not, and pinning BROWSER opens it by itself.

/** Whether the panel is open after a change: a fresh pin opens it; otherwise it stays as it was. */
export function panelAfterPin(wasPinned, nowPinned, opened) {
  return !wasPinned && nowPinned ? true : opened
}
