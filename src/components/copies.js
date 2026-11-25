// Two copies of Castle on one PC (Rust: install_check::Copies - { this, installed, leftover, two }).
// A leftover lives inside another app's private folder (an installer run from inside that app);
// the line points the member to the real one.

/** The line to show, or '' when this is the only, real copy and nothing newer used this PC's Castle. */
export function copiesLine(copies) {
  if (!copies) return ''
  const newer = newerLine(copies)
  if (newer && !copies.leftover && !copies.two) return newer
  if (copies.leftover) {
    const real = copies.installed ? ` Open the real one in ${copies.installed}, or install` : ' Install'
    return `This Castle is a leftover copy inside another app's private folder (${copies.this}).${real} Castle again from its download page, not from inside another app.`
  }
  if (copies.two && copies.installed) {
    return `Two copies of Castle are on this PC: this one (${copies.this}) and the installed one (${copies.installed}). Updates go to the installed one.`
  }
  return ''
}

/** An older Castle opening what a newer one used: said, never changed. */
export function newerLine(copies) {
  if (copies?.newerSettings) return `These settings were last used by Castle ${copies.newerSettings}; this one is older. Open the newer Castle.`
  if (copies?.newerDatabase) return 'Your memory database was last used by a newer Castle; this one is older. Open the newer Castle.'
  return ''
}

/** The short words on the top bar's chip. */
export const copiesChip = (copies) => (copies?.leftover ? 'Leftover copy' : copies?.two ? 'Two copies' : newerLine(copies) ? 'Older Castle' : '')
