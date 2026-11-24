<script setup>
// Settings > Sentinel: on and off, the model the companion judges on (the recommended one, or a
// name the member types), how often it patrols, the one line that sets it up (run by the member
// in a PowerShell opened as administrator - Castle never asks for admin rights itself), and the
// line that removes it, in the Danger zone. Built from Settings rows (docs/settings-standard.md).
import { computed, inject, onMounted, ref } from 'vue'
import { sentinelSettingsSave, sentinelSetup } from '../api/castle.js'
import GlassSelect from '../components/GlassSelect.vue'
import SettingChoice from '../components/settings/SettingChoice.vue'
import SettingGroup from '../components/settings/SettingGroup.vue'
import SettingRow from '../components/settings/SettingRow.vue'
import SettingSwitch from '../components/settings/SettingSwitch.vue'
import SettingText from '../components/settings/SettingText.vue'
import { ROW_STATUS, createRowStatus } from '../components/settings/rowstatus.js'
import { MAC_LINE } from '../components/sentinel.js'

const props = defineProps({
  settings: { type: Object, required: true },
})
const emit = defineEmits(['saved'])

// the page's row status, or one of its own when shown outside Settings
const rows = inject(ROW_STATUS, null) ?? createRowStatus()
const setup = ref(null)
const busy = ref('')
const said = ref('')
const gear = computed(() => props.settings.sentinel || { on: false, model: null, patrolMinutes: 60, network: false })
const modelChoice = ref(gear.value.model ? 'own' : 'recommended')
const minutes = ref(gear.value.patrolMinutes || 60)
const companion = computed(() => props.settings.companionName ?? 'your companion')
const recommendedText = computed(() => (setup.value?.recommended ? `${setup.value.recommended} (recommended for ${setup.value.cliLabel})` : `${setup.value?.cliLabel ?? 'your CLI'}'s own model (recommended)`))
const PATROLS = [15, 30, 60, 120, 240, 720].map((m) => ({ value: m, label: m < 60 ? `${m} minutes` : m === 60 ? 'hour' : `${m / 60} hours` }))

async function read(message = '') {
  busy.value = 'Looking…'
  rows.clear('sentinel-installed')
  try {
    setup.value = await sentinelSetup()
    said.value = message
  } catch (reason) {
    rows.failed('sentinel-installed', reason, 'Not checked')
  } finally {
    busy.value = ''
  }
}

async function save(row, patch) {
  busy.value = 'Saving…'
  said.value = ''
  const next = { on: gear.value.on, model: gear.value.model, patrolMinutes: gear.value.patrolMinutes, network: !!gear.value.network, ...patch }
  try {
    const ok = await rows.run(row, async () => {
      await sentinelSettingsSave(next.on, next.model, next.patrolMinutes, next.network)
      return true
    })
    if (ok) emit('saved')
  } finally {
    busy.value = ''
  }
}

function chooseModel(choice) {
  modelChoice.value = choice
  if (choice === 'recommended') save('sentinel-model', { model: null })
}

async function copy(row, text) {
  rows.clear(row)
  try {
    await navigator.clipboard.writeText(text)
    said.value = 'Copied. Paste it into a PowerShell opened as administrator.'
  } catch {
    rows.failed(row, 'select the line and copy it by hand.', 'Not copied')
  }
}

onMounted(() => read())
</script>

