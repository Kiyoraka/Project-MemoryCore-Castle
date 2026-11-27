// SWEEP's state for the whole run: what can be freed, the sheet's ticks, a sweep as it happens and
// its result, Deep clean, and the gear. Measuring walks every group (it can take most of a minute on
// a full Temp), so it runs at start, after a sweep and every 30 minutes while a Sweep card is shown.
import { computed, ref } from 'vue'
import { diskDrives, onSweepProgress, sweepClean, sweepDeep, sweepMeasure, sweepSettingsSave, sweepStatus } from '../api/castle.js'
import { resultLine, startingTicks, totalOf } from '../components/widgets/sweep.js'
import { diskWarning } from '../components/widgets/sweepdev.js'

const REMEASURE_MS = 30 * 60 * 1000

const groups = ref([])
const measuring = ref(false)
const status = ref({ settings: { look: null, recycleBin: false }, lastSwept: null, deep: false })
const ticks = ref(new Set())
const cleaning = ref(false)
const freedSoFar = ref(0)
const result = ref('')
const deepBusy = ref(false)
const problem = ref('')
// the sheet is one for the whole window: HOME's card and the DEV room both open it
const sheetOpen = ref(false)
const sheetDeveloper = ref(false)
// "Disk 92% full" from 90% on the system drive, else null
const diskWarn = ref(null)
let shown = 0
let timer = null
let stopProgress = null

const total = computed(() => totalOf(groups.value))
const tickedTotal = computed(() => totalOf(groups.value, ticks.value))

async function measure() {
  if (measuring.value) return
  measuring.value = true
  try {
    groups.value = await sweepMeasure()
  } catch (error) {
    problem.value = String(error)
  } finally {
    measuring.value = false
  }
}

async function readDisk() {
  try {
    diskWarn.value = diskWarning(await diskDrives())
  } catch {
    // outside Castle's window
  }
}

async function readStatus() {
  try {
    status.value = await sweepStatus()
  } catch {
    // outside Castle's window
  }
}

// The sheet opens with the starting ticks for the groups measured now.
function openSheet() {
  ticks.value = startingTicks(groups.value, status.value.settings.recycleBin)
  result.value = ''
  problem.value = ''
}

// Opens the sheet; `developer` opens it at the Developer section (the DEV room's Free disk space…).
function showSheet({ developer = false } = {}) {
  openSheet()
  sheetDeveloper.value = developer
  sheetOpen.value = true
  readStatus()
  // the numbers may be half an hour old: measure again while the member reads the sheet
  if (!measuring.value && !cleaning.value) measure()
}

function closeSheet() {
  sheetOpen.value = false
  sheetDeveloper.value = false
}

function toggle(id) {
  const next = new Set(ticks.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  ticks.value = next
}

async function clean() {
  if (cleaning.value || !ticks.value.size) return
  cleaning.value = true
  freedSoFar.value = 0
  problem.value = ''
  try {
    stopProgress = await onSweepProgress(({ freed }) => (freedSoFar.value = freed)).catch(() => null)
    const done = await sweepClean([...ticks.value])
    result.value = resultLine(done)
  } catch (error) {
    problem.value = String(error)
  } finally {
    stopProgress?.()
    stopProgress = null
    cleaning.value = false
    await readStatus()
    await measure()
    readDisk()
  }
}

async function deep() {
  if (deepBusy.value) return
  deepBusy.value = true
  problem.value = ''
  try {
    const done = await sweepDeep()
    result.value = done ? `Deep clean: ${resultLine([done])}` : 'Deep clean was not allowed, so nothing changed.'
  } catch (error) {
    problem.value = String(error)
  } finally {
    deepBusy.value = false
    await readStatus()
  }
}

async function saveSettings(look, recycleBin) {
  try {
    status.value = await sweepSettingsSave(look, recycleBin)
  } catch (error) {
    problem.value = String(error)
  }
}

// A Sweep card calls this when it appears and the returned function when it goes.
function attach() {
  shown += 1
  if (shown === 1) {
    readStatus()
    if (!groups.value.length) measure()
    readDisk()
    timer = setInterval(() => {
      measure()
      readDisk()
    }, REMEASURE_MS)
  }
  return () => {
    shown -= 1
    if (shown === 0 && timer) {
      clearInterval(timer)
      timer = null
    }
  }
}

export function useSweep() {
  return { groups, measuring, status, ticks, cleaning, freedSoFar, result, deepBusy, problem, total, tickedTotal, sheetOpen, sheetDeveloper, diskWarn, measure, readDisk, openSheet, showSheet, closeSheet, toggle, clean, deep, saveSettings, attach }
}
