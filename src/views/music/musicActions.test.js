import { describe, expect, it } from 'vitest'
import {
  ACTIONS,
  DIAL_DEFAULTS,
  MODELS,
  PAID,
  STEPS,
  defaultSections,
  isPaid,
  lyricSections,
  newState,
  paidWord,
  parseTiming,
  pathsIn,
  peaksOf,
  selectionForHer,
  timingSections,
  withTakeDuration,
  withTiming,
  wordLines,
  record,
  runAction,
  sectionsOf,
  seedKind,
  setSectionLyrics,
  stateForHer,
  stemGain,
  undoLast,
  withDuration,
  withTakes,
  wordsBetween,
} from './musicActions.js'

const song = (over = {}) => ({
  ...withDuration(newState(), 200),
  song: { album: 'first-rain', track: 1, title: 'First Rain' },
  takes: [
    { take: 1, path: 'C:\\m\\01-v1.mp3', approved: false },
    { take: 2, path: 'C:\\m\\01-v2.mp3', approved: true },
  ],
  chosenTake: 2,
  stems: [
    { name: 'vocals', path: 'C:\\s\\vocals.mp3', muted: false, volume: 1, solo: false },
    { name: 'drums', path: 'C:\\s\\drums.mp3', muted: false, volume: 1, solo: false },
  ],
  ...over,
})

describe('the registry', () => {
  it('names every page action of the plan', () => {
    for (const name of ['step.set', 'section.select', 'section.clear', 'dial.set', 'draft.set', 'lyrics.set', 'take.choose', 'stem.mute', 'stem.volume', 'stem.solo', 'loop.lay', 'play', 'pause', 'seek', 'seed.add', 'seed.remove']) {
      expect(ACTIONS[name], name).toBeTruthy()
      expect(typeof ACTIONS[name].check).toBe('function')
      expect(typeof ACTIONS[name].apply).toBe('function')
      expect(typeof ACTIONS[name].label).toBe('function')
    }
  })

  it('keeps paid actions out of the page registry', () => {
    for (const name of ['sing', 'resing', 'extend', 'cover', 'stems', 'sound', 'timing', 'wav', 'cover_art', 'video', 'style_expand']) {
      expect(isPaid(name)).toBe(true)
      expect(ACTIONS[name]).toBeUndefined()
    }
    expect(() => runAction(song(), 'sing', {})).toThrow('paid actions go through the room')
  })

  it('refuses an unknown name and values that are not an object', () => {
    expect(() => runAction(song(), 'explode', {})).toThrow('no action named explode')
    expect(() => runAction(song(), 'seek', [1])).toThrow('must be an object')
  })

  it('keeps the model list as a constant', () => {
    expect(MODELS).toEqual(['V4', 'V4_5', 'V4_5PLUS', 'V5'])
  })
})

describe('step and song', () => {
  it('sets each of the six steps and refuses others', () => {
    for (const step of STEPS) expect(runAction(song(), 'step.set', { step }).state.step).toBe(step)
    expect(() => runAction(song(), 'step.set', { step: 'mix' })).toThrow('step must be one of')
  })

  it('opens a song at FIX with a clean slate and its words in the draft', () => {
    const s = { ...song(), selection: { from: 1, to: 5 } }
    const out = runAction(s, 'song.open', { album: 'night-sky', track: 2, title: 'Night', lyrics: '[Verse]\nhi', style: 'pop' })
    expect(out.state.song).toEqual({ album: 'night-sky', track: 2, title: 'Night' })
    expect(out.state.step).toBe('fix')
    expect(out.state.selection).toBeNull()
    expect(out.state.stems).toEqual([])
    expect(out.state.draft.lyrics).toBe('[Verse]\nhi')
    expect(() => runAction(s, 'song.open', { track: 1 })).toThrow('album is needed')
  })

  it('starts a new song at SEED and keeps the dials', () => {
    const s = runAction(song(), 'dial.set', { name: 'weirdness', value: 0.9 }).state
    const out = runAction({ ...s, credits: 50 }, 'song.new').state
    expect(out.song.album).toBe('')
    expect(out.step).toBe('seed')
    expect(out.takes).toEqual([])
    expect(out.dials.weirdness).toBe(0.9)
    expect(out.credits).toBe(50)
  })
})

