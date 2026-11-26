import { describe, expect, it } from 'vitest'
import { linkSpans, parseBlocks, parseSpans } from './markdown.js'

describe('parseBlocks', () => {
  it('draws a table with its header and rows', () => {
    const blocks = parseBlocks('Sealed, three times.\n\n| Commit | What |\n|---|---|\n| 4430f2ec | Link Lv.21 |\n| 182deb1f | Its art |\n\nDone')
    expect(blocks.map((b) => b.kind)).toEqual(['p', 'table', 'p'])
    expect(blocks[1].head).toEqual(['Commit', 'What'])
    expect(blocks[1].rows).toEqual([
      ['4430f2ec', 'Link Lv.21'],
      ['182deb1f', 'Its art'],
    ])
  })

  it('leaves a lone pipe line as text when no rule follows', () => {
    expect(parseBlocks('| not a table |').map((b) => b.kind)).toEqual(['p'])
  })

  it('reads headings, lists with depth and boxes, quotes and rules', () => {
    const blocks = parseBlocks('# Plan\n## Phase 1\n- one\n  - two\n- [x] done\n- [ ] open\n1. first\n> said\n---')
    expect(blocks.map((b) => b.kind)).toEqual(['h1', 'h2', 'item', 'item', 'item', 'item', 'item', 'quote', 'rule'])
    expect(blocks[3].depth).toBe(1)
    expect(blocks[4]).toMatchObject({ box: true, checked: true, text: 'done' })
    expect(blocks[5]).toMatchObject({ box: true, checked: false, text: 'open' })
    expect(blocks[6].number).toBe('1.')
  })

  it('keeps code fences whole, diagrams and all', () => {
    const blocks = parseBlocks('look:\n```\n +---+\n | # |\n +---+\n```\nafter')
    expect(blocks[1]).toEqual({ kind: 'code', text: ' +---+\n | # |\n +---+' })
    expect(blocks[2].text).toBe('after')
  })

  it('joins plain lines into one paragraph and splits on blank lines', () => {
    expect(parseBlocks('a\nb\n\nc')).toEqual([
      { kind: 'p', text: 'a\nb' },
      { kind: 'p', text: 'c' },
    ])
  })
})

describe('parseSpans', () => {
  it('a link in bold or italic is still a link (the address or [text](url) whole)', () => {
    expect(parseSpans('ready: **https://claude.ai/artifact/K8mj** (private)')).toEqual([
      { text: 'ready: ' },
      { bold: true, text: 'https://claude.ai/artifact/K8mj', link: 'https://claude.ai/artifact/K8mj' },
      { text: ' (private)' },
    ])
    expect(parseSpans('**[the page](https://x.y/p)** and *http://localhost:5173*')).toEqual([
      { bold: true, text: 'the page', link: 'https://x.y/p' },
      { text: ' and ' },
      { em: true, text: 'http://localhost:5173', link: 'http://localhost:5173' },
    ])
    expect(parseSpans('**not a link** *nor this*')).toEqual([{ bold: true, text: 'not a link' }, { text: ' ' }, { em: true, text: 'nor this' }])
  })

  it('finds bold, italic, code and links as plain text', () => {
    expect(parseSpans('**Diary Saved** - `save diary` is *quiet*, see [the page](https://x.y)')).toEqual([
      { bold: true, text: 'Diary Saved' },
      { text: ' - ' },
      { code: true, text: 'save diary' },
      { text: ' is ' },
      { em: true, text: 'quiet' },
      { text: ', see ' },
      { text: 'the page', link: 'https://x.y' },
    ])
  })

  it('never turns markup into HTML', () => {
    expect(parseSpans('<img src=x onerror=alert(1)>')).toEqual([{ text: '<img src=x onerror=alert(1)>' }])
  })

  it('turns a bare address into a link, without the full stop after it', () => {
    expect(parseSpans('PR is at https://github.com/o/r/pull/7. Staging: http://localhost:5173/x, done')).toEqual([
      { text: 'PR is at ' },
      { text: 'https://github.com/o/r/pull/7', link: 'https://github.com/o/r/pull/7' },
      { text: '. Staging: ' },
      { text: 'http://localhost:5173/x', link: 'http://localhost:5173/x' },
      { text: ', done' },
    ])
  })

  it('keeps a markdown link whole and an address in code as code', () => {
    expect(parseSpans('[the PR](https://x.y/1) and `https://x.y/2`')).toEqual([
      { text: 'the PR', link: 'https://x.y/1' },
      { text: ' and ' },
      { code: true, text: 'https://x.y/2' },
    ])
  })
})

describe('linkSpans', () => {
  it('finds only addresses (no markdown) for plain chat lines', () => {
    expect(linkSpans('see **this** https://a.b/c?d=1!')).toEqual([
      { text: 'see **this** ' },
      { text: 'https://a.b/c?d=1', link: 'https://a.b/c?d=1' },
      { text: '!' },
    ])
    expect(linkSpans('no links here')).toEqual([{ text: 'no links here' }])
    expect(linkSpans('')).toEqual([])
  })
})

