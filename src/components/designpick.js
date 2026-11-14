// Choosing designs in the chat (Phase 49): a group of two to four pictures from one `Design`
// folder can be ticked and chosen, one or several at once. Pure, tested.

const folderOf = (path) => String(path ?? '').replace(/[\\/][^\\/]*$/, '')
const nameOf = (path) => String(path ?? '').split(/[\\/]/).pop()

/** True when a picture group is a set of designs to choose from: two to four pictures, all in the same folder, inside a folder named Design. */
export function isDesignGroup(list) {
  const paths = (list ?? []).map((p) => p.path ?? p)
  if (paths.length < 2 || paths.length > 4) return false
  const folder = folderOf(paths[0])
  if (!folder || !paths.every((p) => folderOf(p) === folder)) return false
  return folder.split(/[\\/]/).some((part) => part.toLowerCase() === 'design')
}

/**
 * The line Choose sends: the Alice Keep's own words first (`choose design <file>` or
 * `choose design <a>, <b>`, in picture order), then one plain sentence for a companion that has
 * no special command, so every companion knows what to do.
 */
export function chooseLine(list, ticked) {
  const chosen = (list ?? []).map((p) => p.path ?? p).filter((p) => ticked.includes(p))
  if (!chosen.length) return ''
  const names = chosen.map(nameOf)
  const head = `choose design ${names.join(', ')}`
  if (names.length === 1) {
    return `${head} - I choose ${names[0]}. Keep it, move the other designs in that folder to an archived folder beside them, and build from it.`
  }
  const said = `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`
  return `${head} - I choose ${said}. Keep them, move the rest to an archived folder beside them, and offer the chosen ones as looks I can switch in Settings (like the THEME looks) rather than building only one.`
}