describe('sections', () => {
  it('selects inside the song and clamps the ends', () => {
    const out = runAction(song(), 'section.select', { from: -4, to: 260 })
    expect(out.state.selection).toEqual({ from: 0, to: 200 })
    expect(out.touched).toBe('selection')
    expect(out.label).toBe('Selected 0:00-3:20')
  })

  it('refuses a backwards or too short selection', () => {
    expect(() => runAction(song(), 'section.select', { from: 50, to: 40 })).toThrow('before')
    expect(() => runAction(song(), 'section.select', { from: 50, to: 50.5 })).toThrow('at least one second')
    expect(() => runAction(song(), 'section.select', { from: 'a', to: 3 })).toThrow('must be a number')
  })

  it('clears the selection', () => {
    const s = runAction(song(), 'section.select', { from: 10, to: 20 }).state
    expect(runAction(s, 'section.clear').state.selection).toBeNull()
  })

  it('cuts a song into quarters until she marks its sections', () => {
    expect(defaultSections(200).map((s) => s.name)).toEqual(['intro', 'verse', 'chorus', 'bridge'])
    expect(defaultSections(200)[3]).toEqual({ name: 'bridge', from: 150, to: 200 })
    expect(defaultSections(0)).toEqual([])
    const marked = runAction(song(), 'sections.set', { sections: [{ name: 'chorus', from: 60, to: 90 }, { name: 'intro', from: 0, to: 12 }] }).state
    expect(sectionsOf(marked).map((s) => s.name)).toEqual(['intro', 'chorus'])
  })
})

describe('dials', () => {
  it('clamps the numbers to the API ranges', () => {
    const set = (name, value) => runAction(song(), 'dial.set', { name, value }).state.dials[name]
    expect(set('length', 5)).toBe(10)
    expect(set('length', 999)).toBe(360)
    expect(set('length', '200.6')).toBe(201)
    expect(set('styleWeight', 1.7)).toBe(1)
    expect(set('weirdness', -1)).toBe(0)
    expect(set('audioWeight', 0.333)).toBe(0.33)
    expect(set('variety', 9)).toBe(4)
    expect(set('variety', 2.4)).toBe(2)
  })

  it('takes the model, voice and persona only from their lists', () => {
    expect(runAction(song(), 'dial.set', { name: 'model', value: 'V4_5PLUS' }).state.dials.model).toBe('V4_5PLUS')
    expect(() => runAction(song(), 'dial.set', { name: 'model', value: 'V9' })).toThrow('model must be one of')
    expect(runAction(song(), 'dial.set', { name: 'vocalGender', value: 'any' }).state.dials.vocalGender).toBe('any')
    expect(() => runAction(song(), 'dial.set', { name: 'vocalGender', value: 'x' })).toThrow()
    expect(runAction(song(), 'dial.set', { name: 'persona', value: '  p-1 ' }).state.dials.persona).toBe('p-1')
    expect(() => runAction(song(), 'dial.set', { name: 'volume', value: 1 })).toThrow('dial must be one of')
  })

  it('lights the dial it turned', () => {
    expect(runAction(song(), 'dial.set', { name: 'weirdness', value: 0.8 }).touched).toBe('dial:weirdness')
  })
})

