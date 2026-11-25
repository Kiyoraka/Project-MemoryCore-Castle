// Types a line into the session and presses Enter, the way the quick-action buttons and the
// switch prompt do. Codex reads an Enter that arrives together with the text as part of a paste
// and leaves the text in its box unsent, so Enter follows after a short pause (Codex needs more
// than 50 ms; this leaves margin for a busy PC). Claude Code turns each picture path in the text
// into [Image #N], which takes a moment, and swallows an Enter that arrives meanwhile, so every
// picture path makes the pause longer.

export const ENTER = String.fromCharCode(13)
export const ENTER_DELAY_MS = 300
export const PICTURE_DELAY_MS = 700

// A picture path as typed into the box: a drive letter or a leading / or \, ending in a picture
// extension (any case), in quotes when it holds spaces. It begins the text or follows a space, or
// the ~ that opens a bracketed paste. The same rule as the core's enter_delay (messages.rs).
const PICTURE = /(?:^|[\s~])(?:"(?:[a-z]:|[/\\])[^"]*\.(?:png|jpe?g|gif|webp)"|(?:[a-z]:|[/\\])[^\s"]*\.(?:png|jpe?g|gif|webp)\b)/gi

/** The pause between the text and its Enter: 300 ms, and 700 ms more for each picture path in it. */
export function enterDelay(text) {
  const pictures = String(text ?? '').match(PICTURE)?.length ?? 0
  return ENTER_DELAY_MS + PICTURE_DELAY_MS * pictures
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

export async function typeLine(write, text, wait = sleep) {
  await write(text)
  await wait(enterDelay(text))
  await write(ENTER)
}
