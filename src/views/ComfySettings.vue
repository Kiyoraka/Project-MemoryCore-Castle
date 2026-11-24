<script setup>
// Settings > ComfyUI: draw STUDIO's plain pictures with the member's own ComfyUI. Whether it is on,
// where it listens, whether it answers, the file that starts it, the checkpoint to draw with, a
// workflow of their own and how long Castle waits for one picture. Built from Settings rows
// (docs/settings-standard.md); every choice is kept by Castle's own command (comfy_save), which
// checks it and says why in plain words when it cannot.
import { computed, inject, onMounted, ref } from 'vue'
import { comfyCheck, comfyChooseFile, comfySave, comfyStart } from '../api/castle.js'
import GlassSelect from '../components/GlassSelect.vue'
import SettingGroup from '../components/settings/SettingGroup.vue'
import SettingPill from '../components/settings/SettingPill.vue'
import SettingRow from '../components/settings/SettingRow.vue'
import SettingSwitch from '../components/settings/SettingSwitch.vue'
import SettingText from '../components/settings/SettingText.vue'
import { ROW_STATUS, createRowStatus } from '../components/settings/rowstatus.js'
import { DEFAULT_ADDRESS, checkpointOptions, comfyOf, connectionLine, connectionPill, fileName, limitOptions, modelLine, startLine } from '../components/comfy.js'

const props = defineProps({
  settings: { type: Object, required: true },
})
const emit = defineEmits(['saved'])

// the page's row status, or one of its own when shown outside Settings
const rows = inject(ROW_STATUS, null) ?? createRowStatus()
const status = ref(null)
const checking = ref(false)
const starting = ref(false)
const busy = ref(false)
const gear = computed(() => comfyOf(props.settings))
const pill = computed(() => connectionPill(status.value, checking.value))

/** Asks the saved address whether ComfyUI answers; never starts it. */
async function check() {
  checking.value = true
  rows.clear('comfy-connection')
  try {
    status.value = await comfyCheck()
    // a workflow file that went wrong since it was saved is said under its own row
    if (status.value.workflowProblem) rows.failed('comfy-workflow', status.value.workflowProblem, 'Cannot use it')
    else rows.clear('comfy-workflow')
  } catch (reason) {
    status.value = null
    rows.failed('comfy-connection', reason, 'Not checked')
  } finally {
    checking.value = false
  }
}

/** Keeps one change (with everything else as it is); the page takes it only once Castle has kept it. */
async function save(row, patch, recheck = true) {
  busy.value = true
  const next = { ...gear.value, ...patch }
  try {
    const ok = await rows.run(row, async () => {
      await comfySave(next.on, next.address, next.start, next.model, next.workflow, next.limitMinutes)
      return true
    })
    if (ok) {
      emit('saved')
      if (recheck) await check()
    }
  } finally {
    busy.value = false
  }
}

async function choose(kind, row, field) {
  const path = await comfyChooseFile(kind)
  if (path) await save(row, { [field]: path })
}

/** Runs the start file if nothing answers, waits for ComfyUI to come up, then reads its state. */
async function startNow() {
  starting.value = true
  rows.clear('comfy-start-now')
  try {
    status.value = await comfyStart()
  } catch (reason) {
    rows.failed('comfy-start-now', reason, 'Not started')
  } finally {
    starting.value = false
  }
}

onMounted(() => {
  if (gear.value.on) check()
})
</script>

