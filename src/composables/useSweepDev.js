// SWEEP's Developer section, its state for the whole run: the last scan, the picks, a clean as it
// happens and its result. The scan reads the DEV projects, their worktrees and what is in use, and
// sizes the build folders (it can take a minute on a big disk), so it runs only when asked: the
// section opened with nothing scanned yet, or Scan again.
import { computed, ref } from 'vue'
import { onSweepDevProgress, sweepDevClean, sweepDevScan } from '../api/castle.js'
import { devResultLine, picksToSend, startingPicks, tickedTotal } from '../components/widgets/sweepdev.js'

const scan = ref(null)
const scanning = ref(false)
const picks = ref({})
const cleaning = ref(false)
const progress = ref({ index: 0, count: 0, freed: 0 })
const result = ref('')
const problem = ref('')

const items = computed(() => scan.value?.items ?? [])
const total = computed(() => tickedTotal(items.value, picks.value))

async function rescan() {
  if (scanning.value || cleaning.value) return
  scanning.value = true
  problem.value = ''
  try {
    const next = await sweepDevScan()
    // a pick the member made stays when its item is still offered the same way
    const before = picks.value
    const start = startingPicks(next.items)
    for (const item of next.items) {
      const kept = before[item.id]
      if (kept && !item.blocked && (kept === 'leave' || kept === 'clean' || item.choices?.includes(kept))) start[item.id] = kept
    }
    scan.value = next
    picks.value = start
  } catch (error) {
    problem.value = String(error)
  } finally {
    scanning.value = false
  }
}

function setPick(id, how) {
  picks.value = { ...picks.value, [id]: how }
}

async function clean() {
  const sent = picksToSend(items.value, picks.value)
  if (cleaning.value || !sent.length) return
  cleaning.value = true
  result.value = ''
  problem.value = ''
  progress.value = { index: 0, count: sent.length, freed: 0 }
  let stop = null
  try {
    stop = await onSweepDevProgress(({ index, freed }) => (progress.value = { index, count: sent.length, freed })).catch(() => null)
    result.value = devResultLine(await sweepDevClean(sent))
  } catch (error) {
    problem.value = String(error)
  } finally {
    stop?.()
    cleaning.value = false
    // after a clean the sizes are scanned again; picks that went start fresh
    picks.value = {}
    await rescan()
  }
}

export function useSweepDev() {
  return { scan, scanning, picks, cleaning, progress, result, problem, items, total, rescan, setPick, clean }
}
