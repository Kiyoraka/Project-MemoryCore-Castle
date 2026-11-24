import './loader/loader.js'
import { createApp } from 'vue'
import App from './App.vue'
// a theme's widget skin (theme.json widgetSkin), on top of the glass base
import './themes/skins.css'
import { isMac } from './platform.js'

// Browser keys that would reload, print or search the page. Keys the terminal uses
// are handled there first, so only keys nothing else wanted are stopped here.
// On a Mac the same browser keys use ⌘ (meta) instead of Ctrl; Ctrl there belongs to the terminal.
const BLOCKED = isMac
  ? new Set(['F5', 'meta+r', 'meta+shift+r', 'meta+p', 'meta+f', 'meta+g', 'meta+shift+g', 'meta+u', 'meta+alt+u', 'F3', 'F7'])
  : new Set(['F5', 'ctrl+r', 'ctrl+shift+r', 'ctrl+p', 'ctrl+f', 'ctrl+g', 'ctrl+u', 'F3', 'F7'])

window.addEventListener('keydown', (event) => {
  if (event.defaultPrevented) return
  const name = event.key.length === 1 ? event.key.toLowerCase() : event.key
  const combo = isMac
    ? [event.metaKey && 'meta', event.altKey && 'alt', event.shiftKey && 'shift', name].filter(Boolean).join('+')
    : [event.ctrlKey && 'ctrl', event.shiftKey && 'shift', name].filter(Boolean).join('+')
  if (BLOCKED.has(combo) || BLOCKED.has(event.key)) event.preventDefault()
})

// A window on another screen is told what it is before the page loads (world.rs):
// { view: 'world', screen, lead, music, volume, config } or { view: 'identify', number }.
// Dev only: http://localhost:<port>/?world=3d/crystal-isle&camera=orbit draws one world in the
// browser, for tuning a world.json by eye (MAKE-IT-YOURS, "A world of your own").
const devWorld = import.meta.env.DEV ? new URLSearchParams(location.search).get('world') : null
const view = devWorld
  ? { view: 'world', screen: 0, lead: false, config: { world: devWorld, camera: new URLSearchParams(location.search).get('camera') || 'orbit', holdMin: 8, holdMax: 20, glide: 'mix', shots: ['follow', 'wide', 'close', 'orbit', 'crane'], seconds: 15, shuffle: true } }
  : window.__CASTLE_VIEW__
// Dev only: ?stage=classic|cinema|book|messenger|jrpg shows the STORY stage with a sample beat.
const devStage = import.meta.env.DEV ? new URLSearchParams(location.search).get('stage') : null
// Dev only: ?diary=all|letter|journal|notebook shows the DIARY room's reading pages with a sample entry.
const devDiary = import.meta.env.DEV ? new URLSearchParams(location.search).get('diary') : null
// Dev only: ?theme-room=1 shows the THEME room's showroom over the default theme (&theme=<id> for another).
const devThemeRoom = import.meta.env.DEV ? new URLSearchParams(location.search).get('theme-room') : null
if (devThemeRoom) {
  Promise.all([import('./views/ThemeView.vue'), import('./themes/loader.js')]).then(([{ default: ThemeView }, loader]) => {
    loader.applyTheme(loader.chooseTheme(loader.builtInThemes(), new URLSearchParams(location.search).get('theme') || loader.DEFAULT_THEME_ID).theme)
    Object.assign(document.body.style, { margin: '0', background: 'var(--wallpaper) var(--wallpaper-position) / cover no-repeat fixed, #040817', color: 'var(--text)', fontFamily: "-apple-system, 'Segoe UI', system-ui, sans-serif" })
    Object.assign(document.getElementById('app').style, { position: 'fixed', inset: '0' })
    const q = new URLSearchParams(location.search)
    createApp(ThemeView, { companion: 'lyra', devStory: q.get('story') ?? '', devDiary: q.get('look') ?? '' }).mount('#app')
  })
} else if (devDiary) {
  import('./components/DiaryDemo.vue').then(({ default: DiaryDemo }) =>
    createApp(DiaryDemo, { design: devDiary, theme: new URLSearchParams(location.search).get('theme') ?? '' }).mount('#app'),
  )
} else if (devStage) {
  import('./components/stage/StageDemo.vue').then(({ default: StageDemo }) =>
    createApp(StageDemo, { look: devStage, faces: new URLSearchParams(location.search).get('faces') ?? '' }).mount('#app'),
  )
} else if (view?.view === 'world') {
  Object.assign(document.documentElement.style, { height: '100%', overflow: 'hidden' })
  Object.assign(document.body.style, { margin: '0', height: '100%', overflow: 'hidden', background: '#040817' })
  import('./worlds/WorldWindow.vue').then(({ default: WorldWindow }) => createApp(WorldWindow, { initial: view }).mount('#app'))
} else if (view?.view === 'browser') {
  // the BROWSER window's own toolbar page (browser.rs); the web pages are separate webviews
  Object.assign(document.documentElement.style, { height: '100%', overflow: 'hidden' })
  Object.assign(document.body.style, { margin: '0', height: '100%', overflow: 'hidden', background: '#050b1f' })
  import('./browser/BrowserWindow.vue').then(({ default: BrowserWindow }) => createApp(BrowserWindow).mount('#app'))
} else if (view?.view === 'identify') {
  // see-through, so only the number and its frame show over whatever is on that screen
  Object.assign(document.documentElement.style, { height: '100%', overflow: 'hidden', background: 'transparent' })
  Object.assign(document.body.style, { margin: '0', height: '100%', overflow: 'hidden', background: 'transparent' })
  import('./worlds/IdentifyNumber.vue').then(({ default: IdentifyNumber }) => createApp(IdentifyNumber, { number: Number(view.number) || 1 }).mount('#app'))
} else {
  createApp(App).mount('#app')
}
