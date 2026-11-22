// What the Database panel and the HOME card say, worked out from the two things Castle's core
// reports: the engine ({ state, port, external, answering }) and this memory's own database
// ({ name, reachable, movedIn, canRestore }). Pure, so it is tested without a window.
import { isMac } from '../platform.js'

// How the words name this computer: 'this Mac' on a Mac, 'this PC' elsewhere.
export function thisComputer(mac = isMac) {
  return mac ? 'this Mac' : 'this PC'
}

// The four steps, in order, each done or not.
export function steps(engine, memory, mac = isMac) {
  const state = engine?.state
  const installed = state === 'stopped' || state === 'running' || state === 'external'
  const running = state === 'running' || (state === 'external' && engine.answering)
  return [
    { id: 'install', label: state === 'external' ? `Using the database already on ${thisComputer(mac)}` : 'Database installed', ok: installed },
    { id: 'start', label: 'Database running', ok: running },
    { id: 'create', label: memory?.name ? `${memory.name} created` : 'Your memory database created', ok: running && !!memory?.name && !!memory?.reachable },
    { id: 'move', label: 'Memory moved in', ok: running && !!memory?.movedIn },
  ]
}

// The first step still open, or null when everything is done.
export function nextStep(engine, memory) {
  return steps(engine, memory).find((step) => !step.ok)?.id ?? null
}

// "95 MB" from a byte count.
export function megabytes(bytes) {
  return `${Math.round((bytes ?? 0) / (1024 * 1024))} MB`
}

// How Install gets the engine, in words: the download's size, or Homebrew on a Mac. An
// interrupted download is said so, with how much of it is already here.
export function installLine(view) {
  if (view?.source === 'homebrew') return 'Installed with Homebrew'
  if (view?.partialBytes > 0) return `${megabytes(view.partialBytes)} of ${megabytes(view.downloadBytes)} already downloaded`
  return `About ${megabytes(view?.downloadBytes)}`
}

// The Install button's words: it carries on when an interrupted download is waiting.
export function installButton(view) {
  return view?.source !== 'homebrew' && view?.partialBytes > 0 ? 'Continue the download' : 'Install the database'
}

// "Downloading… 43%" while the engine downloads.
export function progressLine(done, total) {
  if (!total) return 'Downloading…'
  return `Downloading… ${Math.min(100, Math.floor((done / total) * 100))}%`
}

// What moving in will take, in one sentence.
export function moveLine(counts) {
  if (!counts) return ''
  const parts = [
    [counts.diary, 'diary page'],
    [counts.memory, 'memory file'],
    [counts.reminders, 'reminder'],
    [counts.projects, 'project'],
  ]
    .filter(([n]) => n > 0)
    .map(([n, word]) => `${n} ${word}${n === 1 ? '' : 's'}`)
  if (!parts.length) return 'There is nothing to move: this memory has no diary, memory files, reminders or projects in files.'
  return `${counts.files} ${counts.files === 1 ? 'file' : 'files'} will move: ${parts.join(', ')}.`
}

// The HOME card: one line and at most one button.
export function card(engine, memory, mac = isMac) {
  const next = nextStep(engine, memory)
  if (!engine) return { line: 'Checking…', button: null }
  if (next === 'install') return { line: 'Not set up yet.', button: { label: 'Set up', action: 'open' } }
  if (next === 'start') {
    return engine.state === 'external'
      ? { line: `The database on ${thisComputer(mac)} is stopped.`, button: { label: 'Open', action: 'open' } }
      : { line: 'Stopped.', button: { label: 'Start', action: 'start' } }
  }
  if (next === 'create') return { line: 'Running · no memory database yet.', button: { label: 'Open', action: 'open' } }
  if (next === 'move') return { line: 'Running · your memory is still in files.', button: { label: 'Open', action: 'open' } }
  return { line: `${memory.name} · running`, button: null }
}

// The words beside each step while "Set it up for me" runs.
export function setupStepLabel(step) {
  return (
    {
      install: 'Downloading and installing the database',
      start: 'Starting it',
      create: 'Creating your memory database',
      check: 'Checking what would move',
    }[step] ?? step
  )
}

// Whether to show the one-time offer: only to someone who had Castle before this version
// (an update, not a first run), whose memory folder is chosen, who has not answered yet, and
// who has no database of any kind - not installed, not an existing one chosen, none created.
export function shouldOffer({ offerSeen, hadCastleBefore, hasFolder, setupOpen }, engine, memory) {
  if (offerSeen || !hadCastleBefore || !hasFolder || setupOpen) return false
  if (!engine || engine.state !== 'notInstalled') return false
  return !memory?.name && !memory?.movedIn
}
