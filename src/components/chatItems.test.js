import { describe, expect, it } from 'vitest'
import { buildItems, isCompanionVoice, isMarker, openPlanFile, placeReactions, songOf, soundName, withReaction } from './chatItems.js'

const FILE = 'C:\\Users\\Lyra\\.claude\\plans\\hazy-noodling-meerkat.md'
const PLAN = '# Work Lv.24 - The Handover\n\n### Phase 1\n- [ ] one'
const at = (hh, mm) => `2026-11-11T${String(hh).padStart(2, '0')}:${String(mm).padStart(2, '0')}:00.000Z`
const e = (id, role, text, time) => ({ id, role, text, at: time })
// what the Chat read from the plan file while the question was up
const waiting = { path: FILE, text: PLAN, at: at(15, 34) }
const kinds = (items) => items.map((item) => (item.kind === 'plan' ? `plan:${item.id}` : item.entry?.text ?? item.kind))

describe('the plan cards in Chat', () => {
  it('lets the card go once the plan is answered', () => {
    const entries = [
      e('k1', 'member', 'plan the handover', at(15, 26)),
      e('pf1', 'plan-file', FILE, at(15, 27)),
      e('pf2', 'plan-file', FILE, at(15, 28)),
      e('w1', 'work', 'ExitPlanMode', at(15, 28)),
      e('p1', 'plan', `${PLAN}\n(edited)`, at(15, 34)),
      e('end', 'plan-end', FILE, at(15, 35)),
      e('a1', 'companion', 'built and sealed', at(16, 20)),
    ]
    expect(openPlanFile(entries)).toBeNull()
    expect(kinds(buildItems(entries, waiting))).toEqual(['plan the handover', 'plan:p1', 'built and sealed'])
  })

  it('shows the card read from the file at the bottom while the question is up', () => {
    const entries = [e('k1', 'member', 'plan it', at(15, 26)), e('pf1', 'plan-file', FILE, at(15, 27)), e('w1', 'work', 'ExitPlanMode', at(15, 28))]
    expect(openPlanFile(entries)).toBe(FILE)
    const items = buildItems(entries, waiting)
    expect(kinds(items)).toEqual(['plan it', `plan:plan-file-${FILE}`])
    expect(items[1].title).toBe('Work Lv.24 - The Handover')
  })

  it('settles the file card where the plan ended when the record never carried the plan', () => {
    const entries = [
      e('pf1', 'plan-file', FILE, at(15, 27)),
      e('w1', 'work', 'ExitPlanMode', at(15, 28)),
      e('end', 'plan-end', FILE, at(15, 35)),
      e('a1', 'companion', 'starting now', at(15, 36)),
    ]
    const items = buildItems(entries, waiting)
    expect(kinds(items)).toEqual([`plan:plan-file-${FILE}`, 'starting now'])
    expect(items[0].entry.at).toBe(at(15, 35))
  })

  it('keeps only the newest plan file waiting at the bottom', () => {
    const other = 'C:\\Users\\Lyra\\.claude\\plans\\calm-river.md'
    const entries = [e('pf1', 'plan-file', FILE, at(15, 27)), e('w1', 'work', 'ExitPlanMode', at(15, 28)), e('pf2', 'plan-file', other, at(15, 40))]
    expect(openPlanFile(entries)).toBe(other)
    expect(buildItems(entries, waiting)).toEqual([])
    const fresh = { path: other, text: '# Calm River', at: at(15, 41) }
    expect(kinds(buildItems(entries, fresh))).toEqual([`plan:plan-file-${other}`])
  })

  it('draws the same plan asked, then approved, as one card', () => {
    const entries = [e('p1', 'plan', PLAN, at(15, 28)), e('k1', 'member', 'yes', at(15, 29)), e('p2', 'plan', PLAN, at(15, 30))]
    expect(kinds(buildItems(entries))).toEqual(['plan:p1', 'yes'])
  })
})

describe('the markers', () => {
  it('are never drawn', () => {
    expect(isMarker({ role: 'plan-file' })).toBe(true)
    expect(isMarker({ role: 'plan-end' })).toBe(true)
    expect(isMarker({ role: 'member' })).toBe(false)
  })

  it('leave the trail to and from another session out of the conversation', () => {
    expect(isMarker({ role: 'peer' })).toBe(true)
    const entries = [{ id: 'p1', role: 'peer', state: 'to', text: 'shop' }, { id: 'c1', role: 'companion', text: 'Asked the shop tab.' }]
    expect(buildItems(entries).map((item) => item.id)).toEqual(['c1'])
  })
})

