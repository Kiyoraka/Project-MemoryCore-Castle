<script setup>
// Settings > Companion: the memory folder, setup again, thinking (CLI, only this companion, start, the weekly share), names and picture, daily life, the local voice.
// Cut from SettingsView.vue (Shard): the page's shared state comes through SETTINGS_PAGE.
import { inject, onMounted, ref } from 'vue'
import { offeredProviders } from '../../components/providers.js'
import SettingGroup from '../../components/settings/SettingGroup.vue'
import SettingRow from '../../components/settings/SettingRow.vue'
import SettingSwitch from '../../components/settings/SettingSwitch.vue'
import SettingChoice from '../../components/settings/SettingChoice.vue'
import SettingText from '../../components/settings/SettingText.vue'
import SettingPill from '../../components/settings/SettingPill.vue'
import { valueOf } from '../../components/settings/settingsDefaults.js'
import { providerSwitch, roomCompanionRefresh, avatarClear, avatarGet, avatarSet, chooseFolder, choosePicture, openUrl, setupApply, setupCheckCli, setupInspect, voiceStudioStatus } from '../../api/castle.js'
import { SETTINGS_PAGE } from './settingsPage.js'

const { settings, locked, done, attempt, saveRow, save, rows, companionLabel, words, go, emit } = inject(SETTINGS_PAGE)

const avatar = ref(null)
const cliCheck = ref(null)

// ---------- Companion ----------

// VoiceStudio (optional, for people who run it on this computer): is it answering right now?
const voiceUp = ref(null)
async function checkVoice() {
  voiceUp.value = settings.value?.voicestudioOn ? await voiceStudioStatus().catch(() => false) : null
}
async function toggleVoice(on) {
  const saved = await saveRow('voicestudio', { voicestudioOn: on })
  if (saved) await checkVoice()
}

async function changeFolder() {
  const folder = await chooseFolder('Choose your MemoryCore folder')
  if (!folder) return
  const state = await attempt('Looking at the folder…', () => setupInspect(folder))
  if (!state) return
  if (!state.isMemorycore) {
    rows.failed('folder', 'that folder has no master-memory.md, so it is not a MemoryCore folder.', 'Not changed')
    return
  }
  await save({ memoryFolder: folder }, 'The memory folder is changed. Wake your companion again to use it.')
}

const NAME_ROWS = { companionName: 'companion-name', memberName: 'member-name', relationshipStyle: 'relationship' }

async function saveName(field, value) {
  const row = NAME_ROWS[field]
  const s = { ...settings.value, [field]: value }
  if (!s.companionName?.trim() || !s.memberName?.trim()) {
    rows.failed(row, 'both names are needed.')
    return
  }
  const saved = await saveRow(row, { [field]: value })
  if (saved && s.memoryFolder) await attempt('Updating CLAUDE.md and AGENTS.md…', () => setupApply(s.memoryFolder, s.companionName, s.memberName, s.relationshipStyle ?? ''))
  if (saved) await tellRoom('Names saved.')
}

// When the companion is in the chat room, its name and picture there follow Settings.
async function tellRoom(message) {
  const told = await attempt('Telling the room…', roomCompanionRefresh)
  if (told?.inRoom) done.value = `${message} ${told.renamed ? 'The room shows the new name and picture.' : 'The room shows it too.'}`
}

async function switchCli(cli) {
  if (settings.value.cli === cli) return
  cliCheck.value = await attempt('Checking…', () => setupCheckCli(cli))
  await attempt('Queuing switch…', async () => {
    const state = await providerSwitch(cli)
    done.value = state.message
  })
}

async function changeAvatar() {
  const path = await choosePicture('Choose a picture for your companion')
  if (!path) return
  const saved = await attempt('Saving…', () => avatarSet(path))
  if (saved) {
    avatar.value = saved
    rows.saved('picture')
    await tellRoom('Picture saved.')
  }
  emit('saved')
}

async function clearAvatar() {
  await attempt('Saving…', avatarClear)
  avatar.value = await avatarGet()
  rows.saved('picture')
  await tellRoom('Picture cleared.')
  emit('saved')
}

