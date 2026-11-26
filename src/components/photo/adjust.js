// PHOTO's ADJUST: eight sliders, each -100..100, never written into the pixels. They are applied
// when the picture is drawn and when it is exported. Pure, so the maths can be tested.

export const ADJUST_KEYS = ['exposure', 'brightness', 'contrast', 'highlights', 'shadows', 'warmth', 'saturation', 'sharpness']
export const ADJUST_LABELS = {
  exposure: 'Exposure',
  brightness: 'Brightness',
  contrast: 'Contrast',
  highlights: 'Highlights',
  shadows: 'Shadows',
  warmth: 'Warmth',
  saturation: 'Saturation',
  sharpness: 'Sharpness',
}

export function neutralAdjust() {
  return Object.fromEntries(ADJUST_KEYS.map((k) => [k, 0]))
}

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))

/** One slider's value: a finite number, rounded, inside -100..100 (anything else is 0). */
export function clampValue(v) {
  const n = Number(v)
  return Number.isFinite(n) ? clamp(Math.round(n), -100, 100) : 0
}

/** Every slider present and clamped; unknown keys dropped. */
export function cleanAdjust(adjust) {
  return Object.fromEntries(ADJUST_KEYS.map((k) => [k, clampValue(adjust?.[k] ?? 0)]))
}

export function isNeutral(adjust) {
  return ADJUST_KEYS.every((k) => !adjust?.[k])
}

/** An `adjust` op on the current values: `relative` adds, `set` replaces. -> { next, before } where
 * `before` holds only the sliders this op changed, so its undo puts back just those. */
export function applyAdjustOp(current, values, mode = 'relative') {
  const now = cleanAdjust(current)
  const next = { ...now }
  const before = {}
  for (const k of ADJUST_KEYS) {
    if (values?.[k] === undefined) continue
    const v = mode === 'set' ? clampValue(values[k]) : clampValue(now[k] + clampValue(values[k]))
    if (v !== now[k]) {
      before[k] = now[k]
      next[k] = v
    }
  }
  return { next, before }
}

/** A short line for a pill: "Brightness +20, Contrast -5". */
export function adjustLabel(before, next) {
  const parts = Object.keys(before).map((k) => {
    const d = next[k] - before[k]
    return `${ADJUST_LABELS[k] ?? k} ${d > 0 ? '+' : ''}${d}`
  })
  return parts.join(', ') || 'Adjust'
}

const smooth = (a, b, x) => {
  const t = clamp((x - a) / (b - a), 0, 1)
  return t * t * (3 - 2 * t)
}

/** The adjustments applied to RGBA pixels in place (alpha is never changed). Returns `data`. */
export function adjustPixels(data, width, height, adjust) {
  const a = cleanAdjust(adjust)
  if (isNeutral(a)) return data
  const tone = ADJUST_KEYS.some((k) => k !== 'sharpness' && a[k])
  if (tone) {
    const exposure = Math.pow(2, a.exposure / 50)
    const brightness = (a.brightness / 100) * 0.35
    const contrast = a.contrast >= 0 ? 1 + a.contrast / 100 : 1 + (a.contrast / 100) * 0.9
    const highlights = (a.highlights / 100) * 0.35
    const shadows = (a.shadows / 100) * 0.35
    const warmth = (a.warmth / 100) * 0.12
    const saturation = 1 + a.saturation / 100
    for (let i = 0; i < data.length; i += 4) {
      let r = (data[i] / 255) * exposure + brightness
      let g = (data[i + 1] / 255) * exposure + brightness
      let b = (data[i + 2] / 255) * exposure + brightness
      r = (r - 0.5) * contrast + 0.5
      g = (g - 0.5) * contrast + 0.5
      b = (b - 0.5) * contrast + 0.5
      if (highlights || shadows) {
        const l = clamp(0.2126 * r + 0.7152 * g + 0.0722 * b, 0, 1)
        const d = highlights * smooth(0.5, 1, l) + shadows * (1 - smooth(0, 0.5, l))
        r += d
        g += d
        b += d
      }
      r += warmth
      b -= warmth
      if (saturation !== 1) {
        const l = 0.2126 * r + 0.7152 * g + 0.0722 * b
        r = l + (r - l) * saturation
        g = l + (g - l) * saturation
        b = l + (b - l) * saturation
      }
      data[i] = clamp(Math.round(r * 255), 0, 255)
      data[i + 1] = clamp(Math.round(g * 255), 0, 255)
      data[i + 2] = clamp(Math.round(b * 255), 0, 255)
    }
  }
  if (a.sharpness) sharpen(data, width, height, a.sharpness / 100)
  return data
}

/** Sharpness: above 0 an unsharp mask over the 3x3 blur, below 0 a blend toward it. In place. */
export function sharpen(data, width, height, amount) {
  if (!amount || width < 1 || height < 1) return data
  const src = new Uint8ClampedArray(data)
  const k = amount > 0 ? amount * 1.5 : amount
  for (let y = 0; y < height; y++) {
    const y0 = y > 0 ? y - 1 : y
    const y1 = y < height - 1 ? y + 1 : y
    for (let x = 0; x < width; x++) {
      const x0 = x > 0 ? x - 1 : x
      const x1 = x < width - 1 ? x + 1 : x
      const i = (y * width + x) * 4
      for (let c = 0; c < 3; c++) {
        const blur =
          (src[(y0 * width + x0) * 4 + c] + src[(y0 * width + x) * 4 + c] + src[(y0 * width + x1) * 4 + c] +
            src[(y * width + x0) * 4 + c] + src[i + c] + src[(y * width + x1) * 4 + c] +
            src[(y1 * width + x0) * 4 + c] + src[(y1 * width + x) * 4 + c] + src[(y1 * width + x1) * 4 + c]) / 9
        // k > 0: away from the blur (sharper); k < 0: toward it (softer)
        data[i + c] = clamp(Math.round(src[i + c] + (src[i + c] - blur) * k), 0, 255)
      }
    }
  }
  return data
}
