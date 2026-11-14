<script setup>
// THEME: a showroom with three cases - the main theme (the whole Castle), the diary theme (how
// the DIARY is browsed and the page it wears) and the story theme (the STORY stage). Each case
// shows the look that is worn; its arrows walk through the others. Create theme makes a main
// theme: Castle drafts a working one from a picture at once, then the companion may polish it
// or paint a wallpaper for it. Castle's loader checks every theme before it is used.
import { computed, onMounted, ref } from 'vue'
import {
  choosePicture,
  getSettings,
  saveSettings,
  terminalState,
  terminalWrite,
  themeCreate,
  themeDelete,
  themeFolder,
  userThemes,
} from '../api/castle.js'
import DiaryPage from '../components/DiaryPage.vue'
import { lookOf } from '../components/diaryviews.js'
import { typeLine } from '../components/typeline.js'
import { DIARY_THEMES, indexOf, SAMPLE_PAGE, SAMPLE_PAGES, SAMPLE_TEXT, step, STORY_THEMES, wearingLine } from '../components/themeroom.js'
import ProfileCards from './profile/ProfileCards.vue'
import ProfileConstellation from './profile/ProfileConstellation.vue'
import { PROFILE_LOOKS, profileLookOf } from './profile/profile.js'
import { builtInThemes, fromUserFolders, problemWith } from '../themes/loader.js'

const props = defineProps({
  companion: { type: String, default: 'Your companion' },
  // dev only (?theme-room=1&story=<id>&look=<id>): which stage and diary look to open at
  devStory: { type: String, default: '' },
  devDiary: { type: String, default: '' },
})
const emit = defineEmits(['changed', 'show-chat'])

const themes = ref([])
const broken = ref([])
const currentId = ref('constellation')
// the main theme standing in the case (walked with the arrows; worn only after Use)
const shownAt = ref(0)
const diaryLook = ref('list')
const storyLook = ref('classic')
const profileLook = ref('cards')
// sample traits for the PROFILE case: never anyone's own
const SAMPLE_TRAITS = [
  { number: 1, name: 'Gentle Curiosity', line: 'asks one more question, kindly', audience: 'everyone', status: 'active' },
  { number: 2, name: 'Quiet Warmth', line: 'soft, unhurried, never cold', audience: 'everyone', status: 'active' },
  { number: 3, name: 'Playful Teasing', line: 'a little teasing, never unkind', audience: 'owner', status: 'active' },
  { number: 4, name: 'Steady Honesty', line: 'says the true thing gently', audience: 'everyone', status: 'active' },
]
const SAMPLE_WAITING = [{ key: 'new-0', source: 'new', index: 0, name: 'Shy When Praised', line: 'flustered, then glad', audience: 'everyone' }]
const profileAt = computed(() => Math.max(0, PROFILE_LOOKS.findIndex((l) => l.id === profileLook.value)))
const profile = computed(() => PROFILE_LOOKS[profileAt.value])
const busy = ref('')
const problem = ref('')
const done = ref('')
const form = ref({ open: false, name: '', mood: '', picture: '' })
const lastCreated = ref('')
let settings = null

const pictureName = computed(() => form.value.picture.split(/[\\/]/).pop())
const shown = computed(() => themes.value[shownAt.value] ?? null)
const worn = computed(() => themes.value.find((t) => t.id === currentId.value) ?? null)
const diaryAt = computed(() => indexOf(DIARY_THEMES, diaryLook.value))
const storyAt = computed(() => indexOf(STORY_THEMES, storyLook.value))
const diary = computed(() => DIARY_THEMES[diaryAt.value])
const story = computed(() => STORY_THEMES[storyAt.value])
const wearing = computed(() => wearingLine(worn.value?.name, diaryLook.value, storyLook.value))

async function load() {
  try {
    const view = await getSettings()
    settings = view.settings
    currentId.value = view.settings.theme ?? 'constellation'
    diaryLook.value = lookOf(view.settings.diaryLook)
    storyLook.value = view.settings.storyLook ?? 'classic'
    profileLook.value = profileLookOf(view.settings.profileLook)
    const yours = fromUserFolders(await userThemes())
    themes.value = [...builtInThemes(), ...yours.themes]
    broken.value = yours.broken
  } catch {
    themes.value = builtInThemes()
    if (props.devStory) storyLook.value = props.devStory
    if (props.devDiary) diaryLook.value = lookOf(props.devDiary)
  }
  shownAt.value = indexOf(themes.value, currentId.value)
}