describe('draft and lyrics', () => {
  it('sets only the parts given and cuts the title at 80', () => {
    const out = runAction(song(), 'draft.set', { title: 'x'.repeat(100), style: 'city pop' })
    expect(out.state.draft.title).toHaveLength(80)
    expect(out.state.draft.style).toBe('city pop')
    expect(out.state.draft.lyrics).toBe('')
    expect(() => runAction(song(), 'draft.set', {})).toThrow()
  })

  it('reads and replaces a section of the lyrics', () => {
    const words = '[Verse 1]\nrain falls\nslowly\n\n[Chorus]\nunder one umbrella'
    expect(lyricSections(words)).toEqual([
      { section: 'Verse 1', text: 'rain falls\nslowly' },
      { section: 'Chorus', text: 'under one umbrella' },
    ])
    expect(setSectionLyrics(words, 'chorus', 'two hearts')).toBe('[Verse 1]\nrain falls\nslowly\n\n[Chorus]\ntwo hearts')
    expect(setSectionLyrics(words, 'Bridge', 'la la')).toContain('[Bridge]\nla la')
    const s = runAction({ ...song(), draft: { ...song().draft, lyrics: words } }, 'lyrics.set', { section: '[Chorus]', text: 'new' }).state
    expect(lyricSections(s.draft.lyrics)[1]).toEqual({ section: 'Chorus', text: 'new' })
  })

  it('finds the words sung inside a part', () => {
    const words = '[Verse]\naaaa\nbbbb\ncccc\ndddd'
    expect(wordsBetween(words, 100, 0, 100)).toBe('aaaa\nbbbb\ncccc\ndddd')
    expect(wordsBetween(words, 100, 0, 1)).toBe('')
    expect(wordsBetween(words, 100, 0, 100, [10, 20, 30, 40]).split('\n')).toHaveLength(4)
    expect(wordsBetween(words, 100, 15, 35, [10, 20, 30, 40])).toBe('bbbb\ncccc')
  })
})

describe('takes, stems and loops', () => {
  it('chooses only a take the shelf holds', () => {
    expect(runAction(song(), 'take.choose', { take: 1 }).state.chosenTake).toBe(1)
    expect(() => runAction(song(), 'take.choose', { take: 3 })).toThrow('no take 3')
  })

  it('mutes, solos and sets a stem volume within 0..1.5', () => {
    let s = runAction(song(), 'stem.mute', { stem: 'drums', on: true }).state
    expect(s.stems[1].muted).toBe(true)
    s = runAction(s, 'stem.volume', { stem: 'vocals', value: 3 }).state
    expect(s.stems[0].volume).toBe(1.5)
    s = runAction(s, 'stem.solo', { stem: 'vocals', on: 'true' }).state
    expect(s.stems[0].solo).toBe(true)
    expect(() => runAction(s, 'stem.mute', { stem: 'bass', on: true })).toThrow('no stem named bass')
    expect(() => runAction(s, 'stem.solo', { stem: 'vocals', on: 'maybe' })).toThrow('true or false')
  })

  it('plays each stem at its gain: mute, solo and karaoke', () => {
    const stems = song().stems
    expect(stemGain(stems[0], stems)).toBe(1)
    expect(stemGain({ ...stems[0], muted: true }, stems)).toBe(0)
    const soloed = [{ ...stems[0], solo: true }, stems[1]]
    expect(stemGain(soloed[1], soloed)).toBe(0)
    expect(stemGain(soloed[0], soloed)).toBe(1)
    expect(stemGain(stems[0], stems, true)).toBe(0)
    expect(stemGain(stems[1], stems, true)).toBe(1)
  })

  it('lays the stems the room brought home and keeps their mix by name', () => {
    const s = runAction(song(), 'stem.volume', { stem: 'drums', value: 0.4 }).state
    const out = runAction(s, 'stems.set', { stems: [{ name: 'drums', path: 'C:\\d.mp3' }, { name: 'bass', path: 'C:\\b.mp3' }] }).state
    expect(out.stems).toEqual([
      { name: 'drums', path: 'C:\\d.mp3', muted: false, volume: 0.4, solo: false },
      { name: 'bass', path: 'C:\\b.mp3', muted: false, volume: 1, solo: false },
    ])
    expect(() => runAction(s, 'stems.set', { stems: [{ name: 'a', path: 'x' }, { name: 'a', path: 'y' }] })).toThrow('Two stems')
  })

  it('lays and lifts a loop under the intro or outro', () => {
    const s = runAction(song(), 'loop.lay', { part: 'intro', file: 'C:\\loops\\rain.mp3' }).state
    expect(s.loops.intro).toBe('C:\\loops\\rain.mp3')
    expect(runAction(s, 'loop.lay', { part: 'intro', file: '' }).state.loops.intro).toBe('')
    expect(() => runAction(s, 'loop.lay', { part: 'middle', file: 'x' })).toThrow('part must be one of')
  })
})