<template>
  <SettingGroup v-if="setup && !setup.supported" id="watch" title="Keep watch">
    <SettingRow id="unsupported" label="Not on this computer" :description="MAC_LINE" />
  </SettingGroup>
  <template v-else>
    <SettingGroup id="watch" title="Keep watch" :tip="`${companion} keeps watch over this computer: the antivirus, the firewall, what listens for connections, what starts with Windows, scheduled tasks, new services, accounts and sign-ins. A small watch runs every five minutes, even when Castle is closed, and switches the firewall and protection back on if they are found off. ${companion} looks over what is new on each patrol and says one sentence on the shield. Nothing about this computer leaves it.`">
      <SettingRow id="sentinel-on" :label="`Let ${companion} keep watch`" description="Off stops the patrols. The watch runs until you remove it.">
        <SettingSwitch :model-value="!!gear.on" :label="`Let ${companion} keep watch`" :disabled="!!busy" @update:model-value="(on) => save('sentinel-on', { on })" />
      </SettingRow>
      <SettingRow id="sentinel-model" label="Model" :description="modelChoice === 'recommended' ? recommendedText : 'Any model your CLI knows.'" :tip="`Each patrol is one short run of ${companion} on your own plan, and only when something is open. The recommended model judges steadily at a fair cost; you can name any model your CLI knows.`">
        <SettingChoice :model-value="modelChoice" :options="[{ value: 'recommended', label: 'Recommended' }, { value: 'own', label: 'Another' }]" label="Model" :disabled="!!busy" @update:model-value="chooseModel" />
      </SettingRow>
      <SettingRow id="sentinel-model-name" label="Model name" :disabled="modelChoice !== 'own'" :description="modelChoice === 'own' ? 'For example opus.' : 'Choose Another to name one.'">
        <SettingText :model-value="gear.model || ''" label="Model name" placeholder="for example opus" :maxlength="64" :disabled="!!busy || modelChoice !== 'own'" @save="(name) => name.trim() && save('sentinel-model-name', { model: name })" />
      </SettingRow>
      <SettingRow id="sentinel-patrol" label="Patrol every" description="An alarm always starts a patrol at once.">
        <GlassSelect :model-value="minutes" :options="PATROLS" :disabled="!!busy" aria-label="Patrol every" @update:model-value="(m) => { minutes = m; save('sentinel-patrol', { patrolMinutes: m }) }" />
      </SettingRow>
    </SettingGroup>

    <SettingGroup id="sentinel-network" title="The Aegis Network" tip="One castle's sighting protects every castle. When the Sentinel catches an attacker (a burst of failed sign-ins, an address it blocked) or the antivirus names a detection, it shares that one address or name with the Foundation, and nothing else. An address two castles reported within a day goes on a shared list; every castle that joined blocks it for a day, and you can undo any block. A detection on the list is only a warning.">
      <SettingRow
        id="sentinel-network"
        label="Join the Aegis Network"
        :disabled="!setup?.signedIn && !gear.network"
        :description="setup?.signedIn || gear.network ? 'Shares only an attacker’s address or a detection’s name, never anything about you.' : 'Sign in to the Foundation first: the network is for its members.'"
      >
        <SettingSwitch :model-value="!!gear.network" label="Join the Aegis Network" :disabled="!!busy || (!setup?.signedIn && !gear.network)" @update:model-value="(network) => save('sentinel-network', { network })" />
      </SettingRow>
    </SettingGroup>

    <SettingGroup id="sentinel-setup" title="Set it up" tip="The line installs a small helper where only an administrator can change it, and three scheduled tasks: the five-minute watch, a second look whenever the antivirus or the firewall raises an alarm, and the helper itself. The helper can only close this computer further - switch the firewall or protection back on, scan, update the antivirus, block an address that keeps knocking for a day - and, only when you press its button, block a port, switch off a task or a startup entry, or stop a program.">
      <SettingRow id="sentinel-installed" label="The watch" :description="setup?.installed?.line ?? 'Looking…'">
        <button :disabled="!!busy" @click="read('Checked.')">Check again</button>
      </SettingRow>
      <SettingRow v-if="setup?.installLine" id="sentinel-install" label="Install line" description="Paste it into PowerShell run as administrator, then press Enter." tip="Open PowerShell as administrator (right-click it, Run as administrator), paste this line and press Enter. Castle never asks for administrator rights itself.">
        <button @click="copy('sentinel-install', setup.installLine)">Copy the line</button>
        <template #below><div class="line"><code>{{ setup.installLine }}</code></div></template>
      </SettingRow>
    </SettingGroup>

    <SettingGroup v-if="setup?.uninstallLine" id="sentinel-remove" title="Danger zone" danger>
      <SettingRow id="sentinel-uninstall" label="Remove the Sentinel" description="Removes the helper, its tasks, its folders and its firewall rules.">
        <button class="danger" @click="copy('sentinel-uninstall', setup.uninstallLine)">Copy the line</button>
        <template #below><div class="line"><code>{{ setup.uninstallLine }}</code></div></template>
      </SettingRow>
    </SettingGroup>
  </template>
  <p v-if="said" class="said">{{ said }}</p>
</template>

<style scoped>
.line {
  margin-top: 4px;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--widget-background);
  font-size: 12px;
  overflow-wrap: anywhere;
  user-select: all;
}
.said {
  margin: 0;
  color: var(--good);
  font-size: 12.5px;
}
button {
  padding: 5px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}
button.danger {
  border-color: var(--bad);
  color: var(--bad);
}
button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
</style>
