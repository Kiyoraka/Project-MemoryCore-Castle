// "Update by itself" (Settings > About): when it may start, kept apart from UpdateChip so it can be
// tested. An update restarts Castle, so it waits until the companion has been quiet for a while -
// never in the middle of an answer - and it never starts twice.

/** How long the companion must stay quiet before Castle updates by itself. */
export const QUIET_MS = 2 * 60 * 1000

/** True when Castle may install the waiting update by itself now. */
export function mayUpdateByItself({ status, on, busy, step }) {
  return !!(on && status?.available && status.canInstall && !busy && !step)
}
