// The room's line icons: 24x24, drawn with currentColor so they follow the host's accent. Bundled
// artwork only (shown with v-html), never anything a document brings.

const P = {
  new: '<path d="M6 3h8l4 4v14H6Z"/><path d="M14 3v4h4"/>',
  open: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>',
  save: '<path d="M5 4h11l3 3v13H5Z"/><path d="M8 4v5h7V4M8 20v-6h8v6"/>',
  saveAs: '<path d="M13 20H5V4h11l3 3v5"/><path d="M8 4v5h7V4M8 20v-6h5"/><path d="M15 21l1-3 4-4 2 2-4 4Z"/>',
  undo: '<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11"/>',
  redo: '<path d="m15 14 5-5-5-5"/><path d="M20 9H9.5a5.5 5.5 0 0 0 0 11H13"/>',
  bold: '<path d="M7 5h6a3.5 3.5 0 0 1 0 7H7Zm0 7h7a3.5 3.5 0 0 1 0 7H7Z"/>',
  italic: '<path d="M10 5h8M6 19h8M14 5l-4 14"/>',
  underline: '<path d="M7 4v7a5 5 0 0 0 10 0V4M5 20h14"/>',
  bullets: '<path d="M9 6h11M9 12h11M9 18h11"/><circle cx="4.5" cy="6" r="1"/><circle cx="4.5" cy="12" r="1"/><circle cx="4.5" cy="18" r="1"/>',
  numbers: '<path d="M10 6h10M10 12h10M10 18h10M4 5l1.5-1v5M3.5 13.5a1.5 1.5 0 1 1 2.6 1L3.5 17h3"/>',
  alignLeft: '<path d="M4 6h16M4 10h10M4 14h16M4 18h10"/>',
  alignCenter: '<path d="M4 6h16M7 10h10M4 14h16M7 18h10"/>',
  alignRight: '<path d="M4 6h16M10 10h10M4 14h16M10 18h10"/>',
  image: '<rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="8.5" cy="10" r="1.5"/><path d="m4 18 5-5 4 4 3-3 4 4"/>',
  link: '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
  table: '<rect x="3" y="4" width="18" height="16" rx="1.5"/><path d="M3 9.5h18M3 15h18M9 4v16M15 4v16"/>',
  spark: '<path d="M12 3c.6 4.6 2.4 6.4 7 7-4.6.6-6.4 2.4-7 7-.6-4.6-2.4-6.4-7-7 4.6-.6 6.4-2.4 7-7Z"/><path d="M19 15c.2 1.5.8 2 2 2.2-1.2.2-1.8.8-2 2.3-.2-1.5-.8-2.1-2-2.3 1.2-.2 1.8-.7 2-2.2Z"/>',
  scroll: '<path d="M6 3h8l4 4v14H6Z"/><path d="M14 3v4h4M9 12h6M9 15h6M9 18h4"/>',
  ledger: '<rect x="4" y="4" width="16" height="16" rx="1.5"/><path d="M4 9h16M4 14h16M10 4v16"/>',
  stage: '<rect x="3" y="5" width="18" height="12" rx="1.5"/><path d="M12 17v3M8 20h8M7 13l3-3 2 2 4-4"/>',
  folio: '<path d="M6 3h8l4 4v14H6Z"/><path d="M14 3v4h4"/><path d="M8.5 16v-4h1.5a1.2 1.2 0 0 1 0 2.4H8.5M12.5 16v-4h1a2 2 0 0 1 0 4Z"/>',
  clock: '<circle cx="12" cy="12" r="8"/><path d="M12 8v4l3 2"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>',
  text: '<path d="M5 6V4h14v2M12 4v16M9 20h6"/>',
  sliders: '<path d="M4 7h10M18 7h2M4 17h4M12 17h8"/><circle cx="16" cy="7" r="2"/><circle cx="10" cy="17" r="2"/>',
  palette: '<path d="M12 3a9 9 0 0 0 0 18c1.2 0 2-.8 2-2 0-.6-.3-1-.6-1.4-.3-.4-.6-.8-.6-1.4 0-1.1.9-2 2-2h2.2A4 4 0 0 0 21 10c0-3.9-4-7-9-7Z"/><circle cx="7.5" cy="11" r="1"/><circle cx="10" cy="7.5" r="1"/><circle cx="14.5" cy="7.5" r="1"/>',
  paragraph: '<path d="M4 6h16M4 10h16M4 14h10M4 18h7"/>',
  frame: '<path d="M4 8V4h4M16 4h4v4M20 16v4h-4M8 20H4v-4"/>',
  print: '<path d="M7 9V3h10v6"/><rect x="3" y="9" width="18" height="8" rx="2"/><path d="M7 14h10v7H7Z"/>',
  exportPdf: '<path d="M6 3h8l4 4v6"/><path d="M14 3v4h4M6 3v18h7"/><path d="M17 15v6M14 18l3 3 3-3"/>',
  find: '<circle cx="11" cy="11" r="6"/><path d="m20 20-4.5-4.5"/>',
  replace: '<path d="M4 7h11l-3-3M20 17H9l3 3"/>',
  close: '<path d="M6 6l12 12M18 6 6 18"/>',
  up: '<path d="m6 15 6-6 6 6"/>',
  down: '<path d="m6 9 6 6 6-6"/>',
}

/** The inner markup of an icon (empty for a name it does not know). */
export function iconPaths(name) {
  return P[name] ?? ''
}

/** A whole <svg> for v-html. */
export function icon(name, size = 18) {
  const body = iconPaths(name)
  if (!body) return ''
  return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`
}

/** A button's icon: a name above, or an editor's own inline SVG markup (24x24, currentColor) - bundled
 * with the editor, never anything a document brings. */
export function iconOf(nameOrMarkup, size = 18) {
  const v = String(nameOrMarkup ?? '').trim()
  if (!v.startsWith('<')) return icon(v, size)
  return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${v}</svg>`
}

export const ICON_NAMES = Object.keys(P)
