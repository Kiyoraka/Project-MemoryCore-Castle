// PAINT's colour wheel: the hue round the circle, the saturation out from the middle, and the
// brightness from its own slider. Pure, so it is tested without a page.

const hex2 = (n) => Math.round(Math.max(0, Math.min(255, n))).toString(16).padStart(2, '0')

/** HSV (h in degrees, s and v from 0 to 1) to '#rrggbb'. */
export function hsvToHex(h, s, v) {
  const hue = ((h % 360) + 360) % 360
  const c = v * s
  const x = c * (1 - Math.abs(((hue / 60) % 2) - 1))
  const m = v - c
  const [r, g, b] = hue < 60 ? [c, x, 0] : hue < 120 ? [x, c, 0] : hue < 180 ? [0, c, x] : hue < 240 ? [0, x, c] : hue < 300 ? [x, 0, c] : [c, 0, x]
  return `#${hex2((r + m) * 255)}${hex2((g + m) * 255)}${hex2((b + m) * 255)}`
}

/** '#rrggbb' to { h, s, v }. */
export function hexToHsv(hex) {
  const h6 = String(hex).replace('#', '')
  const [r, g, b] = [0, 2, 4].map((i) => (parseInt(h6.slice(i, i + 2), 16) || 0) / 255)
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const d = max - min
  let h = 0
  if (d) {
    if (max === r) h = 60 * (((g - b) / d) % 6)
    else if (max === g) h = 60 * ((b - r) / d + 2)
    else h = 60 * ((r - g) / d + 4)
  }
  return { h: (h + 360) % 360, s: max ? d / max : 0, v: max }
}

/** The colour under a point (dx, dy from the wheel's middle), or null outside the wheel. */
export function wheelColour(dx, dy, radius, value = 1) {
  const dist = Math.hypot(dx, dy)
  if (dist > radius) return null
  const h = (Math.atan2(dy, dx) * 180) / Math.PI
  return hsvToHex(h, Math.min(1, dist / radius), value)
}

/** Where a colour sits on the wheel: { x, y } from the middle. */
export function wheelPoint(hex, radius) {
  const { h, s } = hexToHsv(hex)
  const a = (h * Math.PI) / 180
  return { x: Math.cos(a) * s * radius, y: Math.sin(a) * s * radius }
}
