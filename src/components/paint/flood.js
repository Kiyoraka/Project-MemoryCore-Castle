// PAINT's bucket: a scanline flood fill on one layer's pixels (RGBA, row by row), with a
// tolerance - the largest difference in any channel a pixel may have from the one clicked and
// still be filled. Pure, so it is tested without a page.

/** The fill colour from '#rrggbb' (fully opaque) or '#rrggbbaa'. */
export function rgbaOf(hex) {
  const h = String(hex).replace('#', '')
  const full = h.length === 3 ? h.split('').map((c) => c + c).join('') : h
  const n = (i) => parseInt(full.slice(i, i + 2), 16)
  return [n(0) || 0, n(2) || 0, n(4) || 0, full.length >= 8 ? n(6) : 255]
}

/**
 * Fills the area around (x, y) whose pixels are within `tolerance` of the one there, with `colour`
 * ([r, g, b, a]). Changes `data` in place and returns how many pixels were filled.
 */
export function floodFill(data, width, height, x, y, colour, tolerance = 0) {
  x = Math.floor(x)
  y = Math.floor(y)
  if (x < 0 || y < 0 || x >= width || y >= height) return 0
  const at = (y * width + x) * 4
  const target = [data[at], data[at + 1], data[at + 2], data[at + 3]]
  const tol = Math.max(0, Math.min(255, tolerance))
  const same = (i) =>
    Math.abs(data[i] - target[0]) <= tol && Math.abs(data[i + 1] - target[1]) <= tol && Math.abs(data[i + 2] - target[2]) <= tol && Math.abs(data[i + 3] - target[3]) <= tol
  if (tol === 0 && target.every((v, i) => v === colour[i])) return 0
  const done = new Uint8Array(width * height)
  const fits = (px, py) => {
    const p = py * width + px
    return !done[p] && same(p * 4)
  }
  const stack = [[x, y]]
  let filled = 0
  while (stack.length) {
    const [sx, sy] = stack.pop()
    if (!fits(sx, sy)) continue
    let left = sx
    while (left > 0 && fits(left - 1, sy)) left--
    let right = sx
    while (right < width - 1 && fits(right + 1, sy)) right++
    for (let px = left; px <= right; px++) {
      const p = sy * width + px
      done[p] = 1
      const i = p * 4
      data[i] = colour[0]
      data[i + 1] = colour[1]
      data[i + 2] = colour[2]
      data[i + 3] = colour[3]
      filled++
    }
    // one seed per run of fillable pixels on the rows above and below
    for (const ny of [sy - 1, sy + 1]) {
      if (ny < 0 || ny >= height) continue
      let inRun = false
      for (let px = left; px <= right; px++) {
        if (fits(px, ny)) {
          if (!inRun) stack.push([px, ny])
          inRun = true
        } else inRun = false
      }
    }
  }
  return filled
}