describe('the transport', () => {
  it('plays, pauses and seeks inside the song', () => {
    let s = runAction(song(), 'play', { at: 300 }).state
    expect(s.playing).toBe(true)
    expect(s.at).toBe(200)
    s = runAction(s, 'pause').state
    expect(s.playing).toBe(false)
    expect(runAction(s, 'seek', { at: 42.123 }).state.at).toBe(42.12)
    expect(() => runAction(s, 'seek', {})).toThrow('at must be a number')
  })
})

describe('seeds', () => {
  it('knows a seed by its ending and refuses others', () => {
    expect(seedKind('C:\\a\\b.PNG')).toBe('picture')
    expect(seedKind('x.mid')).toBe('midi')
    expect(seedKind('x.mp4')).toBe('video')
    expect(seedKind('x.flac')).toBe('audio')
    expect(seedKind('x.txt')).toBeNull()
    expect(() => runAction(song(), 'seed.add', { path: 'C:\\notes.txt' })).toThrow('not a picture')
  })

  it('adds once, keeps the API limits and removes', () => {
    let s = runAction(song(), 'seed.add', { path: 'C:\\p\\1.png' }).state
    s = runAction(s, 'seed.add', { path: 'C:\\p\\1.png' }).state
    expect(s.seeds).toEqual([{ kind: 'picture', path: 'C:\\p\\1.png' }])
    s = runAction(s, 'seed.add', { kind: 'video', path: 'C:\\v.mp4' }).state
    expect(() => runAction(s, 'seed.add', { path: 'C:\\w.mp4' })).toThrow('At most 1 video')
    s = runAction(s, 'seed.remove', { path: 'C:\\p\\1.png' }).state
    expect(s.seeds.map((x) => x.kind)).toEqual(['video'])
    expect(() => runAction(s, 'seed.remove', { path: 'C:\\nope.png' })).toThrow('not in the list')
  })

  it('reads paths out of pasted text', () => {
    expect(pathsIn('"C:\\My Pictures\\a.png" D:\\b.mp3')).toEqual(['C:\\My Pictures\\a.png', 'D:\\b.mp3'])
    expect(pathsIn('hello')).toEqual([])
  })
})

describe('undo', () => {
  it('folds a run of changes in one step by one hand into one entry', () => {
    let s = song()
    let history = []
    for (const value of [0.6, 0.7, 0.8]) {
      const before = s
      const out = runAction(s, 'dial.set', { name: 'weirdness', value })
      history = record(history, { group: out.group, by: 'member', label: out.label, before })
      s = out.state
    }
    expect(history).toHaveLength(1)
    const back = undoLast(history, s)
    expect(back.state.dials.weirdness).toBe(DIAL_DEFAULTS.weirdness)
    expect(back.history).toEqual([])
    expect(undoLast(back.history, back.state)).toBeNull()
  })

  it('keeps her change as her own entry, and never undoes the step or the transport', () => {
    let s = song()
    let history = []
    const mine = runAction(s, 'stem.mute', { stem: 'drums', on: true })
    history = record(history, { group: mine.group, by: 'member', label: mine.label, before: s })
    s = mine.state
    const hers = runAction(s, 'stem.volume', { stem: 'vocals', value: 0.5 })
    history = record(history, { group: hers.group, by: 'companion', label: hers.label, before: s })
    s = hers.state
    expect(history.map((h) => h.by)).toEqual(['member', 'companion'])
    const step = runAction(s, 'step.set', { step: 'finish' })
    expect(record(history, { group: step.group, by: 'member', label: step.label, before: s })).toBe(history)
    s = step.state
    const back = undoLast(history, s)
    expect(back.entry.by).toBe('companion')
    expect(back.state.stems[0].volume).toBe(1)
    expect(back.state.stems[1].muted).toBe(true)
    expect(back.state.step).toBe('finish')
  })
})

