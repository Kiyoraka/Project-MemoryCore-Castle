import { describe, expect, it } from 'vitest'
import { bestImage, companionSentence, deviceSize, frameWidth, iosAllowed, keepTail, plainLines, newPhoneName, osOf, parseFrame, phoneLine, rowMark, servedIn, stateLine, toDevice } from './mobile.js'

describe('Mobile setup', () => {
  const rows = [
    { id: 'sdk', label: 'Android SDK', ok: true, action: '' },
    { id: 'emulator', label: 'Emulator', ok: true, action: '' },
    { id: 'phone', label: 'A phone', ok: false, action: 'create' },
    { id: 'flutter', label: 'Flutter', ok: false, action: 'install' },
  ]

  it('marks each row the way the picture does', () => {
    expect(rows.map(rowMark)).toEqual(['ok', 'ok', 'create', 'missing'])
    expect(rowMark(null)).toBe('missing')
  })

  it('asks the companion only for what she must install', () => {
    const all = companionSentence(rows)
    expect(all).toContain('Flutter')
    expect(all).not.toContain('system image') // a phone is made here, not installed
    expect(companionSentence(rows, 'flutter')).toContain('Flutter')
    expect(companionSentence(rows, 'sdk')).toBe('')
    expect(companionSentence([{ id: 'sdk', ok: false, action: 'install' }, { id: 'java', ok: false, action: 'install' }])).toMatch(/Android SDK.*; a Java JDK/)
    expect(companionSentence([])).toBe('')
  })

  it('words the install for the member\'s own computer', () => {
    const missing = [{ id: 'flutter', ok: false, action: 'install' }]
    expect(companionSentence(missing, '', 'windows')).toContain('winget')
    expect(companionSentence(missing, '', 'mac')).toContain('Homebrew')
    expect(companionSentence(missing, '', 'linux')).toContain('package manager')
    const image = [{ id: 'phone', ok: false, action: 'install' }]
    expect(companionSentence(image, '', 'mac')).toContain('arm64-v8a')
    expect(companionSentence(image, '', 'windows')).not.toContain('arm64')
    expect(companionSentence([{ id: 'xcode', ok: false, action: 'install' }], '', 'mac')).toContain('Xcode')
  })

  it('tells the computer from its navigator', () => {
    expect(osOf({ platform: 'MacIntel' })).toBe('mac')
    expect(osOf({ platform: 'Win32', userAgent: 'Windows NT 10.0' })).toBe('windows')
    expect(osOf({ platform: 'Linux x86_64', userAgent: 'X11; Linux x86_64' })).toBe('linux')
    expect(osOf(undefined)).toBe('windows')
  })

  it('names a new phone and picks its image', () => {
    expect(newPhoneName([])).toBe('Castle_Phone')
    expect(newPhoneName([{ name: 'Castle_Phone' }, { name: 'Castle_Phone_2' }])).toBe('Castle_Phone_3')
    expect(bestImage(['system-images;android-35;google_apis;x86_64', 'system-images;android-35;google_apis_playstore_ps16k;x86_64'])).toContain('playstore')
    expect(bestImage(['system-images;android-34;google_apis;x86_64'])).toContain('android-34')
    expect(bestImage([])).toBe('')
  })

  it('describes a phone and gates the iOS side', () => {
    expect(phoneLine({ width: 1080, height: 2400, api: 35 })).toBe('1080 x 2400 · Android 35')
    expect(phoneLine({})).toBe('')
    expect(iosAllowed('flutter')).toBe(true)
    expect(iosAllowed('android')).toBe(false)
  })

  it('reads a frame from the phone', () => {
    const buf = new ArrayBuffer(8 + 2 * 1 * 4)
    const v = new DataView(buf)
    v.setUint32(0, 2, true)
    v.setUint32(4, 1, true)
    const f = parseFrame(buf)
    expect([f.width, f.height, f.pixels.length]).toEqual([2, 1, 8])
    expect(parseFrame(new ArrayBuffer(4))).toBe(null)
    expect(parseFrame(new ArrayBuffer(12))).toBe(null)
  })

  it('maps a click to the phone and keeps its orientation', () => {
    const phone = { width: 1080, height: 2400 }
    expect(deviceSize(phone, { width: 360, height: 800 })).toEqual({ width: 1080, height: 2400 })
    expect(deviceSize(phone, { width: 800, height: 360 })).toEqual({ width: 2400, height: 1080 })
    expect(toDevice(180, 400, { width: 360, height: 800 }, { width: 1080, height: 2400 })).toEqual({ x: 540, y: 1200 })
    expect(toDevice(-5, 900, { width: 360, height: 800 }, { width: 1080, height: 2400 })).toEqual({ x: 0, y: 2399 })
    expect(toDevice(1, 1, { width: 0, height: 0 }, phone)).toBe(null)
  })

  it('asks a sensible frame and says where the phone stands', () => {
    expect(frameWidth(300, 1.5)).toBe(450)
    expect(frameWidth(600, 2)).toBe(540)
    expect(frameWidth(50)).toBe(120)
    expect(stateLine(null)).toBe('The phone is off.')
    expect(stateLine({ state: 'booting', note: 'Booting…' })).toBe('Booting…')
  })

  it('keeps the run as plain lines', () => {
    const esc = String.fromCharCode(27)
    expect(plainLines(`${esc}[32mLaunching${esc}[0m lib/main.dart\r\nDone`)).toEqual(['Launching lib/main.dart', 'Done'])
    expect(plainLines('50%\r100%')).toEqual(['100%'])
    expect(keepTail(['Building'], '... done\nInstalled')).toEqual(['Building... done', 'Installed'])
    expect(keepTail([], 'a\nb\nc', 2)).toEqual(['b', 'c'])
    expect(servedIn('lib\\main.dart is being served at http://127.0.0.1:7357')).toBe(true)
    expect(servedIn('Compiling lib/main.dart for the Web...')).toBe(false)
  })
})