onMounted(async () => {
  avatar.value = await avatarGet().catch(() => null)
  await checkVoice()
})
</script>

<template>
  <SettingGroup id="memory" title="Memory">
    <SettingRow id="folder" label="Memory folder">
      <template #description><code>{{ settings.memoryFolder ?? 'Not chosen' }}</code></template>
      <button :disabled="locked" @click="changeFolder">Change</button>
    </SettingRow>
    <SettingRow id="setup" label="First-run setup" description="Asks setup's questions again, from the start.">
      <button :disabled="locked" @click="emit('setup')">Run it again</button>
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="thinking" title="Thinking">
    <SettingRow id="cli" label="Thinks with" description="Switching wakes your companion again with the other CLI.">
      <template v-if="cliCheck && !cliCheck.installed" #description>
        <span class="warn">{{ cliCheck.label }} is not installed: <code>{{ cliCheck.installCommand }}</code></span>
      </template>
      <SettingChoice :model-value="settings.cli" :options="offeredProviders(settings.cli).map((p) => ({ value: p.id, label: p.label }))" label="Thinks with" :disabled="locked" @update:model-value="switchCli" />
    </SettingRow>
    <SettingRow
      id="only-this"
      :label="`Only ${companionLabel} in Castle`"
      :disabled="settings.cli !== 'claude'"
      :description="settings.cli === 'claude' ? `Leaves out your own Claude Code setup, from ${companionLabel}'s next wake.` : 'Only with Claude Code.'"
      tip="Leave out my personal Claude Code setup (its own CLAUDE.md, hooks and plugins) in Castle's session. Turn this on when that setup is another companion, or to test with a clean one."
    >
      <SettingSwitch :model-value="valueOf(settings, 'onlyThisCompanion')" :label="`Only ${companionLabel} in Castle`" :disabled="locked || settings.cli !== 'claude'" @update:model-value="(on) => saveRow('only-this', { onlyThisCompanion: on })" />
    </SettingRow>
    <SettingRow
      id="main-start"
      label="When Castle starts"
      description="Fresh sessions wake from memory and use fewer tokens; Continue keeps the talk."
      tip="Continue picks the last talk up every time. After a rest picks it up within six hours and starts fresh after that, so the morning after does not read the whole old talk back. Fresh always starts new. A fresh session still wakes with its full memory; only what was said but not saved stays in the old talk. Type /clear in the Log to start fresh at any time."
    >
      <SettingChoice :model-value="valueOf(settings, 'mainStart')" :options="[{ value: 'continue', label: 'Continue' }, { value: 'rested', label: 'After a rest' }, { value: 'fresh', label: 'Fresh' }]" label="When Castle starts" :disabled="locked" @update:model-value="(v) => saveRow('main-start', { mainStart: v })" />
    </SettingRow>
    <SettingRow
      id="share-fallback"
      label="When a model's weekly share is spent"
      :disabled="settings.cli !== 'claude'"
      :description="settings.cli === 'claude' ? 'Starts on another model until the share opens again, and says so.' : 'Only with Claude Code.'"
      tip="Some Claude plans give a model its own weekly share. When the model your companion uses has spent it, a session cannot go on with it, so the next wake starts on Opus (or Sonnet when Opus is spent too) and a line above Chat says until when. After the reset your own model comes back by itself. A talk already running when the share runs out stops with Claude Code's own message: wake again, or press Start fresh."
    >
      <SettingSwitch :model-value="valueOf(settings, 'shareFallback')" label="Start on another model when a weekly share is spent" :disabled="locked || settings.cli !== 'claude'" @update:model-value="(on) => saveRow('share-fallback', { shareFallback: on })" />
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="names" title="Name and picture" tip="Names already written in your memory files stay as they are - ask your companion to update the memory if you rename.">
    <SettingRow id="companion-name" label="Companion's name">
      <SettingText :model-value="settings.companionName ?? ''" label="Companion's name" :maxlength="40" :disabled="locked" @save="(v) => saveName('companionName', v)" />
    </SettingRow>
    <SettingRow id="member-name" label="Your name" tip="Your name in the chat room is your Foundation name (Library card > Profile). Your companion's name and picture here go to the room when it is let in.">
      <SettingText :model-value="settings.memberName ?? ''" label="Your name" :maxlength="40" :disabled="locked" @save="(v) => saveName('memberName', v)" />
    </SettingRow>
    <SettingRow id="relationship" label="Your companion is" description="How it relates to you, in your words.">
      <SettingText :model-value="settings.relationshipStyle ?? ''" label="Your companion is" :maxlength="80" :disabled="locked" @save="(v) => saveName('relationshipStyle', v)" />
    </SettingRow>
    <SettingRow id="picture" label="Picture" description="Shown in Castle, and in the room when it is let in.">
      <div class="avatar"><img v-if="avatar" :src="avatar" alt="" /><span v-else>◆</span></div>
      <button :disabled="locked" @click="changeAvatar">Choose…</button>
      <button :disabled="locked" @click="clearAvatar">Use the drawn look</button>
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="daily" title="Daily life">
    <SettingRow id="mood" label="A new mood every day" :description="`Shows under the portrait and colours how ${companionLabel} talks.`" :tip="`Every morning ${companionLabel} wakes in a new mood, written from three words drawn at random, its traits and today's look. It shows under its portrait, colours how it talks, and stays the day - unless something truly moves it and it changes the mood itself. Each morning's mood is one short run on your own plan.`">
      <SettingSwitch :model-value="valueOf(settings, 'dailyMoodOn')" label="A new mood every day" :disabled="locked" @update:model-value="(on) => saveRow('mood', { dailyMoodOn: on })" />
    </SettingRow>
    <SettingRow id="keeper" label="Shopkeepers speak" :description="`In STUDIO's Marketplace, ${companionLabel} explains items as the keeper.`" :tip="`In STUDIO's Marketplace, a shop's keeper explains each item and answers your questions. ${companionLabel} speaks as the keeper, from the shop's public card and the item's public facts only, never more than the listing shows. Each explanation or answer is one short run on your own plan. Off, the item's own description shows instead.`">
      <SettingSwitch :model-value="valueOf(settings, 'keeperSpeaks')" label="Shopkeepers speak" :disabled="locked" @update:model-value="(on) => saveRow('keeper', { keeperSpeaks: on })" />
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="local-voice" title="Local voice" note="Optional" :tip="`For people who run their own VoiceStudio on ${words.thisComputer}. Off: Castle never contacts it. On: STUDIO's Voice card can speak a line with it, and the clip is saved with your other voice lines. Everything stays on this computer. Castle does not install or start VoiceStudio, and its voice models carry their own licences (some are non-commercial).`">
    <SettingRow id="voicestudio" label="Use VoiceStudio">
      <template #description>For STUDIO's Voice card, if you run it on {{ words.thisComputer }}. <a href="#" @click.prevent="openUrl('https://github.com/debpalash/voicestudio')">What is VoiceStudio?</a></template>
      <SettingSwitch :model-value="valueOf(settings, 'voicestudioOn')" label="Use VoiceStudio" :disabled="locked" @update:model-value="toggleVoice" />
    </SettingRow>
    <SettingRow
      id="voicestudio-status"
      label="VoiceStudio"
      :disabled="!settings.voicestudioOn"
      :description="!settings.voicestudioOn ? 'Turn on Use VoiceStudio to check it.' : voiceUp === null ? 'Being checked…' : voiceUp ? 'Running on this computer.' : 'Not running. Open it, let it finish starting, then check again.'"
    >
      <SettingPill v-if="settings.voicestudioOn && voiceUp !== null" :tone="voiceUp ? 'good' : 'warn'">{{ voiceUp ? 'Running' : 'Not found' }}</SettingPill>
      <button :disabled="!settings.voicestudioOn" @click="checkVoice">Check again</button>
    </SettingRow>
  </SettingGroup>
</template>

<style scoped src="./settings-page.css"></style>
