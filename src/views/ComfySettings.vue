<script setup>
// Settings > ComfyUI: draw STUDIO's plain pictures with the member's own ComfyUI. Whether it is on,
// where it listens, whether it answers, the file that starts it, the checkpoint to draw with, a
// workflow of their own and how long Castle waits for one picture. Built from Settings rows
// (docs/settings-standard.md); every choice is kept by Castle's own command (comfy_save), which
// checks it and says why in plain words when it cannot.
import { computed, inject, onMounted, ref } from 'vue'
import { comfyBind, comfyCheck, comfyChooseFile, comfySave, comfyScan, comfyStart } from '../api/castle.js'
import GlassSelect from '../components/GlassSelect.vue'
import SettingGroup from '../components/settings/SettingGroup.vue'
import SettingPill from '../components/settings/SettingPill.vue'
import SettingRow from '../components/settings/SettingRow.vue'
import SettingSwitch from '../components/settings/SettingSwitch.vue'
import SettingText from '../components/settings/SettingText.vue'
import { ROW_STATUS, createRowStatus } from '../components/settings/rowstatus.js'
import { DEFAULT_ADDRESS, FIELD_GROUPS, afterSwitch, bindingsLine, checkpointOptions, comfyOf, connectionLine, connectionPill, fieldKey, fieldLabel, fieldNote, fileName, initialPicks, limitOptions, modelLine, picksToBindings, startLine, toggled } from '../components/comfy.js'

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
const pill = computed(() => connectionPill(status.value, checking.value, starting.value))

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
  return status.value
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
      // another workflow file is another list: the open one no longer describes it
      if ('workflow' in patch) scan.value = null
      emit('saved')
      if (recheck) await check()
    }
    return ok
  } finally {
    busy.value = false
  }
}

/** Keeps the switch, then looks for ComfyUI and starts it when it was switched on and is not there. */
async function setOn(on) {
  if (await save('comfy-on', { on }, false)) await afterSwitch(on, gear.value.start, { check, start: startNow })
}

async function choose(kind, row, field) {
  const path = await comfyChooseFile(kind)
  if (path) await save(row, { [field]: path })
}

