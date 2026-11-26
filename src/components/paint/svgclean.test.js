import { describe, expect, it } from 'vitest'
import { cleanSvg, decodeEntities, svgDataUrl, valueOk } from './svgclean.js'

const clean = (svg, prefix) => {
  const r = cleanSvg(svg, 800, 600, prefix)
  if (r.error) throw new Error(r.error)
  return r.svg
}

describe("PAINT's SVG allow-list in the page", () => {
  it('keeps a plain drawing and forces the canvas viewBox', () => {
    expect(clean('<svg viewBox="0 0 10 10" width="10"><circle cx="5" cy="5" r="4" fill="#ffd24a"/></svg>')).toBe(
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 600"><circle cx="5" cy="5" r="4" fill="#ffd24a"/></svg>',
    )
  })

  it('drops scripts, handlers, links, images, foreignObject and styles', () => {
    const out = clean(`<svg onload="alert(1)"><script>alert(1)</script><style>*{}</style><a href="https://x"><path d="M0 0"/></a>
      <image href="https://x/a.png"/><foreignObject><div>hi</div></foreignObject>
      <path d="M1 1L5 5" onclick="x()" style="fill:red" xlink:href="#a" href="#b" stroke="red"/></svg>`)
    for (const bad of ['script', 'alert', 'style', 'onclick', 'onload', 'href', 'image', 'foreignObject', 'div', 'https', '<a']) expect(out).not.toContain(bad)
    expect(out).toContain('<path d="M1 1L5 5" stroke="red"/>')
  })

  it('keeps only a local url', () => {
    const out = clean('<svg><defs><linearGradient id="g1"><stop offset="0" stop-color="#fff"/></linearGradient></defs><rect width="5" height="5" fill="url(#g1)" stroke="url(https://evil/x)"/></svg>')
    expect(out).toContain('fill="url(#g1)"')
    expect(out).not.toContain('evil')
    expect(valueOk('fill', 'url(data:image/png;base64,AAA)')).toBe(false)
    expect(valueOk('fill', "url( 'javascript:x' )")).toBe(false)
    expect(valueOk('fill', 'url(#a')).toBe(false)
    expect(valueOk('fill', 'red;background:url(x)')).toBe(false)
    expect(valueOk('id', '1abc')).toBe(false)
    expect(valueOk('id', 'moon-glow_2')).toBe(true)
  })

  it('lets no entity smuggle anything in', () => {
    expect(decodeEntities('&#x75;rl(&#58;) &amp; &bogus;')).toBe('url(:) & &bogus;')
    const out = clean('<svg><rect width="5" height="5" fill="&#x75;rl(http&#58;//x)"/><title>a &lt;moon&gt; &amp; me</title></svg>')
    expect(out).toContain('<rect width="5" height="5"/>')
    expect(out).toContain('<title>a &lt;moon&gt; &amp; me</title>')
  })

  it('skips unknown elements whole and unwraps every svg', () => {
    const out = clean('<svg><g transform="translate(2 3)"><svg x="5"><ellipse rx="1" ry="2"/></svg><unknown><path d="M9 9"/></unknown><polyline points="1,2 3,4"/></g></svg>')
    expect(out.match(/<svg/g)).toHaveLength(1)
    expect(out).toContain('<g transform="translate(2 3)"><ellipse rx="1" ry="2"/><polyline points="1,2 3,4"/></g>')
    expect(out).not.toContain('M9 9')
  })

  it('drops comments, CDATA, doctypes and processing lines', () => {
    const out = clean('<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY x "boom">]><svg><!-- <script> --><![CDATA[<script>]]><line x1="0" y1="0" x2="5" y2="5" stroke="black"/></svg>')
    expect(out).not.toMatch(/boom|script|xml version/)
    expect(out).toContain('<line')
  })

  it('closes what was left open and gives a fragment its root', () => {
    expect(clean('<g><path d="M0 0L1 1">')).toMatch(/<g><path d="M0 0L1 1"><\/path><\/g><\/svg>$/)
    expect(clean('<rect width="2" height="2"/><circle r="2"/><rect width="5')).toContain('<circle r="2"/>')
  })

  it('refuses an empty or oversized drawing', () => {
    expect(cleanSvg('<svg><title>nothing</title></svg>', 10, 10).error).toBeTruthy()
    expect(cleanSvg('<svg><script>alert(1)</script></svg>', 10, 10).error).toBeTruthy()
    expect(cleanSvg('', 10, 10).error).toBeTruthy()
    expect(cleanSvg(`<svg><path d="${'M0 0 '.repeat(90000)}"/></svg>`, 10, 10).error).toContain('400 KB')
  })

  it('gives the proper case to element and attribute names', () => {
    expect(clean('<SVG><LINEARGRADIENT ID="a"><STOP OFFSET="1"/></LINEARGRADIENT><RECT WIDTH="2" HEIGHT="2"/></SVG>')).toContain(
      '<linearGradient id="a"><stop offset="1"/></linearGradient><rect width="2" height="2"/>',
    )
  })

  it("puts the layer's prefix on every id and local url, never twice", () => {
    const once = clean('<linearGradient id="g"/><rect width="1" height="1" fill="url(#g)"/>', 'p7-')
    expect(once).toContain('id="p7-g"')
    expect(once).toContain('fill="url(#p7-g)"')
    const again = clean(once, 'p9-')
    expect(again).toContain('id="p9-g"')
    expect(again).toContain('url(#p9-g)')
  })

  it('makes a sized picture address for thumbnails', () => {
    const url = svgDataUrl(clean('<circle r="1"/>'), 800, 600)
    expect(url.startsWith('data:image/svg+xml;charset=utf-8,')).toBe(true)
    expect(decodeURIComponent(url)).toContain('<svg width="800" height="600" xmlns=')
  })
})
