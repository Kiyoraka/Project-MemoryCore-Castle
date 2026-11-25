// The Settings page's shared state, provided by SettingsView.vue and injected by its panes
// (CompanionPane, ChatPane, RoomsPane, KeysPane): the settings, the keys, the footer's words and the
// two ways a change is saved. The row status stays ROW_STATUS (rowstatus.js).
export const SETTINGS_PAGE = Symbol('settings page')