describe('live facts', () => {
  it('chooses the approved take, else the newest, and keeps his choice', () => {
    const s = withTakes(newState(), [{ take: 1, path: 'a' }, { take: 2, path: 'b', approved: true }, { take: 3, path: 'c' }])
    expect(s.chosenTake).toBe(2)
    expect(withTakes(newState(), [{ take: 1, path: 'a' }, { take: 2, path: 'b' }]).chosenTake).toBe(2)
    const picked = { ...s, chosenTake: 3 }
    expect(withTakes(picked, [{ take: 1, path: 'a' }, { take: 2, path: 'b', approved: true }, { take: 3, path: 'c' }])).toBe(picked)
  })
})

describe('paid words', () => {
  it('types one compact line with the open song riding along', () => {
    const s = song()
    const line = paidWord('resing', { from: 40, to: 60, lyrics: 'two hearts\nunder rain' }, s)
    expect(line).toBe('studio resing {"from":40,"to":60,"lyrics":"two hearts\\nunder rain","album":"first-rain","track":1,"take":2}')
    expect(line.includes('\n')).toBe(false)
  })

  it('keeps a re-sung part between 10 s and half the song', () => {
    expect(() => paidWord('resing', { from: 40, to: 45, lyrics: 'x' }, song())).toThrow('at least 10 seconds')
    expect(() => paidWord('resing', { from: 0, to: 120, lyrics: 'x' }, song())).toThrow('at most half')
    expect(() => paidWord('resing', { from: 0, to: 20 }, song())).toThrow('lyrics is needed')
  })

  it('checks every other paid action', () => {
    expect(paidWord('extend', { at: 150 }, song())).toBe('studio extend {"at":150,"album":"first-rain","track":1,"take":2}')
    expect(() => paidWord('extend', { at: 250 }, song())).toThrow('inside the song')
    expect(paidWord('stems', { mode: 12 })).toBe('studio stems {"mode":12}')
    expect(() => paidWord('stems', { mode: 4 })).toThrow('2 or 12')
    expect(paidWord('sound', { prompt: 'soft rain', key: 'F# minor', tempo: 900 })).toBe('studio sound {"prompt":"soft rain","key":"F# minor","tempo":300,"loop":true}')
    expect(() => paidWord('sound', { prompt: 'x', key: 'H' })).toThrow('key looks like')
    expect(paidWord('cover', { style: 'acoustic' })).toBe('studio cover {"style":"acoustic"}')
    expect(paidWord('style_expand', { style: 'city pop' }, song())).toBe('studio style_expand {"style":"city pop"}')
    for (const name of ['timing', 'wav', 'cover_art', 'video']) expect(paidWord(name, {})).toBe(`studio ${name} {}`)
    expect(() => paidWord('play', {})).toThrow('not a paid action')
  })

  it('sings from the draft and the dials, cleaned', () => {
    const line = paidWord('sing', { title: 'First Rain', lyrics: '[Verse]\nrain', style: 'pop', length: 999, model: 'V5', seeds: [{ kind: 'picture', path: 'C:\\a.png' }] })
    expect(JSON.parse(line.slice('studio sing '.length))).toEqual({
      title: 'First Rain',
      style: 'pop',
      lyrics: '[Verse]\nrain',
      negative: '',
      length: 360,
      model: 'V5',
      seeds: [{ kind: 'picture', path: 'C:\\a.png' }],
    })
    expect(() => paidWord('sing', { title: 'x' })).toThrow('lyrics or a style')
  })

  it('has a check and a label for each paid action', () => {
    for (const name of Object.keys(PAID)) {
      expect(typeof PAID[name].check).toBe('function')
      expect(typeof PAID[name].label).toBe('function')
    }
  })
})

