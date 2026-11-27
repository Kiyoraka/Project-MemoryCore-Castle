<script setup>
// Settings: everything setup asked, changeable any time - the memory folder and companion, the
// keys (masked, Show, Test, Remove), quick actions, the room, backup, startup and updates.
// Built to the Settings standard (docs/settings-standard.md): a sidebar in five groups, one page per
// item, groups of rows, one control kit. Toggles, choices and selects save at once and flash
// "Saved ✓"; typed values get Save and Cancel; a failed save says why under its row.
import AlertsPane from './AlertsPane.vue'
import SentinelSettings from './SentinelSettings.vue'
import ComfySettings from './ComfySettings.vue'
import WidgetsView from './WidgetsView.vue'
import { computed, nextTick, onMounted, provide, ref, watch } from 'vue'
import ModalSheet from '../components/ModalSheet.vue'
import BackupPanel from '../components/BackupPanel.vue'
import DatabasePanel from '../components/DatabasePanel.vue'
import HelpersStatus from '../components/HelpersStatus.vue'
import ContributorsWall from '../components/ContributorsWall.vue'
import SettingsPage from '../components/settings/SettingsPage.vue'
import SettingGroup from '../components/settings/SettingGroup.vue'
import SettingRow from '../components/settings/SettingRow.vue'
import SettingSwitch from '../components/settings/SettingSwitch.vue'
import SettingChoice from '../components/settings/SettingChoice.vue'
import SettingConfirm from '../components/settings/SettingConfirm.vue'
import SettingPill from '../components/settings/SettingPill.vue'
import { ROW_STATUS, createRowStatus } from '../components/settings/rowstatus.js'
import { valueOf } from '../components/settings/settingsDefaults.js'
import { SETTINGS_GROUPS, pageOf, parseTarget, stepPage, subtitleOf } from '../components/settings/settingsnav.js'
import { isMac } from '../platform.js'
import { automationProgramPick, automationProgramsSave, getSettings, keyStatus, openUrl, releasesPage, saveSettings, installCopies } from '../api/castle.js'
import { copiesLine } from '../components/copies.js'
import CompanionPane from './settings/CompanionPane.vue'
import ChatPane from './settings/ChatPane.vue'
import RoomsPane from './settings/RoomsPane.vue'
import KeysPane from './settings/KeysPane.vue'
import { SETTINGS_PAGE } from './settings/settingsPage.js'

const props = defineProps({
  // which page opens first, and optionally the row to scroll to: "keys", "chat#pictures"
  section: { type: String, default: 'companion' },
  themeId: { type: String, default: '' },
  themeName: { type: String, default: '' },
  themeWidgets: { type: Object, default: () => ({}) },
  version: { type: String, default: '' },
  updateLine: { type: String, default: '' },
})
const emit = defineEmits(['close', 'saved', 'setup', 'whatsnew', 'open-room', 'arrange', 'tidy'])

// The few words that name the computer's own parts: Windows' wording, or the Mac's.
const words = isMac
  ? {
      thisComputer: 'this Mac',
      keysKept: "in your Mac's Keychain",
      moveKeys: '⌥← / ⌥→',
      startWith: 'Open Castle when you log in',
      wakeWhen: 'when Castle opens at log-in',
      minimise: 'Minimise to the Dock',
      minimiseOff: 'Off: the minus button hides Castle to the menu bar icon.',
      minimiseHint: 'The minus button in the top bar then minimises Castle to the Dock, where one click brings it back. With this off, it hides Castle to the menu bar icon.',
    }
  : {
      thisComputer: 'this PC',
      keysKept: 'encrypted by Windows',
      moveKeys: 'Alt+Left / Alt+Right',
      startWith: 'Start Castle with Windows',
      wakeWhen: 'when Castle starts with Windows',
      minimise: 'Minimise to the taskbar',
      minimiseOff: 'Off: the minus button hides Castle to the tray icon.',
      minimiseHint: 'The minus button in the top bar then minimises Castle to the taskbar, where one click brings it back. With this off, it hides Castle to the tray icon.',
    }