/** Runs the start file if nothing answers, waits for ComfyUI to come up, then reads its state. */
async function startNow() {
  if (starting.value) return
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

// ---- Import: which inputs of the member's own workflow take Castle's values ----
// the placeholder, kept out of the template where Vue would read its braces as its own
const PROMPT_MARK = '{{prompt}}'
const scan = ref(null)
const picks = ref({})
const importing = ref(false)
const pickGroups = computed(() => FIELD_GROUPS.map((group) => ({ ...group, fields: scan.value?.[group.id] ?? [] })).filter((group) => group.fields.length))
// the words need somewhere to go: a ticked field, or a {{prompt}} the file already holds
const canUse = computed(() => !!scan.value && ((picks.value.prompt ?? []).length > 0 || scan.value.placeholders.prompt > 0))

/** Reads the saved workflow and opens the list; nothing is changed until "Use these". */
async function importFields() {
  importing.value = true
  rows.clear('comfy-fields')
  try {
    scan.value = await comfyScan()
    picks.value = initialPicks(scan.value)
  } catch (reason) {
    scan.value = null
    rows.failed('comfy-fields', reason, 'Not read')
  } finally {
    importing.value = false
  }
}

async function useFields() {
  busy.value = true
  try {
    const ok = await rows.run('comfy-fields', async () => {
      await comfyBind(picksToBindings(scan.value, picks.value))
      return true
    })
    if (ok) {
      scan.value = null
      emit('saved')
      await check()
    }
  } finally {
    busy.value = false
  }
}

// looked at on every visit, switched on or not, so a ComfyUI that is running never looks stopped
onMounted(check)
</script>

<template>
  <SettingGroup id="comfy" title="Your ComfyUI" tip="ComfyUI is a free picture program you run yourself, on this computer or on another one. With this on, Castle sends the plain text pictures from STUDIO to it first. A picture that follows a reference picture, and APPEARANCE's looks, still go to Codex or your OpenAI key. Nothing leaves your own network unless you give the address of a computer elsewhere.">
    <SettingRow id="comfy-on" label="Draw with ComfyUI" description="Plain text pictures go to your ComfyUI first. Switching on starts it if needed.">
      <SettingSwitch :model-value="gear.on" label="Draw with ComfyUI" :disabled="busy" @update:model-value="setOn" />
    </SettingRow>
    <SettingRow id="comfy-address" label="Address" description="Where ComfyUI listens. Empty means this computer.">
      <SettingText :model-value="gear.address" label="Address" :placeholder="DEFAULT_ADDRESS" :maxlength="200" :disabled="busy" @save="(address) => save('comfy-address', { address })" />
    </SettingRow>
    <SettingRow id="comfy-connection" label="Connection" :description="connectionLine(status)">
      <SettingPill :tone="pill.tone">{{ pill.text }}</SettingPill>
      <button :disabled="checking || busy" @click="check">Check</button>
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="comfy-starting" title="Starting ComfyUI" tip="Castle can start ComfyUI for you when it is not running (when you switch drawing on, press Start, or draw), and closes it again only if Castle started it. Choose the file you start ComfyUI with (on Windows, the .bat in its folder); Castle runs it hidden, from its own folder. ComfyUI's usual start file opens a browser tab every time it starts: use a copy with --disable-auto-launch added to stop that. Other flags, such as --disable-smart-memory, depend on your graphics card and your model.">
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
    <SettingRow id="comfy-workflow" label="Your own workflow" :description="gear.workflow ? fileName(gear.workflow) : 'Empty uses the plain workflow Castle brings.'" tip="Save your workflow from ComfyUI with Workflow > Export (API), then choose the file here. Castle never changes your file. Next, press Import below to say which fields take the words to draw and, if you like, Studio's size and a new seed. A picture the workflow must follow (img2img) is not supported yet.">
      <SettingText :model-value="gear.workflow" label="Your own workflow" placeholder="Not set" :maxlength="1000" :disabled="busy" @save="(workflow) => save('comfy-workflow', { workflow })" />
      <button :disabled="busy" @click="choose('workflow', 'comfy-workflow', 'workflow')">Choose…</button>
    </SettingRow>
    <SettingRow id="comfy-limit" label="Time limit" description="How long Castle waits for one picture.">
      <GlassSelect :model-value="gear.limitMinutes" :options="limitOptions(gear.limitMinutes)" :disabled="busy" aria-label="Time limit" @update:model-value="(limitMinutes) => save('comfy-limit', { limitMinutes }, false)" />
    </SettingRow>
  </SettingGroup>

  <SettingGroup v-if="gear.workflow" id="comfy-words" title="Where Castle's words go" tip="Press Import and Castle reads your workflow file and lists its text and number fields, with the most likely prompt first and a negative prompt marked. Tick every field the words to draw should go into. Tick Width and Height to use Studio's size, Seed for a new picture each time, and Steps to use Studio's quality as the number of steps. A field you leave unticked keeps your own value. Your file is never changed. Instead of ticking, you can put {{prompt}}, {{width}}, {{height}}, {{seed}} and {{steps}} into the file itself.">
    <SettingRow id="comfy-fields" label="Choose the fields" :description="bindingsLine(gear.bindings)">
      <button :disabled="importing || busy" @click="importFields">{{ importing ? 'Reading…' : 'Import' }}</button>
      <template #below>
        <div v-if="scan" class="import">
          <section v-for="group in pickGroups" :key="group.id" class="group">
            <p class="group-title">{{ group.title }} <small>{{ group.note }}</small></p>
            <div v-for="field in group.fields" :key="fieldKey(field)" class="field">
              <SettingSwitch :model-value="(picks[group.id] ?? []).includes(fieldKey(field))" :label="`${group.title}: ${fieldLabel(field)}`" :disabled="busy" @update:model-value="(on) => (picks = toggled(picks, group.id, field, on))" />
              <span class="field-name">{{ fieldLabel(field) }}</span>
              <span class="field-note">{{ fieldNote(field) }}</span>
            </div>
          </section>
          <p v-if="!scan.prompt.length" class="plain">No text field was found in this workflow. Put {{ PROMPT_MARK }} into its prompt, then press Import again.</p>
          <p v-else-if="!canUse" class="plain">Tick at least one field for the words to draw.</p>
          <div class="actions">
            <button :disabled="busy || !canUse" @click="useFields">Use these</button>
            <button :disabled="busy" @click="scan = null">Cancel</button>
          </div>
        </div>
      </template>
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
.import {
  display: grid;
  gap: 10px;
  margin-top: 6px;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--widget-background);
}
.group-title {
  margin: 0 0 4px;
  font-size: 12.5px;
  font-weight: 600;
}
.group-title small {
  margin-left: 6px;
  font-weight: 400;
  color: var(--muted, var(--text));
}
.field {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 3px 0;
  min-width: 0;
}
.field-name {
  font-size: 12.5px;
  white-space: nowrap;
}
.field-note {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--muted, var(--text));
}
.plain {
  margin: 0;
  font-size: 12.5px;
  color: var(--warn, var(--text));
}
.actions {
  display: flex;
  gap: 8px;
}
</style>