describe('what she reads', () => {
  it('holds the song, step, sections, lyrics, dials, stems, takes, seeds, credits and selection', () => {
    const s = { ...song(), selection: { from: 10, to: 30 }, credits: 420, draft: { title: 'F', style: '', lyrics: '[Chorus]\nla', negative: '' } }
    const her = stateForHer(s)
    expect(her.song).toEqual({ album: 'first-rain', track: 1, title: 'First Rain' })
    expect(her.step).toBe('seed')
    expect(her.sections).toHaveLength(4)
    expect(her.lyrics).toEqual([{ section: 'Chorus', text: 'la' }])
    expect(her.dials).toEqual(DIAL_DEFAULTS)
    expect(her.stems[0]).toEqual({ name: 'vocals', muted: false, volume: 1, solo: false })
    expect(her.takes).toEqual([
      { take: 1, file: 'C:\\m\\01-v1.mp3', audioId: '', duration: 0, approved: false },
      { take: 2, file: 'C:\\m\\01-v2.mp3', audioId: '', duration: 0, approved: true },
    ])
    expect(her.chosenTake).toBe(2)
    expect(her.credits).toBe(420)
    expect(her.selection).toEqual({ from: 10, to: 30, section: 'intro', lyrics: 'la' })
    expect(her.actions).toContain('stem.volume')
    expect(her.paid).toContain('resing')
    expect(JSON.parse(JSON.stringify(her))).toEqual(her)
  })
})

describe('the waveform', () => {
  it('takes the loudest sample of each slice', () => {
    const samples = new Float32Array([0.1, -0.9, 0.2, 0.3, -0.4, 0.05, 0, 0])
    expect(peaksOf(samples, 4).map((n) => Math.round(n * 100) / 100)).toEqual([0.9, 0.3, 0.4, 0])
    expect(peaksOf(null, 3)).toEqual([0, 0, 0])
  })
})