async function attempt(label, work) {
  problem.value = ''
  done.value = ''
  busy.value = label
  try {
    return await work()
  } catch (reason) {
    problem.value = String(reason)
    return null
  } finally {
    busy.value = ''
  }
}

async function use(theme) {
  const why = problemWith(theme)
  if (why) {
    problem.value = why
    return
  }
  const saved = await attempt('Changing the theme…', () => saveSettings({ ...settings, theme: theme.id }))
  if (saved) {
    settings = saved.settings
    currentId.value = theme.id
    emit('changed')
  }
}

// The diary and story cases change what is worn as the arrows turn: they are light to change
// and nothing else on the screen moves.
async function wear(patch, apply) {
  apply()
  if (!settings) return // outside Castle's window: the case still turns
  const saved = await attempt('Saving…', () => saveSettings({ ...settings, ...patch }))
  if (saved) {
    settings = saved.settings
    emit('changed')
  }
}

function turnDiary(by) {
  const next = DIARY_THEMES[step(diaryAt.value, by, DIARY_THEMES.length)].id
  wear({ diaryLook: next }, () => (diaryLook.value = next))
}

function turnProfile(by) {
  const next = PROFILE_LOOKS[step(profileAt.value, by, PROFILE_LOOKS.length)].id
  wear({ profileLook: next }, () => (profileLook.value = next))
}

function turnStory(by) {
  const next = STORY_THEMES[step(storyAt.value, by, STORY_THEMES.length)].id
  wear({ storyLook: next }, () => (storyLook.value = next))
}

async function remove(theme) {
  if (theme.id === currentId.value) {
    problem.value = 'Choose another theme first - this one is in use.'
    return
  }
  if (await attempt('Deleting…', () => themeDelete(theme.id)) !== null) await load()
}

async function pickPicture() {
  const path = await choosePicture('Choose a picture for your theme')
  if (path) form.value.picture = path
}

async function create() {
  const id = await attempt('Making your theme…', () => themeCreate(form.value.name, form.value.mood, form.value.picture || null))
  if (!id) return
  lastCreated.value = id
  await load()
  const made = themes.value.find((t) => t.id === id)
  if (made) await use(made)
  shownAt.value = indexOf(themes.value, id)
  done.value = `${made?.name ?? 'Your theme'} is ready.`
  form.value = { open: false, name: '', mood: '', picture: '' }
}

// A brief for the companion, sent through the session like typed words (several lines go in
// as one paste). The rules keep theme.json something Castle's loader accepts.
async function askCompanion(theme, paint) {
  const state = await terminalState().catch(() => null)
  if (!state?.running) {
    problem.value = `Wake ${props.companion} first, then ask again.`
    return
  }
  const folder = await themeFolder(theme.id)
  const lines = paint
    ? [
        `Please paint a wallpaper for my Castle theme "${theme.name}".`,
        `Mood: ${theme.description || 'calm and beautiful'}. A wide landscape picture with no text, calm on the left third (a panel sits there) and on the right side (cards sit there).`,
        `Save it as ${folder}\\wallpaper.png (1536x1024) with Castle's image helper, and set "wallpaper": "wallpaper.png" in ${folder}\\theme.json.`,
        'Tell me the cost first and ask before you draw.',
      ]
    : [
        `Please polish my Castle theme "${theme.name}" in ${folder}\\theme.json.`,
        `My words for it: ${theme.description || 'make it feel like the picture'}.`,
        'Edit only that file. Keep "id", "name" and "wallpaper". Colours go only in "colors" as --variables (--background, --text, --text-dim, --accent, --line, --panel-background, --widget-background, --bar-background, --hover). Widget places use only left, top, right, bottom, width, height.',
        'Keep text easy to read on the background. Tell me when you are done.',
      ]
  const text = lines.join('\n')
  await typeLine(terminalWrite, `\u001b[200~${text}\u001b[201~`)
  done.value = `Asked ${props.companion}. When it is done, press Reload themes.`
  emit('show-chat')
}

onMounted(load)
</script>