<template>
  <SettingGroup id="comfy" title="Your ComfyUI" tip="ComfyUI is a free picture program you run yourself, on this computer or on another one. With this on, Castle sends the plain text pictures from STUDIO to it first. A picture that follows a reference picture, and APPEARANCE's looks, still go to Codex or your OpenAI key. Nothing leaves your own network unless you give the address of a computer elsewhere.">
    <SettingRow id="comfy-on" label="Draw with ComfyUI" description="Plain text pictures go to your ComfyUI first.">
      <SettingSwitch :model-value="gear.on" label="Draw with ComfyUI" :disabled="busy" @update:model-value="(on) => save('comfy-on', { on })" />
    </SettingRow>
    <SettingRow id="comfy-address" label="Address" description="Where ComfyUI listens. Empty means this computer.">
      <SettingText :model-value="gear.address" label="Address" :placeholder="DEFAULT_ADDRESS" :maxlength="200" :disabled="busy" @save="(address) => save('comfy-address', { address })" />
    </SettingRow>
    <SettingRow id="comfy-connection" label="Connection" :description="connectionLine(status)">
      <SettingPill :tone="pill.tone">{{ pill.text }}</SettingPill>
      <button :disabled="checking || busy" @click="check">Check</button>
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="comfy-starting" title="Starting ComfyUI" tip="Castle can start ComfyUI for you when it is not running, and closes it again only if Castle started it. Choose the file you start ComfyUI with (on Windows, the .bat in its folder); Castle runs it hidden, from its own folder. ComfyUI's usual start file opens a browser tab every time it starts: use a copy with --disable-auto-launch added to stop that. Other flags, such as --disable-smart-memory, depend on your graphics card and your model.">
    <SettingRow id="comfy-start" label="Start file" :description="startLine(gear)" tip="Castle runs this only when ComfyUI is not already running, and closes ComfyUI when Castle closes, but only if Castle started it. Without a start file, you start ComfyUI yourself before drawing.">
      <SettingText :model-value="gear.start" label="Start file" placeholder="Not set" :maxlength="1000" :disabled="busy" @save="(start) => save('comfy-start', { start })" />
      <button :disabled="busy" @click="choose('start', 'comfy-start', 'start')">Choose…</button>
    </SettingRow>
    <SettingRow id="comfy-start-now" label="Start it now" :description="starting ? 'Starting… this can take a minute or two.' : 'Runs the start file and waits for ComfyUI to answer.'">
      <SettingPill v-if="status?.startedByCastle" tone="good">Started by Castle</SettingPill>
      <button :disabled="starting || busy || !gear.start.trim()" @click="startNow">{{ starting ? 'Starting…' : 'Start' }}</button>
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="comfy-draw" title="What it draws with" tip="Without a workflow of your own, Castle draws with one plain workflow and the checkpoint you pick here. A workflow of your own keeps your own model, custom nodes and settings, so the checkpoint is ignored. Time limit: a big model on a modest graphics card can take minutes, so Castle waits this long for one picture before it stops.">
    <SettingRow id="comfy-model" label="Checkpoint" :description="modelLine(gear, status)" :disabled="!!gear.workflow">
      <GlassSelect :model-value="gear.model" :options="checkpointOptions(status, gear.model)" :disabled="busy || !!gear.workflow" aria-label="Checkpoint" @update:model-value="(model) => save('comfy-model', { model }, false)" />
    </SettingRow>
    <SettingRow id="comfy-workflow" label="Your own workflow" :description="gear.workflow ? fileName(gear.workflow) : 'Empty uses the plain workflow Castle brings.'" tip="Save your workflow from ComfyUI with Workflow > Export (API). Where the words to draw go, put {{prompt}}: type it into your prompt node's text box before you export, or edit the file in a text editor. It may appear in several places. To let STUDIO's size and quality count, put {{width}}, {{height}}, {{seed}} and {{steps}} in the matching number fields of the file; any you leave out keep your own numbers. A picture the workflow must follow (img2img) is not supported yet.">
      <SettingText :model-value="gear.workflow" label="Your own workflow" placeholder="Not set" :maxlength="1000" :disabled="busy" @save="(workflow) => save('comfy-workflow', { workflow })" />
      <button :disabled="busy" @click="choose('workflow', 'comfy-workflow', 'workflow')">Choose…</button>
    </SettingRow>
    <SettingRow id="comfy-limit" label="Time limit" description="How long Castle waits for one picture.">
      <GlassSelect :model-value="gear.limitMinutes" :options="limitOptions(gear.limitMinutes)" :disabled="busy" aria-label="Time limit" @update:model-value="(limitMinutes) => save('comfy-limit', { limitMinutes }, false)" />
    </SettingRow>
  </SettingGroup>
</template>

<style scoped>
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
button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
</style>