describe('results from her door', () => {
  it('adds a take with its file and audioId, and chooses it for the timeline', () => {
    const out = runAction(song(), 'take.add', { take: 3, file: 'C:\\m\\01-v3.mp3', audioId: 'a-3' })
    expect(out.state.takes.map((k) => k.take)).toEqual([1, 2, 3])
    expect(out.state.takes[2]).toMatchObject({ take: 3, path: 'C:\\m\\01-v3.mp3', audioId: 'a-3', added: true })
    expect(out.state.chosenTake).toBe(3)
    expect(out.touched).toBe('take')
    expect(out.group).toBeNull()
    expect(() => runAction(song(), 'take.add', { take: 0, file: 'x' })).toThrow('take starts at 1')
    expect(() => runAction(song(), 'take.add', { take: 2 })).toThrow('file is needed')
  })

  it('updates a take it already holds and keeps its approval', () => {
    const out = runAction(song(), 'take.add', { take: 2, file: 'C:\\m\\01-v2b.mp3' })
    expect(out.state.takes).toHaveLength(2)
    expect(out.state.takes[1]).toMatchObject({ path: 'C:\\m\\01-v2b.mp3', approved: true })
  })

  it('keeps a door-added take until the shelf shows it, and its audioId after', () => {
    const added = runAction(song(), 'take.add', { take: 3, file: 'C:\\m\\01-v3.mp3', audioId: 'a-3' }).state
    const shelfOld = [{ take: 1, path: 'C:\\m\\01-v1.mp3' }, { take: 2, path: 'C:\\m\\01-v2.mp3', approved: true }]
    expect(withTakes(added, shelfOld).takes.map((k) => k.take)).toEqual([1, 2, 3])
    const shelfNew = [...shelfOld, { take: 3, path: 'C:\\m\\01-v3.mp3' }]
    const synced = withTakes(added, shelfNew)
    expect(synced.takes[2]).toMatchObject({ take: 3, audioId: 'a-3', added: false })
    expect(synced.chosenTake).toBe(3)
  })

  it('knows each take length once read', () => {
    const s = withTakeDuration(song(), 2, 187.456)
    expect(s.takes[1].duration).toBe(187.46)
    expect(withTakeDuration(s, 2, 187.456)).toBe(s)
    expect(stateForHer(s).takes[1].duration).toBe(187.46)
  })

  it('lays stems given as file as well as path', () => {
    const out = runAction(song(), 'stems.set', { stems: [{ name: 'vocals', file: 'C:\\s\\v.mp3' }, { name: 'music', path: 'C:\\s\\m.mp3' }] })
    expect(out.state.stems.map((s) => s.path)).toEqual(['C:\\s\\v.mp3', 'C:\\s\\m.mp3'])
    expect(() => runAction(song(), 'stems.set', { stems: [{ name: 'x' }] })).toThrow('file is needed')
  })

  it('takes a timing file for the chosen take', () => {
    const out = runAction(song(), 'timing.set', { file: 'C:\\m\\audio\\01-v2.timing.json' })
    expect(out.state.timing).toEqual({ take: 2, file: 'C:\\m\\audio\\01-v2.timing.json' })
    expect(() => runAction(song(), 'timing.set', { file: 'C:\\x.txt' })).toThrow('ends in .json')
  })

  it('lists exports once each and refuses other kinds', () => {
    let s = runAction(song(), 'export.add', { kind: 'wav', file: 'C:\\e\\01.wav' }).state
    s = runAction(s, 'export.add', { kind: 'wav', file: 'C:\\e\\01.wav' }).state
    s = runAction(s, 'export.add', { kind: 'sound', file: 'C:\\e\\rain.mp3' }).state
    expect(s.exports).toEqual([{ kind: 'wav', file: 'C:\\e\\01.wav' }, { kind: 'sound', file: 'C:\\e\\rain.mp3' }])
    expect(stateForHer(s).exports).toHaveLength(2)
    expect(() => runAction(s, 'export.add', { kind: 'zip', file: 'x' })).toThrow('kind must be one of')
  })

  it('sets the credits meter', () => {
    expect(runAction(song(), 'credits.set', { value: '696' }).state.credits).toBe(696)
    expect(() => runAction(song(), 'credits.set', { value: -1 })).toThrow('below 0')
  })

  it('takes a style alone as a partial draft (style_expand answers so)', () => {
    const s = { ...song(), draft: { title: 'T', style: 'pop', lyrics: 'la', negative: '' } }
    expect(runAction(s, 'draft.set', { style: 'dreamy city pop, soft female vocal' }).state.draft).toEqual({
      title: 'T',
      style: 'dreamy city pop, soft female vocal',
      lyrics: 'la',
      negative: '',
    })
  })

  it('never undoes what came home', () => {
    let s = song()
    let history = []
    const knob = runAction(s, 'dial.set', { name: 'weirdness', value: 0.9 })
    history = record(history, { group: knob.group, by: 'member', label: knob.label, before: s })
    s = knob.state
    const results = [
      ['take.add', { take: 3, file: 'C:\\v3.mp3' }],
      ['export.add', { kind: 'cover', file: 'C:\\c.png' }],
      ['credits.set', { value: 5 }],
    ]
    for (const [name, values] of results) {
      const out = runAction(s, name, values)
      history = record(history, { group: out.group, by: 'companion', label: out.label, before: s })
      s = out.state
    }
    expect(history).toHaveLength(1)
    const back = undoLast(history, s).state
    expect(back.dials.weirdness).toBe(0.5)
    expect(back.takes).toHaveLength(3)
    expect(back.exports).toHaveLength(1)
    expect(back.credits).toBe(5)
  })
})

