import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

// Kiyo's room of Start-menu apps keeps its id `apps` (saved tab order, room switches and the Rust room list
// all use it) but is labelled DESKTOP, so the name APPS can belong to the room launcher (id `launcher`).
describe('the DESKTOP room', () => {
  const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')
  const app = read('./App.vue')
  const settings = read('./views/settings/RoomsPane.vue')
  const rust = read('../src-tauri/src/settings.rs')

  it('is called DESKTOP in the top bar and in Settings', () => {
    expect(app).toContain("{ id: 'apps', label: 'DESKTOP' }")
    expect(settings).toContain("{ id: 'apps', label: 'DESKTOP' }")
  })
  it('is no longer called APPS anywhere a room is labelled', () => {
    expect(app).not.toContain("label: 'APPS'")
    expect(settings).not.toContain("label: 'APPS'")
  })
  it('keeps its id everywhere the code depends on it', () => {
    expect(app).toContain("v-if=\"active === 'apps'\"")
    expect(app).toMatch(/BUILT = new Set\(\[[^\]]*'apps'/)
    expect(rust).toMatch(/pub const ROOMS: \[&str; \d+\] = \[[^\]]*"apps"/)
  })
})
