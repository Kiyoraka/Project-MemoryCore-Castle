import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { PROVIDERS, drawsItself, offeredProviders, providerLabel, readsClaudeScreen, readsCodexAsk } from './providers.js'

describe('providers', () => {
  it('holds the same four ids, in the same order, as Rust', () => {
    const rust = readFileSync(new URL('../../src-tauri/src/settings.rs', import.meta.url), 'utf8')
    // the arms of `Cli::id`, up to the end of its match
    const arms = rust.split('pub const fn id(self)')[1].split('}')[0]
    const ids = [...arms.matchAll(/=> "(\w+)"/g)].map((m) => m[1])
    expect(ids).toEqual(['claude', 'codex', 'gemini', 'grok'])
    expect(PROVIDERS.map((p) => p.id)).toEqual(ids)
  })

  it('names each provider, and an unknown one as it is', () => {
    expect(PROVIDERS.map((p) => providerLabel(p.id))).toEqual(['Claude Code', 'Codex', 'Gemini', 'Grok'])
    expect(providerLabel('other')).toBe('other')
  })

  it('offers Claude Code and Codex, and keeps a current Gemini or Grok in the list', () => {
    expect(offeredProviders('claude').map((p) => p.id)).toEqual(['claude', 'codex'])
    expect(offeredProviders('grok').map((p) => p.id)).toEqual(['claude', 'codex', 'grok'])
  })

  it('gives a new provider the plain message box', () => {
    for (const cli of ['gemini', 'grok']) {
      expect(readsClaudeScreen(cli)).toBe(false)
      expect(readsCodexAsk(cli)).toBe(false)
      expect(drawsItself(cli)).toBe(false)
    }
    expect(readsClaudeScreen('claude')).toBe(true)
    expect(readsCodexAsk('codex') && drawsItself('codex')).toBe(true)
  })
})