describe('timing files', () => {
  it('reads the new shape with lines and words', () => {
    const t = parseTiming({
      lines: [{ start: 12.4, text: 'rain falls' }, { start: 20, text: 'slowly' }],
      words: [{ word: 'rain', start: 12.4, end: 13 }, { word: 'falls', start: 13.1, end: 14 }, { word: 'slowly', start: 20.2 }],
    })
    expect(t.lines).toEqual([12.4, 20])
    expect(t.texts).toEqual(['rain falls', 'slowly'])
    expect(t.words[2]).toEqual({ word: 'slowly', start: 20.2, end: 20.2 })
  })

  it('reads the old shape, a bare list and text with a BOM', () => {
    expect(parseTiming({ lines: [12.4, 20] })).toEqual({ lines: [12.4, 20], texts: [null, null], words: [] })
    expect(parseTiming([1, 2]).lines).toEqual([1, 2])
    expect(parseTiming('\uFEFF{"lines":[3.5]}').lines).toEqual([3.5])
    expect(parseTiming('not json')).toBeNull()
    expect(parseTiming({})).toBeNull()
  })

  it('groups the words under their lines for the word-by-word glow', () => {
    const t = parseTiming({
      lines: [{ start: 10, text: 'a b' }, { start: 20, text: 'c' }],
      words: [{ word: 'a', start: 10, end: 11 }, { word: 'b', start: 11, end: 12 }, { word: 'c', start: 20, end: 21 }],
    })
    expect(wordLines(t).map((l) => l.words.map((w) => w.word))).toEqual([['a', 'b'], ['c']])
    expect(wordLines(parseTiming({ lines: [1, 2] }))).toEqual([])
    expect(wordLines(parseTiming({ words: [{ word: 'x', start: 1, end: 2 }] }))[0].words).toHaveLength(1)
  })

  it('cuts the song into its labelled sections from the timed lines', () => {
    const lyrics = '[Verse]\na\nb\n\n[Chorus]\nc\nd\n\n[Outro]\ne'
    expect(timingSections(lyrics, [8, 15, 40, 50, 90], 120)).toEqual([
      { name: 'Verse', from: 0, to: 40 },
      { name: 'Chorus', from: 40, to: 90 },
      { name: 'Outro', from: 90, to: 120 },
    ])
    expect(timingSections(lyrics, [8, 15], 120)).toEqual([])
    expect(timingSections('a\nb', [1, 2], 120)).toEqual([])
    const s = withTiming({ ...song(), draft: { ...song().draft, lyrics } }, parseTiming({ lines: [8, 15, 40, 50, 90] }))
    expect(sectionsOf(s).map((x) => x.name)).toEqual(['Verse', 'Chorus', 'Outro'])
    expect(stateForHer(s).sections).toHaveLength(3)
  })

  it('names the section and the words under a selection', () => {
    const lyrics = '[Verse]\na\nb\n\n[Chorus]\nc\nd\n\n[Outro]\ne'
    const s = withTiming({ ...song(), selection: { from: 40, to: 60 }, draft: { ...song().draft, lyrics } }, parseTiming({ lines: [8, 15, 40, 50, 90] }))
    expect(selectionForHer(s)).toEqual({ from: 40, to: 60, section: 'Chorus', lyrics: 'c\nd' })
    // across sections, mostly in none of them: no section is named
    expect(selectionForHer({ ...s, selection: { from: 20, to: 140 } }).section).toBeUndefined()
  })
})
