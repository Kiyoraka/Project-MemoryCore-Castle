// A phone in the DEV tab (S-1124-5, his picks mobile-emulator-3 + mobile-emulator-1): the pure side of
// Mobile setup and the docked phone. Lifted from the Alice Keep (MIT); the companion's install
// sentence is worded for the member's own computer (Windows, macOS or Linux), never one PC's.

import { detectMac } from '../../platform.js'

/** Which computer this is, in the words the install sentence needs: 'windows', 'mac' or 'linux'. */
export function osOf(nav) {
  if (detectMac(nav)) return 'mac'
  const said = `${nav?.userAgentData?.platform ?? ''} ${nav?.platform ?? ''} ${nav?.userAgent ?? ''}`
  return /linux|x11/i.test(said) && !/android/i.test(said) ? 'linux' : 'windows'
}

const pageNavigator = typeof window !== 'undefined' && typeof navigator !== 'undefined' ? navigator : undefined
export const thisOs = osOf(pageNavigator)

/** How a setup row looks: a green check, an amber ring (we can make it here), an empty ring (to install). */
export function rowMark(row) {
  if (row?.ok) return 'ok'
  return row?.action === 'create' ? 'create' : 'missing'
}

// what the companion is asked to install, row by row, in words it can act on on this computer
function asks(os) {
  const image = os === 'mac' ? 'arm64-v8a on Apple silicon, x86_64 on an Intel Mac' : 'x86_64'
  return {
    sdk: 'the Android SDK (Android Studio, or its command-line tools)',
    emulator: 'the Android Emulator (sdkmanager "emulator")',
    phone: `an Android system image to make a phone from (sdkmanager "system-images;android-35;google_apis;<abi>", ${image})`,
    flutter: 'Flutter (the stable channel, with its bin folder on the PATH)',
    java: 'a Java JDK 17 or newer',
    xcode: 'Xcode from the App Store, then its command-line tools (xcode-select --install) and its first launch',
    simulator: 'an iPhone simulator runtime (Xcode > Settings > Components)',
  }
}

// how to install on this computer
const HOW = {
  windows: 'Use winget or the official installers',
  mac: 'Use Homebrew or the official installers',
  linux: 'Use the official installers or the package manager',
}

/** The one sentence typed into this tab's companion session for what is missing (one row, or every row). */
export function companionSentence(rows, only = '', os = thisOs) {
  const ask = asks(os)
  const wanted = (rows ?? []).filter((r) => !r.ok && r.action === 'install' && (!only || r.id === only)).map((r) => ask[r.id] ?? r.label)
  if (!wanted.length) return ''
  return `Please install what this project's phone setup is missing: ${wanted.join('; ')}. ${HOW[os] ?? HOW.windows}, then tell me when it's done so I can check again.`
}

/** A new phone's name that no phone here has yet: Castle_Phone, Castle_Phone_2, ... */
export function newPhoneName(phones) {
  const taken = new Set((phones ?? []).map((p) => p.name))
  if (!taken.has('Castle_Phone')) return 'Castle_Phone'
  let n = 2
  while (taken.has(`Castle_Phone_${n}`)) n += 1
  return `Castle_Phone_${n}`
}

/** The image a new phone is made from: the newest Android with Google Play, else the newest of all. */
export function bestImage(images) {
  const list = images ?? []
  return list.find((i) => i.includes('playstore')) ?? list[0] ?? ''
}

/** A phone's one line under its name: "1080 x 2400 · Android 35". */
export function phoneLine(phone) {
  const size = phone?.width && phone?.height ? `${phone.width} x ${phone.height}` : ''
  const api = phone?.api ? `Android ${phone.api}` : ''
  return [size, api].filter(Boolean).join(' · ')
}

/** Whether the iOS side can be used: a Flutter project only (Kotlin builds Android apps only). */
export function iosAllowed(kind) {
  return kind === 'flutter'
}

// THE DOCKED PHONE (his pick mobile-emulator-1)

/** A frame from the phone's channel: width and height (u32, little-endian), then the RGBA bytes; null when torn. */
export function parseFrame(buffer) {
  if (!(buffer instanceof ArrayBuffer) || buffer.byteLength < 8) return null
  const head = new DataView(buffer, 0, 8)
  const width = head.getUint32(0, true)
  const height = head.getUint32(4, true)
  if (!width || !height || buffer.byteLength !== 8 + width * height * 4) return null
  return { width, height, pixels: new Uint8ClampedArray(buffer, 8) }
}

/** The phone's own size in the frame's orientation (its config is portrait; a landscape frame swaps it). */
export function deviceSize(phone, frame) {
  let w = phone?.width || frame?.width || 0
  let h = phone?.height || frame?.height || 0
  if (frame && frame.width > frame.height !== w > h) [w, h] = [h, w]
  return { width: w, height: h }
}

/** A point on the canvas (as shown) to a point on the phone's own screen. */
export function toDevice(x, y, shown, device) {
  if (!shown?.width || !shown?.height) return null
  const fx = Math.min(Math.max(x / shown.width, 0), 1)
  const fy = Math.min(Math.max(y / shown.height, 0), 1)
  return { x: Math.round(fx * (device.width - 1)), y: Math.round(fy * (device.height - 1)) }
}

/** The width of frame to ask for: the panel's width in real pixels, 120 to 540. */
export function frameWidth(cssWidth, ratio = 1) {
  return Math.min(540, Math.max(120, Math.round(cssWidth * (ratio || 1))))
}

/** The state line under the phone. */
export function stateLine(state) {
  if (!state || state.state === 'off') return 'The phone is off.'
  return state.note || state.state
}

// RUN (Phase 171)

// colours, cursor moves and window titles a console writes (ESC sequences)
const ESC = String.fromCharCode(27)
const BEL = String.fromCharCode(7)
const ESCAPES = new RegExp(`${ESC}\\[[0-9;?]*[ -/]*[@-~]|${ESC}\\][^${BEL}]*(${BEL}|${ESC}\\\\)|${ESC}[()][A-Za-z0-9]`, 'g')

/** A console's text as plain lines: colours and cursor moves taken out, carriage returns folded. */
export function plainLines(text) {
  return String(text ?? '')
    .replace(ESCAPES, '')
    .split('\n')
    .map((line) => {
      const parts = line.split('\r').filter((p) => p !== '')
      return parts.length ? parts[parts.length - 1] : ''
    })
}

/** Whether Flutter's web server says it is serving (the iPhone preview can load then). */
export function servedIn(text) {
  return /is being served at|Serving at http/i.test(String(text ?? ''))
}

/** The run's text kept to its last lines (new text added to what was there). */
export function keepTail(lines, more, most = 60) {
  const all = [...(lines ?? [])]
  const fresh = plainLines(more)
  if (all.length && fresh.length) all[all.length - 1] += fresh.shift()
  return [...all, ...fresh].slice(-most)
}

