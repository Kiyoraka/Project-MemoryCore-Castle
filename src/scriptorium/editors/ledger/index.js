// Ledger's ribbon and keys. The Ledger lane owns this file; the room reads it through ribbon.js.
//
//   tabs     [{ name, groups: [{ label, buttons: [button] }] }]   the ribbon (Classic) and the toolbar (Studio)
//   panels   [{ id, icon, label, groups: [{ label, buttons }] }]  Studio's inspector cards
//   intents  { 'edit.insert_text': { command: 'ledger.insert_text', args: {} }, ... }
//            the neutral key intents (input.js) this editor answers, as its own bus commands; the room
//            adds the selection as args.sel. caret.* and select.all need no entry (the room moves the
//            caret through the editing ground) unless the editor wants its own command for them.
//
//   button   { icon, title, command, args, stateKey }
//            icon: a name in icons.js or inline SVG markup (24x24, currentColor); command: a bus command
//            ('ledger.bold'), or `intent` in its place for a neutral one ('find.open'); args: merged with
//            { sel }; stateKey: the key in the editor's state JSON that shows the button pressed.

export const tabs = [
  { name: 'Home', groups: [] },
  { name: 'Insert', groups: [] },
  { name: 'Formulas', groups: [] },
  { name: 'Data', groups: [] },
  { name: 'View', groups: [] },
]

export const panels = []

export const intents = {}

export default { tabs, panels, intents }
