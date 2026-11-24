// The window layout keys, while Castle is in front. macOS: Ctrl+Cmd+F full screen (the Mac
// standard), Ctrl+Opt+Return full width, Ctrl+Opt+Left / Right the halves (as Rectangle and
// Magnet do). Windows: F11 full screen; Win+arrows stay with Windows. Esc leaves full screen.
// Gives what to ask castle_window_layout for, or null.
export function layoutForKey(event, { mac, layout }) {
  if (event.defaultPrevented || event.repeat) return null
  if (event.key === 'Escape') {
    return layout === 'fullscreen' && !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey ? 'leave-fullscreen' : null
  }
  if (!mac) return event.key === 'F11' && !event.ctrlKey && !event.altKey && !event.metaKey ? 'toggle-fullscreen' : null
  if (event.shiftKey) return null
  if (event.ctrlKey && event.metaKey && !event.altKey && event.code === 'KeyF') return 'toggle-fullscreen'
  if (event.ctrlKey && event.altKey && !event.metaKey) {
    if (event.key === 'Enter') return 'full'
    if (event.key === 'ArrowLeft') return 'left'
    if (event.key === 'ArrowRight') return 'right'
  }
  return null
}

// The shortcut shown beside each item in the window size menu.
export function layoutHint(id, mac) {
  if (!mac) return id === 'fullscreen' ? 'F11' : ''
  return { fullscreen: '⌃⌘F', full: '⌃⌥↩', left: '⌃⌥←', right: '⌃⌥→' }[id] || ''
}