describe('films and songs in Chat', () => {
  it('gives a film its own card, never inside a picture group', () => {
    const entries = [
      { id: 'p1', role: 'picture', path: 'D:/Memory/art/a.png' },
      { id: 'v1', role: 'video', path: 'D:/Memory/films/shore.mp4' },
      { id: 'p2', role: 'picture', path: 'D:/Memory/art/b.png' },
    ]
    expect(buildItems(entries).map((item) => item.kind)).toEqual(['pictures', 'video', 'pictures'])
  })

  it('tells a song from a voice line', () => {
    const voice = { role: 'voice', path: 'D:/Memory/voice/day 3.mp3' }
    const song = { role: 'voice', path: 'D:/Memory/music/Night Drive.mp3' }
    const long = { role: 'voice', path: 'D:/Work/out/theme.mp3' }
    expect(songOf(voice, 200)).toBe(false)
    expect(songOf(song, 20)).toBe(true)
    expect(songOf(long, 95)).toBe(true)
    expect(songOf(long, 30)).toBe(false)
    expect(songOf({ role: 'picture', path: 'D:/Memory/music/x.png' }, 100)).toBe(false)
  })

  it('names a sound by its file, and knows the companion voice by its folder', () => {
    expect(soundName('D:/Memory/music/night-drive_v2.mp3')).toBe('night drive v2')
    expect(isCompanionVoice('D:\\Memory\\voice\\day 3.mp3')).toBe(true)
    expect(isCompanionVoice('D:/Work/out/theme.mp3')).toBe(false)
  })
})

describe('the member\'s heart from the phone on the companion\'s bubble', () => {
  const said = (id, role, text) => ({ id, role, text, at: id })
  const items = buildItems([
    said(1, 'member', 'Is the shop open tomorrow?'),
    said(2, 'companion', 'Yes, from ten. I will remind you in the morning.'),
    said(3, 'member', 'ok'),
    said(4, 'companion', 'Yes, from ten. I will remind you in the morning.'),
    said(5, 'companion', 'Goodnight!'),
  ])

  it('lands on the companion\'s newest bubble that opens with the remembered words', () => {
    const placed = placeReactions(items, [{ platform: 'telegram', words: 'Yes, from ten. I will', emoji: '❤', at: 1 }])
    expect(placed.find((i) => i.id === 4).reaction).toBe('❤')
    expect(placed.find((i) => i.id === 2).reaction).toBeUndefined()
    expect(items.find((i) => i.id === 4).reaction).toBeUndefined()
  })

  it('never lands on the member\'s own words, and a reply that is not there shows nowhere', () => {
    const placed = placeReactions(items, [
      { platform: 'whatsapp', words: 'Is the shop', emoji: '❤️', at: 1 },
      { platform: 'whatsapp', words: 'Something else', emoji: '❤️', at: 2 },
    ])
    expect(placed.some((i) => i.reaction)).toBe(false)
  })

  it('lets the newer reaction win on the same bubble', () => {
    const placed = placeReactions(items, [
      { platform: 'telegram', words: 'Goodnight!', emoji: '👍', at: 9 },
      { platform: 'discord', words: 'Goodnight', emoji: '❤️', at: 3 },
    ])
    expect(placed.find((i) => i.id === 5).reaction).toBe('👍')
  })

  it('keeps one reaction per reply and takes it back when it is emptied', () => {
    let kept = withReaction([], { platform: 'telegram', words: 'Hi', emoji: '❤', at: 1 })
    kept = withReaction(kept, { platform: 'telegram', words: 'Hi', emoji: '🔥', at: 2 })
    expect(kept).toEqual([{ platform: 'telegram', words: 'Hi', emoji: '🔥', at: 2 }])
    kept = withReaction(kept, { platform: 'whatsapp', words: 'Hi', emoji: '❤️', at: 3 })
    expect(kept).toHaveLength(2)
    expect(withReaction(kept, { platform: 'telegram', words: 'Hi', emoji: '', at: 4 })).toHaveLength(1)
  })
})