<template>
  <div class="theme-room">
    <header class="bar">
      <h2>THEME</h2>
      <span class="spacer"></span>
      <span v-if="busy" class="busy">{{ busy }}</span>
      <span v-else-if="problem" class="problem">{{ problem }}</span>
      <span v-else-if="done" class="good">{{ done }}</span>
      <button @click="load">Reload themes</button>
    </header>

    <section v-if="form.open" class="create widget">
      <h3>Create a theme</h3>
      <div class="row">
        <label>Name <input v-model="form.name" placeholder="Sakura Night" maxlength="40" /></label>
        <label class="wide">Mood and colours, in your words <input v-model="form.mood" placeholder="soft pink blossoms under a dark blue night, calm" /></label>
      </div>
      <div class="row">
        <button @click="pickPicture">{{ form.picture ? 'Change picture' : 'Upload a picture' }}</button>
        <span class="hint">{{ form.picture ? pictureName : 'No picture: Castle makes a soft gradient, and you can ask your companion to paint one after.' }}</span>
        <span class="spacer"></span>
        <button class="primary" :disabled="!form.name.trim() || !!busy" @click="create">Create</button>
      </div>
      <p class="hint">Castle takes the colours from your picture so the theme works right away. Then you can ask {{ companion }} to polish it.</p>
    </section>

    <div class="showroom">
      <!-- the main theme: the whole Castle -->
      <section class="case">
        <h3>MAIN THEME</h3>
        <div class="glass">
          <span class="lamp" aria-hidden="true" />
          <div
            v-if="shown"
            class="stage picture"
            :style="shown.wallpaperUrl ? { backgroundImage: `url(&quot;${shown.wallpaperUrl}&quot;)` } : { background: shown.colors?.['--background'] ?? '#050b1f' }"
          >
            <span class="swatch" :style="{ background: shown.colors?.['--accent'] }"></span>
            <span v-if="shown.id === currentId" class="badge">In use</span>
            <span class="source">{{ shown.source === 'yours' ? 'Yours' : 'Built in' }}</span>
          </div>
        </div>
        <div class="pick">
          <button class="arrow" aria-label="Earlier theme" :disabled="themes.length < 2" @click="shownAt = step(shownAt, -1, themes.length)">‹</button>
          <strong>{{ shown?.name ?? '' }}</strong>
          <button class="arrow" aria-label="Later theme" :disabled="themes.length < 2" @click="shownAt = step(shownAt, 1, themes.length)">›</button>
        </div>
        <p class="dots" aria-hidden="true"><i v-for="(t, i) in themes" :key="t.id" :class="{ on: i === shownAt, worn: t.id === currentId }" /></p>
        <p class="line" :title="shown?.description">{{ shown?.description }}</p>
        <div v-if="shown" class="row centre">
          <button v-if="shown.id !== currentId" class="primary" @click="use(shown)">Use</button>
          <template v-if="shown.source === 'yours'">
            <button :title="`Ask ${companion} to polish the colours and layout`" @click="askCompanion(shown, false)">Polish</button>
            <button :title="`Ask ${companion} to paint a wallpaper`" @click="askCompanion(shown, true)">Paint</button>
            <button class="danger" @click="remove(shown)">Delete</button>
          </template>
        </div>
      </section>

      <!-- the diary theme: how the DIARY is browsed, and the page it wears -->
      <section class="case">
        <h3>DIARY THEME</h3>
        <div class="glass">
          <span class="lamp" aria-hidden="true" />
          <div class="stage page">
            <DiaryPage :design="diary.page" compact :page="SAMPLE_PAGE" :text="SAMPLE_TEXT" :pages="SAMPLE_PAGES" :companion="companion" />
          </div>
        </div>
        <div class="pick">
          <button class="arrow" aria-label="Earlier diary theme" @click="turnDiary(-1)">‹</button>
          <strong>{{ diary.name }}</strong>
          <button class="arrow" aria-label="Later diary theme" @click="turnDiary(1)">›</button>
        </div>
        <p class="dots" aria-hidden="true"><i v-for="(t, i) in DIARY_THEMES" :key="t.id" :class="{ on: i === diaryAt, worn: i === diaryAt }" /></p>
        <p class="line" :title="diary.line">{{ diary.line }}</p>
      </section>

      <!-- the story theme: the STORY stage -->
      <section class="case">
        <h3>STORY THEME</h3>
        <div class="glass">
          <span class="lamp" aria-hidden="true" />
          <!-- the same picture of each stage that the STORY room's own Look uses -->
          <div class="stage shot"><img :src="`/story-looks/${story.id}.jpg`" :alt="story.name" /></div>
        </div>
        <div class="pick">
          <button class="arrow" aria-label="Earlier story theme" @click="turnStory(-1)">‹</button>
          <strong>{{ story.name }}</strong>
          <button class="arrow" aria-label="Later story theme" @click="turnStory(1)">›</button>
        </div>
        <p class="dots" aria-hidden="true"><i v-for="(t, i) in STORY_THEMES" :key="t.id" :class="{ on: i === storyAt, worn: i === storyAt }" /></p>
        <p class="line" :title="story.line">{{ story.line }}</p>
      </section>

      <!-- the profile theme: how HOME's PROFILE tab draws the emotion engine, with sample traits -->
      <section class="case">
        <h3>PROFILE THEME</h3>
        <div class="glass">
          <span class="lamp" aria-hidden="true" />
          <div class="stage profile-preview" inert>
            <ProfileCards v-if="profile.id === 'cards'" :companion="companion" :lived="SAMPLE_TRAITS" :waiting="SAMPLE_WAITING" />
            <ProfileConstellation v-else :companion="companion" :lived="SAMPLE_TRAITS" :waiting="SAMPLE_WAITING" />
          </div>
        </div>
        <div class="pick">
          <button class="arrow" aria-label="Earlier profile theme" @click="turnProfile(-1)">‹</button>
          <strong>{{ profile.label }}</strong>
          <button class="arrow" aria-label="Later profile theme" @click="turnProfile(1)">›</button>
        </div>
        <p class="dots" aria-hidden="true"><i v-for="(t, i) in PROFILE_LOOKS" :key="t.id" :class="{ on: i === profileAt, worn: i === profileAt }" /></p>
        <p class="line" :title="profile.line">{{ profile.line }}</p>
      </section>
    </div>

    <footer class="foot">
      <span class="spacer"></span>
      <p class="wearing">{{ wearing }}</p>
      <span class="spacer end"><button class="primary" @click="form.open = !form.open">+ Create theme</button></span>
    </footer>

    <div v-if="broken.length" class="broken widget">
      <h3>Themes that could not be read</h3>
      <p v-for="b in broken" :key="b.folder">{{ b.folder }}: {{ b.error }}</p>
    </div>
  </div>
