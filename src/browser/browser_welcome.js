// The browser's first-open welcome (browser_welcome.rs): one step at a time, each the member's own
// Yes or Skip. A step with an answer is never offered again unless Settings opens the welcome back.

export const STEPS = ['history', 'bookmarks', 'passwords', 'sitecheck']

/** The step to offer now: the first with no answer, or null when every step is answered. */
export function nextStep(answers) {
  const given = answers ?? {}
  return STEPS.find((step) => given[step] !== 'yes' && given[step] !== 'skip') ?? null
}

/** The step the card shows: without Chrome on this PC only the site check is offered. */
export function offeredStep(answers, chrome) {
  const given = answers ?? {}
  if (chrome) return nextStep(given)
  return given.sitecheck === 'yes' || given.sitecheck === 'skip' ? null : 'sitecheck'
}

/** Which step of how many, for the card's corner ("2 of 4"). */
export function stepNumber(step) {
  return STEPS.indexOf(step) + 1
}

/** The card's words for a step, in the companion's name. */
export function stepWords(step, companion = 'your companion') {
  const who = companion || 'your companion'
  switch (step) {
    case 'history':
      return {
        title: 'Bring your history from Chrome?',
        body: 'Castle reads a copy of Chrome\'s history, so Chrome is never touched: the sites and pages you visited and their titles, and the words you searched for, so the address bar can suggest them. Never the part of an address after ? or #. It stays in your own memory database on this PC, and fills Most visited on the new tab.',
        yes: 'Bring my history',
      }
    case 'bookmarks':
      return {
        title: 'Bring your bookmarks?',
        body: 'From a copy of Chrome\'s bookmarks file, with their folders. Web addresses only. They show on the new tab, and the star in the toolbar keeps new ones.',
        yes: 'Bring my bookmarks',
      }
    case 'passwords':
      return {
        title: 'Bring your saved passwords?',
        body: `Only if you want to. Chrome exports them itself (it asks for your Windows PIN); you choose that file here, and Castle's own code seals each password into a vault on this PC and shreds the file at once. ${who} can walk you through it, but never sees a password. On a saved site's sign-in page, a Fill button fills it for you.`,
        yes: 'Choose the exported file...',
        guide: `Ask ${who} to guide me`,
      }
    case 'sitecheck':
      return {
        title: `Let ${who} check sites for you?`,
        body: `Every page is checked for phishing as it loads. With your yes, ${who} also judges the sites you visit by name and asks at once about a sign-in page it has not seen. The sites are kept in your own memory database; nothing about your browsing leaves this PC except a site's name in ${who}'s own turn.`,
        yes: 'Yes, check them',
      }
    default:
      return null
  }
}

/** The line after a step was done. */
export function doneLine(step, result) {
  if (step === 'history') return `${result} page${result === 1 ? '' : 's'} of history brought over.`
  if (step === 'bookmarks') return `${result} bookmark${result === 1 ? '' : 's'} brought over.`
  if (step === 'passwords' && result) {
    const n = result.sealed
    const sealed = `${n} password${n === 1 ? '' : 's'} sealed in your vault`
    return result.shredded ? `${sealed}; the export file is gone.` : `${sealed}, but the export file could not be removed: delete it yourself now.`
  }
  return ''
}
