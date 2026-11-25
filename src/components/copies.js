// Two copies of Castle on one PC (Rust: install_check::Copies - { this, installed, leftover, two }).
// A leftover lives inside another app's private folder (an installer run from inside that app);
// the line points the member to the real one.

/** The line to show, or '' when this is the only, real copy. */
export function copiesLine(copies) {
  if (!copies) return ''
  if (copies.leftover) {
    const real = copies.installed ? ` Open the real one in ${copies.installed}, or install` : ' Install'
    return `This Castle is a leftover copy inside another app's private folder (${copies.this}).${real} Castle again from its download page, not from inside another app.`
  }
  if (copies.two && copies.installed) {
    return `Two copies of Castle are on this PC: this one (${copies.this}) and the installed one (${copies.installed}). Updates go to the installed one.`
  }
  return ''
}

/** The short words on the top bar's chip. */
export const copiesChip = (copies) => (copies?.leftover ? 'Leftover copy' : copies?.two ? 'Two copies' : '')