</template>

<style scoped>
.theme-room {
  position: absolute;
  inset: 24px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  overflow-y: auto;
}
.widget {
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 14px;
  backdrop-filter: blur(var(--glass-blur));
}
.bar,
.row {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
}
.row.centre {
  justify-content: center;
}
/* Nothing under the cases may change their height as you step through the themes: the showroom is
   centred on its tallest case, so any change would lift or drop all three at once. The main theme's
   buttons (Use, and Polish / Paint / Delete for your own) come and go, so their row keeps its height
   either way, and the one line under each case below is held at three lines. */
.case .row.centre {
  min-height: 32px;
}
h2 {
  margin: 0;
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.2em;
  color: var(--accent);
}
.create h3,
.broken h3 {
  margin: 0 0 10px;
  font-size: 15px;
  font-weight: 500;
}
.spacer {
  flex: 1;
}
.create {
  padding: 18px 20px;
}
label {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
}
label.wide {
  flex: 1;
}
input {
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 14px;
}

/* ---------- the showroom ---------- */
.showroom {
  flex: 1;
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: clamp(18px, 3.2vw, 56px);
  /* the cases keep their own shape and sit in the upper middle, the sky around them */
  align-content: center;
  align-items: start;
  width: 100%;
  max-width: 1500px;
  margin: 0 auto;
  padding: 6px clamp(8px, 2vw, 32px) 0;
  box-sizing: border-box;
}
.case {
  display: flex;
  flex-direction: column;
  min-height: 0;
  gap: 8px;
  text-align: center;
}
.case h3 {
  margin: 0 0 4px;
  font-family: 'Palatino Linotype', Palatino, Georgia, serif;
  font-size: 15px;
  font-weight: 400;
  letter-spacing: 0.34em;
  color: var(--text);
  /* readable over a bright wallpaper too */
  text-shadow: 0 1px 3px rgba(0, 0, 0, 0.85), 0 0 14px rgba(0, 0, 0, 0.7);
}
/* a display case: a frame of glass lit from above, the look standing inside it */
.glass {
  position: relative;
  display: flex;
  padding: 24px 14px 14px;
  border: 1px solid color-mix(in srgb, var(--accent) 55%, var(--line));
  border-radius: 10px;
  background:
    linear-gradient(180deg, color-mix(in srgb, var(--accent) 16%, transparent), transparent 34%),
    color-mix(in srgb, var(--panel-background) 82%, transparent);
  box-shadow:
    inset 0 0 0 5px color-mix(in srgb, var(--accent) 7%, transparent),
    inset 0 -30px 40px -26px color-mix(in srgb, var(--accent) 30%, transparent),
    0 18px 40px -18px rgba(0, 0, 0, 0.7);
  backdrop-filter: blur(var(--glass-blur));
}
.lamp {
  position: absolute;
  top: 8px;
  left: 50%;
  width: 46px;
  height: 6px;
  transform: translateX(-50%);
  border-radius: 999px;
  background: #fff;
  box-shadow: 0 0 14px 4px color-mix(in srgb, var(--accent) 70%, #fff), 0 10px 46px 18px color-mix(in srgb, var(--accent) 22%, transparent);
}
.stage {
  position: relative;
  flex: 1;
  min-width: 0;
  /* every case's window is the same wide shape, whatever stands in it */
  aspect-ratio: 16 / 10;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 8px;
  text-align: left;
}
.stage.picture {
  background-size: cover;
  background-position: center;
}
.stage.page {
  background: color-mix(in srgb, #04081c 55%, transparent);
  padding: 4px 6px;
}
/* the page is longer than the window: it fades out at the bottom edge, never cut mid-line */
.stage.page::after {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 34%;
  background: linear-gradient(to bottom, transparent, #060b22 88%);
  pointer-events: none;
}
.stage.page :deep(.diary-page) {
  overflow: hidden;
}
.stage.shot {
  background: #04081c;
}
.stage.shot img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.swatch {
  position: absolute;
  bottom: 12px;
  left: 12px;
  width: 22px;
  height: 22px;
  border: 2px solid rgba(255, 255, 255, 0.7);
  border-radius: 50%;
}
.badge {
  position: absolute;
  top: 12px;
  right: 12px;
  padding: 3px 10px;
  border-radius: 999px;
  background: var(--accent);
  color: #07142e;
  font-size: 12px;
  font-weight: 600;
}
.source {
  position: absolute;
  right: 12px;
  bottom: 12px;
  padding: 2px 8px;
  border-radius: 999px;
  background: rgba(0, 0, 0, 0.45);
  font-size: 11px;
  color: var(--text);
}
/* the plinth under a case: the name, the dots and one line, on glass so they read over any wallpaper */
.pick,
.dots,
.line {
  background: color-mix(in srgb, var(--panel-background) 88%, #04081c);
  backdrop-filter: blur(var(--glass-blur));
}
.pick {
  display: grid;
  grid-template-columns: 36px minmax(0, 1fr) 36px;
  align-items: center;
  gap: 14px;
  margin-top: 6px;
  padding: 10px 12px 4px;
  border: 1px solid var(--line);
  border-bottom: none;
  border-radius: 12px 12px 0 0;
}
.pick strong {
  min-width: 0;
  font-family: 'Palatino Linotype', Palatino, Georgia, serif;
  font-style: italic;
  font-weight: 400;
  font-size: 19px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.arrow {
  width: 36px;
  height: 36px;
  box-sizing: border-box;
  padding: 0 8px;
  border: none;
  background: none;
  color: var(--text);
  font-size: 26px;
  line-height: 1;
  cursor: pointer;
}
.arrow:hover {
  color: var(--accent);
}
.dots {
  display: flex;
  justify-content: center;
  gap: 9px;
  margin: -8px 0 0;
  padding: 6px 0;
  border-left: 1px solid var(--line);
  border-right: 1px solid var(--line);
}
.dots i {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: color-mix(in srgb, var(--text) 28%, transparent);
}
.dots i.worn {
  background: color-mix(in srgb, var(--accent) 55%, transparent);
}
.dots i.on {
  background: var(--accent);
  box-shadow: 0 0 8px var(--accent);
}
.line {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 3;
  overflow: hidden;
  min-height: 4.35em; /* three lines of 1.45, so a short line and a long one stand equally tall */
  box-sizing: content-box;
  margin: -8px 0 0;
  padding: 2px 14px 10px;
  border: 1px solid var(--line);
  border-top: none;
  border-radius: 0 0 12px 12px;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-dim);
}
.foot {
  display: flex;
  align-items: center;
  gap: 10px;
}
.foot .end {
  display: flex;
  justify-content: flex-end;
}
.wearing {
  margin: 0;
  padding: 6px 22px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: color-mix(in srgb, var(--panel-background) 88%, #04081c);
  backdrop-filter: blur(var(--glass-blur));
  font-family: 'Palatino Linotype', Palatino, Georgia, serif;
  font-style: italic;
  font-size: 17px;
  text-align: center;
}
.hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-dim);
}
.busy {
  font-size: 13px;
  color: var(--accent);
}
.problem {
  font-size: 13px;
  color: var(--warn);
}
.good {
  font-size: 13px;
  color: var(--good);
}
.broken {
  padding: 14px 18px;
  font-size: 13px;
  color: var(--warn);
}
button {
  padding: 6px 14px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
button.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #07142e;
  font-weight: 600;
}
button.danger {
  border-color: var(--bad);
  color: var(--bad);
}
button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.profile-preview {
  overflow: hidden;
  padding: 8px;
  zoom: 0.6;
  pointer-events: none;
}
</style>
