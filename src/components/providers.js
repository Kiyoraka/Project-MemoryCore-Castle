// The CLIs a companion can think with - the screens' side of Rust's `settings::Cli`.
// `offered`: shown as a choice in Settings and Setup. Gemini (Google's Antigravity CLI, `agy`) and
// Grok are built but not offered until Chat can read their records (the plan's Phase 6, after
// their records have been read on a real install); a Castle whose settings already name one still
// shows it, so the choice never disappears from under the member.
// `measured`: the Usage card can read its limits.
export const PROVIDERS = [
  { id: 'claude', label: 'Claude Code', offered: true, measured: true },
  { id: 'codex', label: 'Codex', offered: true, measured: true },
  { id: 'gemini', label: 'Gemini', offered: false, measured: false },
  { id: 'grok', label: 'Grok', offered: false, measured: false },
]

/** The name shown for a provider id; an unknown id is shown as it is. */
export function providerLabel(cli) {
  return PROVIDERS.find((p) => p.id === cli)?.label ?? String(cli ?? '')
}

/** The choices for "Thinks with": the offered providers, and the current one when it is not. */
export function offeredProviders(current) {
  return PROVIDERS.filter((p) => p.offered || p.id === current)
}

/** Chat reads Claude Code's own screen: its answer buttons, its mode pills, its send-now key. */
export function readsClaudeScreen(cli) {
  return cli === 'claude'
}

/** Chat notices Codex asking for an approval on the Log screen. */
export function readsCodexAsk(cli) {
  return cli === 'codex'
}

/** The provider draws pictures by itself on its own plan, so no OpenAI key is asked for. */
export function drawsItself(cli) {
  return cli === 'codex'
}
