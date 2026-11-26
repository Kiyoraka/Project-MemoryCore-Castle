// PHOTO's selections: the magic wand's flood (on the composite), and what a selection mask is made
// of - its box, its rows as rectangles (for clipping a brush) and its edges (the marching ants).
// Pure, so it can be tested.

/** The magic wand: every pixel joined to (x, y) through neighbours (4-way) whose colour is within
 * `tolerance` (0..255, the largest channel difference, alpha included) of the clicked one. With
 * `contiguous` false, every such pixel in the picture. -> Uint8Array mask, 1 = selected. */
export function floodMask(data, width, height, x, y, tolerance = 32, contiguous = true) {
  const mask = new Uint8Array(width * height)
  x = Math.floor(x)
  y = Math.floor(y)
  if (x < 0 || y < 0 || x >= width || y >= height) return mask
  const tol = Math.max(0, Math.min(255, Number(tolerance) || 0))
  const s = (y * width + x) * 4
  const r = data[s], g = data[s + 1], b = data[s + 2], a = data[s + 3]
  const near = (p) => {
    const i = p * 4
    return Math.abs(data[i] - r) <= tol && Math.abs(data[i + 1] - g) <= tol && Math.abs(data[i + 2] - b) <= tol && Math.abs(data[i + 3] - a) <= tol
  }
  if (!contiguous) {
    for (let p = 0; p < width * height; p++) if (near(p)) mask[p] = 1
    return mask
  }
  const seen = new Uint8Array(width * height)
  const stack = [y * width + x]
  seen[stack[0]] = 1
  while (stack.length) {
    const p = stack.pop()
    if (!near(p)) continue
    mask[p] = 1
    const px = p % width
    const py = (p - px) / width
    if (px > 0 && !seen[p - 1]) { seen[p - 1] = 1; stack.push(p - 1) }
    if (px < width - 1 && !seen[p + 1]) { seen[p + 1] = 1; stack.push(p + 1) }
    if (py > 0 && !seen[p - width]) { seen[p - width] = 1; stack.push(p - width) }
    if (py < height - 1 && !seen[p + width]) { seen[p + width] = 1; stack.push(p + width) }
  }
  return mask
}

/** The smallest box around the selected pixels, or null when none is. */
export function maskBounds(mask, width, height) {
  let x0 = width, y0 = height, x1 = -1, y1 = -1
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (!mask[y * width + x]) continue
      if (x < x0) x0 = x
      if (x > x1) x1 = x
      if (y < y0) y0 = y
      if (y > y1) y1 = y
    }
  }
  return x1 < 0 ? null : { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 }
}

/** Each row's selected runs as rectangles [x, y, w, 1] (a clip path from them covers the mask exactly). */
export function maskRuns(mask, width, height) {
  const runs = []
  for (let y = 0; y < height; y++) {
    let start = -1
    for (let x = 0; x <= width; x++) {
      const on = x < width && mask[y * width + x]
      if (on && start < 0) start = x
      if (!on && start >= 0) {
        runs.push([start, y, x - start, 1])
        start = -1
      }
    }
  }
  return runs
}

/** The mask's edges as line segments [x1, y1, x2, y2] in pixel corners, joined along each row and
 * column, for the marching ants. */
export function maskOutline(mask, width, height) {
  const at = (x, y) => x >= 0 && y >= 0 && x < width && y < height && mask[y * width + x] === 1
  const segs = []
  // horizontal edges: between row y-1 and row y
  for (let y = 0; y <= height; y++) {
    let start = -1
    for (let x = 0; x <= width; x++) {
      const edge = x < width && at(x, y) !== at(x, y - 1)
      if (edge && start < 0) start = x
      if (!edge && start >= 0) {
        segs.push([start, y, x, y])
        start = -1
      }
    }
  }
  // vertical edges: between column x-1 and column x
  for (let x = 0; x <= width; x++) {
    let start = -1
    for (let y = 0; y <= height; y++) {
      const edge = y < height && at(x, y) !== at(x - 1, y)
      if (edge && start < 0) start = y
      if (!edge && start >= 0) {
        segs.push([x, start, x, y])
        start = -1
      }
    }
  }
  return segs
}

/** A lasso's points (at least three) inside the picture, or null. */
export function cleanLasso(points, width, height) {
  const kept = (points ?? [])
    .filter((p) => Array.isArray(p) && Number.isFinite(p[0]) && Number.isFinite(p[1]))
    .map(([x, y]) => [Math.min(width, Math.max(0, x)), Math.min(height, Math.max(0, y))])
  return kept.length >= 3 ? kept : null
}

/** The box around a lasso's points. */
export function lassoBounds(points) {
  const xs = points.map((p) => p[0])
  const ys = points.map((p) => p[1])
  const x = Math.floor(Math.min(...xs))
  const y = Math.floor(Math.min(...ys))
  return { x, y, w: Math.ceil(Math.max(...xs)) - x, h: Math.ceil(Math.max(...ys)) - y }
}

/** A rectangle dragged from (x0, y0) to (x1, y1), in whole pixels inside the picture, or null when
 * it is less than one pixel wide or tall. */
export function dragRect(x0, y0, x1, y1, width, height) {
  const left = Math.max(0, Math.floor(Math.min(x0, x1)))
  const top = Math.max(0, Math.floor(Math.min(y0, y1)))
  const right = Math.min(width, Math.ceil(Math.max(x0, x1)))
  const bottom = Math.min(height, Math.ceil(Math.max(y0, y1)))
  return right - left >= 1 && bottom - top >= 1 ? { x: left, y: top, w: right - left, h: bottom - top } : null
}

/** What the document keeps of a selection: its kind and box (a mask or lasso is not saved). */
export function selectionBox(sel) {
  if (!sel) return null
  if (sel.kind === 'rect') return { kind: 'rect', x: sel.x, y: sel.y, w: sel.w, h: sel.h }
  const b = sel.bounds ?? (sel.points ? lassoBounds(sel.points) : null)
  return b ? { kind: sel.kind, ...b } : null
}