// ---------- pages, deep links, row status ----------

const rows = createRowStatus()
provide(ROW_STATUS, rows)

const start = parseTarget(props.section)
const tab = ref(start.page)
const anchor = ref(start.anchor)
const panel = ref(null)

function go(target) {
  const next = parseTarget(target)
  tab.value = next.page
  anchor.value = next.anchor
}
watch(() => props.section, go)

function onNavKey(event) {
  const next = stepPage(tab.value, event.key)
  if (!next) return
  event.preventDefault()
  tab.value = next
  event.currentTarget.querySelector(`[data-section="${next}"]`)?.focus()
}

// scroll to and light the row a deep link names, once its page is drawn
async function showAnchor() {
  if (!anchor.value) return
  const id = anchor.value
  anchor.value = ''
  await nextTick()
  const target = panel.value?.querySelector(`[data-setting="${CSS.escape(id)}"]`)
  if (!target) return
  target.scrollIntoView({ block: 'center', behavior: window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' })
  rows.light(id)
}

const settings = ref(null)
const view = ref(null)
const keys = ref([])

const companionLabel = computed(() => settings.value?.companionName ?? 'Your companion')
const subtitle = computed(() => subtitleOf(tab.value, settings.value?.companionName ?? 'your companion'))
const title = computed(() => pageOf(tab.value).label)

async function sentinelChanged() {
  settings.value = (await getSettings()).settings
}

// the ComfyUI page saves its own choices; Studio reads them too, so the rest of Castle is told
async function comfyChanged() {
  settings.value = (await getSettings()).settings
  emit('saved')
}

async function widgetsChanged() {
  settings.value = (await getSettings()).settings
  emit('saved')
}

// ---------- saving ----------

const busy = ref('')
const saving = ref(false)
const problem = ref('')
const done = ref('')
const newAction = ref({ label: '', command: '' })
const locked = computed(() => !!busy.value || saving.value)
// the panes (./settings/*Pane.vue) share the page's state: the settings, the keys, the footer, saving
provide(SETTINGS_PAGE, { settings, view, keys, locked, saving, busy, problem, done, attempt, saveRow, save, rows, companionLabel, words, go, emit: (...args) => emit(...args) })

async function attempt(label, work) {
  problem.value = ''
  done.value = ''
  busy.value = label
  try {
    return await work()
  } catch (reason) {
    problem.value = String(reason?.message ?? reason)
    return null
  } finally {
    busy.value = ''
  }
}

// One row's instant save: the page takes the new settings only once Castle has kept them, so a
// failure leaves the control on its old value and the reason under the row.
async function saveRow(row, patch) {
  const next = { ...settings.value, ...patch }
  saving.value = true
  problem.value = ''
  done.value = ''
  try {
    const saved = await rows.run(row, async () => {
      if (Object.hasOwn(patch, 'automationsCommands')) { await automationProgramsSave(patch.automationsCommands); return getSettings() }
      return saveSettings(next)
    })
    if (saved) {
      view.value = saved
      settings.value = saved.settings
      emit('saved')
    }
    return saved
  } finally {
    saving.value = false
  }
}

// A save that is more than one row (names, the memory folder): its message goes in the footer.
async function save(patch, message = 'Saved.') {
  const next = { ...settings.value, ...patch }
  const saved = await attempt('Saving…', () => saveSettings(next))
  if (saved) {
    view.value = saved
    settings.value = saved.settings
    done.value = message
    emit('saved')
  }
  return saved
}

async function addAutomationProgram() {
  const program = await rows.run('approved-programs', () => automationProgramPick())
  if (program) await saveRow('approved-programs', { automationsCommands: [...(settings.value.automationsCommands || []), program] })
}

// the names the "named" alerts listen for, as the member sees them here (the room name is Rust's)
const namedTip = computed(() => {
  const heard = [settings.value?.memberName, settings.value?.companionName].filter((n) => n && n.trim())
  const list = heard.length ? heard.join(', ') + ', and your name in the room' : 'Your name in the room and your companion\'s name'
  return `${list}, as a whole word, any case. The glow on the CHAT tab still shows every message.`
})
watch(tab, (now) => {
  // the footer speaks for the tab it happened on, never for the next one
  done.value = ''
  problem.value = ''
  showAnchor()
})





// ---------- Quick actions ----------

async function addAction() {
  const a = { label: newAction.value.label.trim(), command: newAction.value.command.trim() }
  if (!a.label || !a.command) return
  if (await saveRow('add-action', { quickActions: [...(settings.value.quickActions ?? []), a] })) newAction.value = { label: '', command: '' }
}

async function removeAction(i) {
  const list = [...settings.value.quickActions]
  list.splice(i, 1)
  await save({ quickActions: list }, 'Button removed.')
}

// another copy of Castle on this PC (install_check.rs), said in About
const copies = ref(null)

onMounted(async () => {
  installCopies().then((c) => (copies.value = c)).catch(() => {})
  try {
    view.value = await getSettings()
    settings.value = view.value.settings
    keys.value = await keyStatus()
    showAnchor()
  } catch (reason) {
    problem.value = String(reason)
  }
})
</script>

<template>
  <ModalSheet title="Settings" :body-key="tab" @close="emit('close')">
    <div v-if="settings" class="settings-frame">
     <div class="settings">
      <nav class="sidebar" role="tablist" aria-orientation="vertical" aria-label="Settings pages" @keydown="onNavKey">
        <template v-for="g in SETTINGS_GROUPS" :key="g.id">
          <span class="nav-group" aria-hidden="true">{{ g.label }}</span>
          <button
            v-for="p in g.pages"
            :id="`settings-tab-${p.id}`"
            :key="p.id"
            role="tab"
            :data-section="p.id"
            :aria-selected="tab === p.id"
            aria-controls="settings-panel"
            :tabindex="tab === p.id ? 0 : -1"
            :class="{ active: tab === p.id }"
            @click="go(p.id)"
          >
            {{ p.label }}
          </button>
        </template>
      </nav>

      <div id="settings-panel" ref="panel" class="panel" role="tabpanel" :aria-labelledby="`settings-tab-${tab}`" tabindex="0">
        <SettingsPage :title="title" :subtitle="subtitle">
          <!-- ============ Companion ============ -->
          <CompanionPane v-if="tab === 'companion'" />

          <!-- ============ Chat ============ -->
          <ChatPane v-else-if="tab === 'chat'" />

          <template v-else-if="tab === 'automations'">
            <SettingGroup id="programs" title="Command jobs">
              <SettingRow id="approved-programs" label="Approved programs" description="Only these programs may run from a job." tip="Choose an executable from this computer. Scripts use an approved interpreter, with the script file as an argument. Removing a program stops future runs using it.">
                <button :disabled="locked" @click="addAutomationProgram">Add program</button>
                <template #below>
                  <p v-if="!settings.automationsCommands?.length">No programs approved yet.</p>
                  <ul class="approved-programs"><li v-for="program in settings.automationsCommands || []" :key="program"><code>{{ program }}</code><SettingConfirm label="Remove" :disabled="locked" @confirm="saveRow('approved-programs', { automationsCommands: settings.automationsCommands.filter(p => p !== program) })" /></li></ul>
                </template>
              </SettingRow>
              <SettingRow id="command-limits" label="Run limits" description="Two jobs at once, with the newest 200 runs kept for each.">
                <SettingPill>While Castle is open</SettingPill>
              </SettingRow>
            </SettingGroup>
          </template>

          <!-- ============ Rooms ============ -->
          <RoomsPane v-else-if="tab === 'rooms'" />

          <!-- ============ Quick actions ============ -->
          <template v-else-if="tab === 'quick'">
            <SettingGroup id="buttons" title="Your buttons" note="After Save diary, Save project and Save memory">
              <SettingRow v-for="(a, i) in settings.quickActions" :key="a.label + i" :label="a.label">
                <template #description><code>{{ a.command }}</code></template>
                <SettingConfirm label="Remove" :disabled="locked" @confirm="removeAction(i)" />
              </SettingRow>
              <SettingRow v-if="!settings.quickActions?.length" label="No buttons yet" description="Add one below." />
            </SettingGroup>
            <SettingGroup id="add" title="Add a button">
              <SettingRow id="add-label" label="Button name" description="Up to 24 characters.">
                <input v-model="newAction.label" aria-label="Button name" placeholder="Plan my day" maxlength="24" />
              </SettingRow>
              <SettingRow id="add-action" label="Words it types">
                <input v-model="newAction.command" class="wide" aria-label="Words it types" placeholder="the words to type, e.g. plan my day" maxlength="200" @keydown.enter="addAction" />
                <button class="primary" :disabled="locked || !newAction.label.trim() || !newAction.command.trim()" @click="addAction">Add</button>
              </SettingRow>
            </SettingGroup>
          </template>

          <!-- ============ Widgets ============ -->
          <WidgetsView v-else-if="tab === 'widgets'" :theme-id="themeId" :theme-name="themeName" :theme-widgets="themeWidgets" @arrange="emit('arrange')" @tidy="emit('tidy')" @changed="widgetsChanged" />

          <!-- ============ Alerts ============ -->
          <AlertsPane v-else-if="tab === 'alerts'" :settings="settings" :busy="locked ? 'busy' : ''" :companion-label="companionLabel" :named-tip="namedTip" @save="(patch, row) => saveRow(row, patch)" @open-room="(room) => emit('open-room', room)" />

          <!-- ============ Sentinel ============ -->
          <SentinelSettings v-else-if="tab === 'sentinel'" :settings="settings" @saved="sentinelChanged" />

          <!-- ============ ComfyUI ============ -->
          <ComfySettings v-else-if="tab === 'comfy'" :settings="settings" @saved="comfyChanged" />

          <!-- ============ Keys ============ -->
          <KeysPane v-else-if="tab === 'keys'" />

          <!-- ============ Database ============ -->
          <template v-else-if="tab === 'database'">
            <SettingGroup id="helpers" title="Helpers" free>
              <HelpersStatus />
            </SettingGroup>
            <SettingGroup id="memory-db" title="Memory database" free>
              <DatabasePanel :companion="settings.companionName ?? 'your companion'" @changed="emit('saved')" />
            </SettingGroup>
          </template>

          <!-- ============ Backup ============ -->
          <SettingGroup v-else-if="tab === 'backup'" id="backup" title="Backup on GitHub" free>
            <BackupPanel :companion="settings.companionName ?? 'your companion'" />
          </SettingGroup>

          <!-- ============ Start-up ============ -->
          <template v-else-if="tab === 'startup'">
            <SettingGroup id="start" title="Start-up">
              <SettingRow id="start-with" :label="words.startWith" :description="view?.startupNote || 'Opens Castle after you sign in. Automations run while Castle stays open, including when hidden or minimized.'">
                <SettingSwitch :model-value="valueOf(settings, 'startWithWindows')" :label="words.startWith" :disabled="locked" @update:model-value="(on) => saveRow('start-with', { startWithWindows: on })" />
              </SettingRow>
              <SettingRow id="wake" :label="`Wake ${companionLabel} on start`" :description="`Starts ${companionLabel}'s session ${words.wakeWhen}.`">
                <SettingSwitch :model-value="valueOf(settings, 'startSessionOnLaunch')" :label="`Wake ${companionLabel} on start`" :disabled="locked" @update:model-value="(on) => saveRow('wake', { startSessionOnLaunch: on })" />
              </SettingRow>
              <SettingRow id="minimise" :label="words.minimise" :description="words.minimiseOff" :tip="words.minimiseHint">
                <SettingSwitch :model-value="valueOf(settings, 'minimizeToTaskbar')" :label="words.minimise" :disabled="locked" @update:model-value="(on) => saveRow('minimise', { minimizeToTaskbar: on })" />
              </SettingRow>
            </SettingGroup>
            <SettingGroup id="clock" title="Clock">
              <SettingRow id="display-clock" label="Show the clock" description="Your local time, top right, before the music controls.">
                <SettingSwitch :model-value="valueOf(settings, 'displayClock')" label="Show the clock" :disabled="locked" @update:model-value="(on) => saveRow('display-clock', { displayClock: on })" />
              </SettingRow>
              <SettingRow id="clock-format" label="Clock format">
                <SettingChoice :model-value="valueOf(settings, 'clockFormat') === '12h' ? '12h' : '24h'" :options="[{ value: '24h', label: '24-hour (14:30)' }, { value: '12h', label: '12-hour (2:30 PM)' }]" label="Clock format" :disabled="locked" @update:model-value="(f) => saveRow('clock-format', { clockFormat: f })" />
              </SettingRow>
            </SettingGroup>
            <!-- WebView2 only (Windows): read once at start, so it says Quit and start again -->
            <SettingGroup v-if="!isMac" id="drawing" title="Drawing">
              <SettingRow id="gpu-compositing" label="Smooth GPU drawing" description="Turn off if the mouse cursor looks frozen inside Castle (seen on some AMD graphics cards). Takes effect after Quit and start again." tip="Quit from the tray icon, then open Castle again: opening it while it is still in the tray only wakes the running copy.">
                <SettingSwitch :model-value="valueOf(settings, 'gpuCompositing')" label="Smooth GPU drawing" :disabled="locked" @update:model-value="(on) => saveRow('gpu-compositing', { gpuCompositing: on })" />
              </SettingRow>
            </SettingGroup>
          </template>

          <!-- ============ About ============ -->
          <template v-else>
            <SettingGroup id="castle" title="MemoryCore Castle">
              <SettingRow id="version" :label="`Version ${version}`.trim()" :description="updateLine || 'Castle looks for a new version every few hours.'">
                <button @click="emit('whatsnew')">What's new</button>
                <button @click="releasesPage().then(openUrl).catch(() => {})">All releases</button>
              </SettingRow>
              <SettingRow v-if="copiesLine(copies)" id="copies" :label="copies?.leftover || copies?.two ? 'Two copies of Castle' : 'An older Castle'" :description="copiesLine(copies)" tip="An installer run from inside another app can leave a copy of Castle in that app's private folder, and Windows may start that copy. Castle keeps Start with Windows and the Start menu pointing at the real one; the leftover folder can be deleted once Castle opens from the right place." />
              <SettingRow id="licence" label="Licence" description="Free and open source under the MIT license." tip="Change anything - see MAKE-IT-YOURS.md in the source." />
              <SettingRow id="privacy" label="Your data" description="Your memory, keys and settings stay on this computer." tip="Castle checks GitHub for new versions, and talks to the MemoryCore Foundation only when you sign in or use CHAT. Your memory, keys and settings stay on this computer (and in your private backup, if you choose)." />
            </SettingGroup>
            <SettingGroup id="credits" title="Credits" free>
              <ContributorsWall />
            </SettingGroup>
          </template>
        </SettingsPage>

        <footer>
          <span v-if="busy" class="busy">{{ busy }}</span>
          <span v-else-if="problem" class="warn">{{ problem }}</span>
          <span v-else-if="done" class="good">{{ done }}</span>
        </footer>
      </div>
     </div>
    </div>
  </ModalSheet>
</template>

<style scoped src="./settings/settings-page.css"></style>

<style scoped>.approved-programs{list-style:none;padding:0;display:grid;gap:10px}.approved-programs li{display:flex;align-items:center;gap:16px;justify-content:space-between}.approved-programs code{overflow-wrap:anywhere;min-width:0;font-size:12px}</style>
