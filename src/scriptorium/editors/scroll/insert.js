// What Scroll's two asking buttons need from the member, turned into bus commands. Host-agnostic: the
// room (or anything else) shows a file picker for 'insert.picture' and a prompt for 'insert.link', then
// hands what it got to these and runs the answer with the selection.
//
//   INSERT_INTENTS['insert.picture'] -> { command, asks: 'file', accept }
//   INSERT_INTENTS['insert.link']    -> { command, asks: 'url' }
//   await pictureCommand(file)       -> { command: 'scroll.insert_picture', args: { data, name } }
//   linkCommand(url, text?)          -> { command: 'scroll.link', args: { url, text? } } or null

export const INSERT_INTENTS = {
  'insert.picture': { command: 'scroll.insert_picture', asks: 'file', accept: 'image/png,image/jpeg,image/gif,image/bmp' },
  'insert.link': { command: 'scroll.link', asks: 'url' },
}

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'

/** Standard base64 of bytes (a Uint8Array or an ArrayBuffer). */
export function toBase64(bytes) {
  const b = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)
  let out = ''
  for (let i = 0; i < b.length; i += 3) {
    const n = (b[i] << 16) | ((b[i + 1] ?? 0) << 8) | (b[i + 2] ?? 0)
    out += B64[(n >> 18) & 63] + B64[(n >> 12) & 63]
    out += i + 1 < b.length ? B64[(n >> 6) & 63] : '='
    out += i + 2 < b.length ? B64[n & 63] : '='
  }
  return out
}

/** The insert-picture command for a picked file (anything with arrayBuffer() and a name). */
export async function pictureCommand(file, size = {}) {
  const data = toBase64(await file.arrayBuffer())
  const args = { data, name: String(file?.name ?? 'picture') }
  if (Number.isFinite(size.width)) args.width = size.width
  if (Number.isFinite(size.height)) args.height = size.height
  return { command: 'scroll.insert_picture', args }
}

/** The link command for an address the member typed: a bare domain gets https://, an address with an @
 * becomes mailto:, #name links to a bookmark. Null for nothing usable. */
export function linkCommand(url, text) {
  let u = String(url ?? '').trim()
  if (!u) return null
  if (!/^[a-z][a-z0-9+.-]*:/i.test(u) && !u.startsWith('#')) u = /^[^\s/@]+@[^\s/@]+\.[^\s/@]+$/.test(u) ? `mailto:${u}` : `https://${u}`
  const args = { url: u }
  if (text) args.text = String(text)
  return { command: 'scroll.link', args }
}
