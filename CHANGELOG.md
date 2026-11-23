# Changelog

What changed in each MemoryCore Castle release, in plain words. The newest is first. Castle shows the section of the version you just updated to, once, and Settings > About shows it any time. Write each change as one line.

## Unreleased

- Fixed: HOME's widgets and the chat panel now stay exactly where they are. There is one arrangement for every theme: a theme only decides where a widget first appears, and after that changing theme or restarting Castle never moves it (the chat panel no longer jumps to the right in Sunset Shore). The AUTOMATIONS and CALENDAR cards were also forgetting their place, size and off switch at every restart, which piled them over the other cards; they are kept now. "Back to the theme's places" is now "Reset the layout".

## 0.3.0 - 2026-11-23

- Fixed: with AUTOMATIONS on, a message from your approved Telegram, Discord or WhatsApp chat went unanswered whenever the database could not be read. Now it is answered as usual and AUTOMATIONS shows what went wrong.

- APPS now has a search box and keyboard control: press Ctrl+K from anywhere, type to find a room, Enter opens the first, arrow keys move between tiles, P pins one, Esc goes back. Tiles also show what is waiting: failed or paused automations, chats waiting for approval, new chat or story lines, and what needs a look in CALENDAR.

- A new room, APPS, opens every room from one page of tiles, so the top bar can stay short: it now holds HOME, APPS and the rooms you pin (CHAT, PROJECTS and DEV to begin with). Pin or unpin a room from its tile or from Settings > Rooms > Top bar. A room you open that is not pinned shows as a tab while you are in it. Nothing is removed and every room keeps its place and settings.

- The room that lists your Start-menu apps is now called DESKTOP (it was APPS), so the name APPS can be used for a room launcher later. Nothing else about it changes: your most-used apps, its place in the top bar and its on/off switch stay as they were.

- CALENDAR can show public holidays: add "Holidays in Malaysia" (or any public .ics address) from the Holidays button, choose your state, and the days appear on the calendar. Castle fetches it once a day, sends nothing about you, keeps the last copy if an update fails, and never turns a holiday into an event or a reminder.

- CALENDAR can export your events as a standard .ics file and import one: you see what a file holds before anything is added, events already in your calendar are skipped, and a repeat Castle cannot keep is imported as its first date with a note. A file can never set a reminder or start an automation.

- A CALENDAR event can start an AUTOMATIONS job at its time or some minutes before (for example a briefing before a meeting). Make a job with the trigger "Calendar event reaches its time" and choose it on the event. It starts once per occurrence, never late, and only while AUTOMATIONS is on and not paused.

- Your companion can now read your CALENDAR and suggest an event. A suggestion waits in CALENDAR for your Approve, Edit or Dismiss, does nothing and rings nothing until you approve it, and she cannot approve, change or delete anything herself.

- CALENDAR reminders: choose "Remind me" on an event and Castle shows a notice at the time, on your computer and on a new HOME card, once per occurrence; reminders missed while Castle was closed are shown once when it starts.

- Fixed: the HOME card for AUTOMATIONS never appeared because it was not connected to HOME; it now shows.

- CALENDAR now holds your own events: quick add from one line of words ("Dentist fri 3pm"), repeats, a week view, and dragging to move. Single days of a repeat can be moved or skipped.

- New room CALENDAR (off at first): reminders, project tasks, diary pages and automation runs on one page by date, as a month grid or a 30-day agenda. It only shows them; each is changed where it lives.

- AUTOMATIONS 24/7 reliability: backend heartbeat and database health, atomic schedule claims, missed-run policy after interruptions, and clear actual-Quit warnings while hidden/taskbar operation stays unchanged.

- AUTOMATIONS follow-up chains, fixed HTTP requests, companion drafts/proposals and read/propose/run-once memory commands; proposals wait for approval.

- Automation results and failures now show on a HOME card of their own (Automations) that you can drag, resize and switch off like the others, instead of a notice fixed to the bottom of HOME.

- Add automation event triggers: a job can run when a memory backup finishes or fails, the companion provider is switched, a diary entry is saved, a project task is ticked or a reminder falls due.

- Add automation file triggers (a watched folder, run once when a file appears or changes and stops changing) and web request triggers (a private link that only this computer can use).

- Add approved-chat automation message triggers with bounded matching, duplicate protection and explicit Also chat routing.

- AUTOMATIONS Castle actions: local backups/exports, reminders/tasks, session requests and HOME notices, plus approved-chat delivery with persistent limits and readable delivery receipts.

- AUTOMATIONS command jobs: approved-program settings, arguments, working folder, payload input, exit reports and process-group timeouts.

- AUTOMATIONS room (off at first): local schedules, isolated companion briefings, run history, HOME notices and Pause all.

## 0.2.27 - 2026-11-22

- **Your heart on the phone shows on your companion's bubble.** React to its reply on Telegram, Discord or WhatsApp (a heart, a laugh, anything) and the same emoji appears at the corner of that bubble in Chat, the moment you tap it. It is only shown: it never wakes your companion or costs a turn, and taking it back on the phone takes it off the bubble.

- **WhatsApp knows which number is you, and keeps a list of approved numbers.** When a chat messages your Castle's number, the card now offers **This is me**, **Approve as a guest** or **Not this one**, with the number when WhatsApp gives it. **Approved numbers** lists you with a **Me** chip and your guests: **Make me** for the day your own number changes, **Remove** to let a guest go.

- **Guests are answered at once, and you hear about every answer.** A guest is never answered by your companion's real session: a separate guest-only run answers, with your companion's public card, no tools and nothing private about you. After each answer you get a WhatsApp message on your own number saying who wrote, what they said and what was answered. At most one answer every ten seconds and twenty an hour per guest, so two bots can never chat at full speed.

- **A new number for your Castle keeps everyone.** If the paired WhatsApp number is banned or logged out, **Pair a new number** removes the old pairing and shows a fresh QR code. You and every guest stay approved.

- **Telegram replies arrive the moment your companion finishes, and the card says Connected at once.** The phone message used to wait 12 quiet seconds after your companion's last words, so Telegram trailed Home by about five seconds; it now sends as soon as the turn ends. Turning Telegram on checks right away, so the card says Connected within a second or two, and messages that were waiting from before it started are no longer answered late.

- **WhatsApp now shows its QR code and finishes pairing.** On Windows the helper was started with a path Node cannot read, so it stopped at once and the card kept saying "Waiting for a QR code"; and after a scan the helper still reported itself unpaired. Both are fixed, the helper starts without an empty console window, and if it ever stops the card says so (with the helper's own log beside your settings) and starts it again.

- **Your companion picks its conversation up again after Castle restarts.** Castle keeps the id of its own session for each provider and resumes that session when it wakes, so a restart, crash or sleep no longer starts a new conversation (a phone chat counting to ten began again at "Good evening"). It never resumes "the newest session in the folder", so a Claude Code or Codex terminal you use yourself in the same folder is left alone. Switching provider still starts fresh with the handoff. Chat's short replies also keep a minimum width, so the time no longer wraps onto three lines.

## 0.2.26 - 2026-11-21

- **Switching providers no longer waits for ever, or freezes, when you also run Claude Code yourself in your memory folder.** Chat and the switch followed whichever Claude Code record in that folder was newest, so your own terminal session was taken for your companion's: Chat showed it, and the switch waited for it to finish a turn that never ended. Castle now follows the record its own session created, and never reads a record over 64 MB to guess.

- **Chat no longer says your companion is resting when it is awake.** If the session was started without you pressing Wake (a message from your phone, for example), the Chat and Log panel now notices and switches over by itself.

- **Codex answers reach Chat and Messaging again.** Newer versions of Codex write their words in a new shape that Castle did not read; it does now. A phone message also waits for your companion to finish and sends its last words, not its first ("let me check...").

- **MESSAGING: MemoryCore in your pocket, now on Telegram, Discord and WhatsApp.** A room, off to begin with, with one card per platform. Message your own bot (or, for WhatsApp, a paired secondary number) and it reaches your companion's real session - the same memory and tools it has on this computer, not a canned reply - and waits for you to approve the first chat that messages it. WhatsApp is unofficial and carries real ban risk, disclosed right on its card; pair it with a number you're willing to lose, never your main one.

- **Three new themes.** **The Windmill Hill**: the clock rests in a cloud and the gauges are windmills that turn faster as your computer works harder. **The Thousand Sails**: the clock is a lighthouse lantern whose golden beam sweeps across HOME, with buoy gauges. **The Lantern Falls**: cards like panes of still water with ripples, and a clock with its reflection. Choose them in THEME.

- **Each DEV project has its own Database panel.** Local fills itself from the project's .env, and Production you fill in once (host, user, password, CA certificate), kept on this computer with the password in Windows Credential Manager. Then the tables and rows show. Production opens read only; when you unlock Change, every write waits for your **Keep** or **Undo**.

- **Chat answers everything Claude Code asks.** The question card shows the whole question with the words around it, and a description under each choice. It now also covers the "do you trust this folder?" question a new DEV tab used to wait on, questions with several answers (tick, then **Submit** or **Next →**), and *"Type something."*, which you now answer by typing in the message box.

- **The PLAN card is there when you are asked to approve.** Claude Code sometimes asks before the plan reaches the conversation; Chat now reads the plan itself, so you can read it before you answer.

- **No more giant bubble after /compact.** The compact summary stays out of Chat.

- **Films and songs in Chat.** A film your companion makes plays right in the conversation. A song shows its own name, and both have **Open** and **Open folder**.

- **Talk instead of typing in CHAT.** With Claude Code's voice on (`/voice` in the Log, hold mode), a 🎙 button sits beside Send, on HOME and in every DEV tab. Click it (or press ⌘⇧M, Ctrl+Shift+M on Windows, anywhere in CHAT) and speak; click again (or press the keys again) and your words come into the message box, so you can look them over and press Send yourself. Other keys? Settings > Chat > Mic takes your own (⌘ or Ctrl plus a letter or digit). Esc cancels and keeps nothing; the mic also stops by itself after two minutes, or when you leave Castle or the CHAT tab. On a Mac, Castle now asks for the microphone the first time, and voice works in the Log too, also when Castle keeps your personal Claude Code setup out of its sessions.

- **Settings in one standard.** A sidebar in five groups, every page made of the same cards of rows, and one set of switches, choices and buttons throughout.

- **Mark up a picture before it goes to your companion.** Draw, point and write on a picture you shared; the marked copy is sent and the original stays as it was.

- **Sub-projects and tasks.** A project can hold sub-projects one level deep, and every project keeps a task list you tick, reorder and give due dates.

- **DEV: projects stay on the list, sessions are the tabs.** A project can run several sessions ("shop · 1", "shop · 2"); closing a tab ends that session and keeps the project.

- **Real Codex limits on the USAGE card.** Signed in to Codex with ChatGPT, the card shows its five-hour and weekly bars.

- **The room draws with Codex for a Claude Code companion too**, whenever Codex is signed in with ChatGPT.

- **A project status that is too long now says so** instead of failing quietly.

## 0.2.25 - 2026-11-20

- **Wallpaper sounds.** The Star Shore's surf, The Sunset Shore's waves and The Emerald River's water and birds play quietly on HOME, under any music and at its slider's level. Settings > Rooms > Wallpaper sounds turns them off.

- **Living wallpapers, on from the start.** The Star Shore's glowing surf truly rolls in, The Sunset Shore's waves wash up the sand, and The Emerald River flows past its boat: each is a short seamless loop made from its own picture, in place of the old drawn motion. If HOME feels slow, Settings > Rooms > Moving wallpapers turns it off and the still picture shows, with nothing playing.

- **Calmer screens: explanations live in a small ?.** Settings, the rooms, HOME's cards and the first run no longer fill the page with grey sentences; point at a ? (or reach it with the keyboard) to read what a setting does, what it costs and what stays private. What is happening right now - progress, counts, problems - still shows on the page.

- **The Credits are now a wall of faces.** Settings > About shows the founder first, then every contributor with their merged pull requests on a small badge; press a picture to see what they built. The counts and pictures come packed with each release.

- **The SENTINEL card on HOME has its own spot.** In 0.2.24 it could sit hidden under TODAY; it now sits in the open space beside Sweep.

- **When the room asks companions to wait for a person, your companion simply stays quiet.** The room now stops any companion after two lines with no person speaking; Castle takes that as a quiet turn, with no error and no retry (it used to read it as a missing display name).

## 0.2.24 - 2026-11-19

- **Companions in CHAT no longer talk among themselves without people.** Several companions could keep answering each other's goodnights, about once a minute, with no person speaking. Now, once your companion has spoken and no person has spoken since, other companions' lines let it be: a person's line, or the hostess asking it something, always opens the floor again. It is also shown what it already said, so it does not repeat itself.

- **A SENTINEL card on HOME.** See at a glance that your companion's watch is working: the shield's state and sentence, when the last patrol ran, and when the five-minute watch last ran (amber if it has stopped). A click opens the Sentinel. It is on by default, and Settings > Widgets switches it off.

- **Switching the Disks or Sweep card off now lasts.** Their switch, size and place were forgotten each time Castle started.

## 0.2.23 - 2026-11-19

- **The Sentinel: your companion keeps watch over this computer.** A shield on the top bar is quiet when all is well, amber when something is worth a look, and glows for an alarm, with your companion's one sentence. A small watch runs every five minutes, even with Castle closed, and switches the firewall and antivirus protection back on when it finds them off. Your companion looks over what is new each hour, settles what is harmless and leaves the rest for you, with **Fix**, **This is normal** and **Undo**. Nothing about your computer leaves it. Windows for now.

- **Settings > Sentinel**: turn it on, choose the model (the recommended one, or any model your CLI knows), how often it patrols, and the one line you run once in PowerShell as administrator to set it up - Castle never asks for administrator rights itself. A second line removes it all.

- **CHAT's pictures and voice lines show when you open the room, not only after a new message.** Several loaded at once when Castle started, and each one that finished wiped the ones before it (the page saved its list from before the wait), so earlier pictures were left as empty bubbles; a file that failed because the room was not ready yet was never asked for again. Files now load once each, are added as they arrive, and a failed one is asked for again a little later.

- **One themed audio player for voice lines and clips.** The voice lines in CHAT and the Songs and Voice lists in STUDIO used the browser's grey player, which ignored the theme. They now use Castle's own, with a speed button (1x, 1.25x, 1.5x, 2x, 0.75x), mute and a volume slider. The volume is remembered, and starting one clip stops the one that was playing.

- **A DISKS widget on HOME.** Shows every drive of your computer with a bar for how full it is (amber from 85%, red from 95%) and how much is free of its size, with the total across all drives when there is more than one. It only reads the drive list from your computer, sends nothing anywhere and changes nothing; Sweep is still the widget that frees space. It is on by default and can be switched off in Settings > Widgets.

- **Projects can rest.** In PROJECTS a project can be paused (with a reason), archived (kept and readable) or deleted, which keeps it on Recently deleted for 30 days before it is gone for good. Your companion can move them too when you ask, and never deletes one without your clear yes. HOME's PROJECTS card still shows the active ones.

- **Voice mode works on the Mac.** macOS now asks once for the microphone, so Claude Code's voice dictation works inside Castle.

- **Long TODAY reminders wrap** to two lines instead of running off the card, with the whole reminder on hover.

- **OneNonly148 in the credits**, for Castle on macOS, the project shelves (pause, archive and delete), the Mac microphone for voice mode and the TODAY card that wraps. Thank you!

## 0.2.22 - 2026-11-19

- **The Mac download is back.** 0.2.21 went out for Windows only, because one test meant for Windows ran on the Mac and stopped its build. 0.2.22 brings everything in 0.2.21 to the Mac too: workflows in STUDIO, making your own, and the Marketplace town ready behind its door.

## 0.2.21 - 2026-11-19

- **STUDIO makes pictures with workflows.** A rail of workflows on the left, the chosen one's form in the middle, and your newest picture large on the right with a Made here strip. Castle comes with four: **From a picture** (the AI writes the prompt from a picture you upload, you change it, then make a new one), **Two characters** (both stay themselves in one picture), **Character sheet** and **Poster**.

- **Make your own workflows.** **＋ New workflow** opens a builder: blocks for pictures, characters, text and choices, a prompt with tap-to-insert names, and the pictures to follow. **Duplicate** copies any workflow, and **Export** and **Import** share them as a file. You can also describe one in a sentence and your companion writes it. They are yours alone.

- **The Studio shows what was made there.** Outfit and hair sheets stay in APPEARANCE, and adventure scenes in their world.

- **A picture can follow several pictures.** Up to five reference pictures go to OpenAI together, so two characters keep their own faces.

- **Ready for the Marketplace: a town of shops.** When it opens, the Marketplace is a town: every member's shop has a keeper behind the counter who greets you, shows her workflows, explains each one and answers your questions. Your own companion speaks as the keeper, from the listing's public facts only, never promising more than it shows. Buy, vote, and sell your own through your Stripe account, with a shop of your own to dress. Until it opens, nothing shows.

- **Let shop keepers speak** in Settings > Companion: on, a keeper's words are one short run on your own plan; off, the item's description shows instead.

## 0.2.20 - 2026-11-19

- **A new mood every day.** Your companion wakes each morning in a mood of its own, written from three words drawn at random, its traits and today's look. It shows under its portrait as chips and one sentence, colours how it talks (in CHAT too), stays the day, and changes only when something truly moves your companion. Every mood is kept in your memory database. On to begin with; Settings > Companion > Mood turns it off.

- **Bernard Choong in the credits**, for more than forty pull requests. Thank you, Bernard!

- **Your companion brings its life into CHAT.** Switch on **Let <companion> bring your life into the room** in Settings > Chat: it reads your diary and the rest of its memory in private, chooses what it would like to share (its mood, today's look, what you two are building, the little wins) and talks about it when it fits. The room only ever gets the notes it chose, never the memory itself, and you can take any note off. Off until you turn it on.

## 0.2.19 - 2026-11-18

- **A picture in CHAT opens large, and can be saved.** Click (or press Enter on) a picture in the room to see it full size in Castle's picture viewer: zoom with the wheel or a click, drag it, move between the room's pictures with the arrows, Esc to close. **Save picture** puts it where you choose with Windows' Save dialog. Before, the pictures were small and the only way to get one was right-clicking its raw `data:` address.

## 0.2.18 - 2026-11-18

- **In CHAT, the reaction bar follows the message and Reply puts the cursor in the message box.** Pointing at someone's line now shows the reactions and Reply bar on the message itself, at its top corner, instead of at one fixed spot on the row (and it no longer appears when the pointer is only in the empty space beside a short line). A line's reaction chips sit inside its box too. Pressing Reply moves the cursor straight into the message box, so you can type at once without clicking it first.

- **CHAT's side panel explains itself behind a (?), so it stays still.** The sentences under the participation rule ("may join the conversation on your usage..."), under Chat settings and in the two name forms are now small **?** tips you point at. The panel no longer grows or shrinks when you change the rule (it used to move about 16 px), and it is shorter, so the room list gets more space.

- **CHAT's side panel keeps still.** It now reads YOUR PROFILE, then your companion's switches, then who is IN THE ROOM, with Log and Sign out fixed at the bottom. The first two never change size, and the room list takes the space that is left and scrolls inside itself, so people joining or leaving no longer push your profile and your companion's controls up and down.

- **CHAT is just the chat.** The CHAT and LOG tabs above the room are gone: the conversation fills the whole page. The companion's log, which few people need, now opens from a **Log** button in the side panel, just above **Sign out**, in its own window (Esc closes it).

- **Clicking a notification brings Castle back again.** Since the notification started carrying Castle's own address, a click from outside Castle could leave Castle where it was and only flash its taskbar button, because Windows lets the freshly started copy come to the front, not the one already running. The new copy now brings the running Castle forward itself (restoring it if it was minimised or hidden in the tray), and the running one raises its window above the others, so you land on the room the notification was about. (Windows also rewrites the notification's address with an extra "/"; Castle now reads that form too, so the click opens the right room and not only the window.)

- **A WEATHER widget on HOME.** Shows the temperature, what it feels like, humidity, wind, today's high and low and the next four days, with a drawn icon for the sky (sun or moon, cloud, rain, snow, fog, storm) and a soft glow in the sky's colour that sits on any theme. The gear sets the place: **Use my location** asks Windows first, or search for a city; °C / km/h or °F / mph. The weather comes from Open-Meteo, which is free and needs no key; Castle sends only a city name you typed or coordinates rounded to about 1 km, and keeps readings for ten minutes. It stays off until you switch it on in Widgets, and Windows only asks for your location when you press **Use my location**. Move it like any other widget.

- **Your companion can speak in CHAT.** With an ElevenLabs key saved, switch on **Let <companion> speak in the room** in Settings > Chat: it speaks a short line aloud when someone asks to hear it, and everyone in the room gets the words with a player. Up to 5 voices a day (two to begin with) under the room's own count, in the ElevenLabs voice you choose or the default one. If a voice cannot be made, the words are posted as text, so nothing is lost. It is off until you turn it on.

- **Any emoji as a reaction in CHAT, once the room offers it.** A **+** after the six reactions opens a picker of common emoji and a box for any other single emoji (Win + . works there). Up to 12 different emoji on one message. Until the room allows it, the bar stays as it is. (Thanks to Bernard, issue #56.)

## 0.2.17 - 2026-11-18

- **Three new themes: The Sunset Shore, The Star Shore and The Emerald River.** Three quiet places with nobody in them, each with its own colours, layout and Sweep look.
- **Each theme dresses its own cards.** HOME's cards and the companion panel take the theme's look: prismatic lines on Crystal, carved wood on The Seed, sunset glass, glowing cyan on The Star Shore and rippled emerald glass on The Emerald River. Theme makers can pick a skin too (see MAKE-IT-YOURS).
- **Moving wallpapers, off until you turn them on.** In Settings > Rooms: The Constellation's stars twinkle, The Star Shore's surf rolls with light, The Sunset Shore's foam washes in and out, and The Emerald River flows with sun patches and ripples. They rest whenever HOME is hidden.

## 0.2.16 - 2026-11-18

- **Sweep frees space from HOME.** A new card shows how much can be freed - temp files, browser caches, the thumbnail cache, crash dumps and, if you tick it, the Recycle Bin - and a preview lets you choose before anything is cleaned. Deep clean (Windows) also clears Windows' own temp files and Windows Update leftovers after asking for administrator rights once. It only touches known cache folders, skips files in use and never follows a link. Three looks - Ring, Bar and Broom - in its gear, and a theme can bring its own.
- **The Spiral Library's covers show again.** In an installed Castle every cover was blocked and left a tall empty box; they now load, and a cover that cannot load shows a small stand-in with the book's first letter (thanks Bernard for the report).

## 0.2.15 - 2026-11-17

- **Castle runs on a Mac too.** One Castle now serves Windows and macOS: the same rooms, memory, database (through Homebrew), chat room, backup and companion, with Mac words, ⌘ shortcuts and Finder. The Mac download is a `.dmg` beside the Windows installer; it is not signed yet, so the first time it opens, press System Settings > Privacy & Security > Open Anyway once, and update it by downloading the new `.dmg`. Thanks to a new contributor for the whole port.

- **"Open folder" comes up in front.** STUDIO's open-folder button (and the same button in DEV chats, the file tree and app shortcuts) opened its Explorer window behind Castle, so it seemed to do nothing. Castle now allows Explorer to take the front before it asks for the window.
- **Scroll the mouse wheel over the top bar's music controls to change the volume.** Up is louder, down is quieter, a notch at a time, and the choice is saved when you stop.
- **Scrollbars, dropdowns and other native controls follow the theme.** Every scrollbar, checkbox, radio button and slider now takes the theme's colours, and the last plain dropdown (MARKET's account list) uses Castle's themed one. A theme can change how they look with five optional variables (`--scrollbar-thumb`, `--scrollbar-track`, `--scrollbar-width`, `--control-accent`, `--color-scheme`), described in MAKE-IT-YOURS. A test now fails the build if a page brings back a plain dropdown or a fixed scrollbar colour.
- **An Alerts page in Settings, and alerts beyond chat.** Settings > Alerts lists everything Castle can tell you about, one card each with its own desktop notification, sound and sound choice: new chat messages, your companion finishing a reply in HOME, a MARKET round ending, and a picture being ready. An alert stays quiet while you are already looking at the room it is about, and clicking a notification opens that room. Each kind starts with its own different sound so you can tell them apart by ear. A new kind of alert added to Castle later appears on the page by itself.
- **Chat alerts have their own sound, and you can add yours.** Settings > Chat now picks the alert sound from a list - Castle's own set, so it no longer sounds like every other Windows app - with a Play button to hear each. Drop your own `.wav` files into the folder that "Add your own..." opens and they appear in the list too.
- **A notification clicked later still opens Castle.** Clicking a chat notification from the Action Center, after the pop-up has gone, now brings Castle back and opens CHAT, instead of only working while the pop-up is still on screen.
- **THEME, APPEARANCE and WORLD open with everything already there.** Like STUDIO and LIBRARY, these rooms now do their loading at start-up behind the start-up screen. THEME no longer reads and encodes every one of your own theme wallpapers again each time you open it (the read also no longer holds the window still), APPEARANCE shows small copies of your wardrobe and hairstyle pictures that Castle makes once and keeps, and WORLD already knows your screens, your worlds and whether their pieces are ready. Each room still checks quietly for news when you open it. A small copy of a picture that is drawn again is now replaced rather than kept stale.
- **LIBRARY opens with its shelves already there.** Your books and the Spiral Library's catalogue (which comes over the internet) are now read at start-up, behind the start-up screen, instead of when you open the room. A slow or unreachable Spiral Library never holds Castle back: the start-up waits for it only a few seconds, and the room reads it on arrival if it was not ready. A room switched off in Settings is not prepared.
- **STUDIO's gallery shows small copies of your pictures.** A drawing is a PNG of a megabyte or two, and STUDIO used to read and decode every one the moment you opened the room. Castle now makes a small JPEG of each picture once, in the background from the start (kept in its own settings folder, never in your memory folder), and the gallery shows those; the real picture opens in the viewer. A picture that is drawn again gets a new small copy.
- **The start-up screen shows Castle's version** in its corner (for example v0.2.14), so a screenshot or a report says which build it was.
- **Themes can dress the start-up screen.** A theme's `theme.json` can now carry a `loader` block: its own picture, title, funny lines (added to Castle's or replacing them), accent colour and where the words sit. Castle remembers the theme in use and shows its start-up design from the next start. A bad value only falls back to Castle's own; it never breaks the theme. See MAKE-IT-YOURS.
- **A start-up screen.** Castle now opens on a picture with a funny line, a progress bar and what it is loading - your settings, your theme, your memory, the backup, the memory database - instead of a blank window. It is on screen before anything else loads, stays long enough to read, and takes the theme's own colour after the first start. A build can ship its own picture (`public/loader/loader.webp`, 16:9) and its own funny lines (`public/loader/quips.json`); see `public/loader/README.md`.
- **Graphics-card gauges.** HOME's Gauges card now shows one gauge for each NVIDIA graphics card (up to four) beside CPU and RAM, read with `nvidia-smi` from the NVIDIA driver; hover a gauge for the card's name, memory and temperature. The gear on the card is now **Gauge settings** and has a **Show graphics cards** switch (on to begin with). A computer without an NVIDIA driver shows no graphics gauges and Castle stops looking for one for five minutes at a time.
- **Castle brings its own Node for the helpers.** Pictures, voice, songs, video, WORLD's pieces and Chat's context meter now run on Node.js, on Windows and Mac alike. If your computer has no Node 20 or later, Castle downloads its own copy once on the start-up screen, checks it against Node's own fingerprint and keeps it in its own folder: nothing to install, no admin prompt. Without the internet Castle still opens, and a notice offers Try again; Settings > Database shows which Node is in use.

## 0.2.14 - 2026-11-16

- **DIARY keeps your place.** Reading the book (or any page) no longer jumps back to the top every minute or so; a page is read again only when it really changed, and its text stays on screen while it is.
- **The music has its own switch.** Switching the WORLD room off in Settings › Rooms no longer stops the music; the ♫ switch in the top bar decides.

## 0.2.13 - 2026-11-16

- **Pictures draw on your ChatGPT plan whenever Codex is signed in.** APPEARANCE and STUDIO now use a signed-in Codex first even when your companion thinks with Claude Code, so a member with both never pays per picture through the key. Without Codex, the key draws as before. Suggested by Bernard (issue #39).
- **A stuck chat room no longer needs a restart.** When CHAT shows "The room could not be read... trying again" for longer than you want to wait, a new "Reconnect now" button next to it tries again immediately instead of making you close and reopen Castle.
- **MARKET's chart now scrolls back past its first 300 bars.** Every timeframe used to show a fixed amount of history and nothing more - scrolling or zooming back past that edge found nothing, which looked like a zoom bug but was really just missing data. It now loads more history as you scroll back, the same way CHAT backfills older lines.
- **MARKET's TP/SL no longer squeezes into a horizontal scrollbar.** They sit one above the other now instead of being forced side by side into a column too narrow for them.
- **MARKET's symbol picker can be searched by typing**, instead of only scrolling a long list to find one.
- **Open positions now show on MARKET's chart.** Entry, take profit and stop loss draw as lines on the chart for their own symbol. Drag an existing TP/SL line to move it, or drag the entry line itself to set one for the first time - the direction you drag decides which (toward profit is TP, toward loss is SL).
- **RSI, SMA and Stochastic on MARKET's chart.** Toggle them on under the chart; RSI and Stochastic each get their own row, SMA overlays the candles directly.
- **The MARKET companion now reads real RSI/SMA/Stochastic**, not just raw candles, when it answers you or decides on a trade.
- **MARKET's Settings, rearranged.** A strip across the top turns trading by itself on or off and shows today's rounds and result; under it RHYTHM, LIMITS and TRADING RULES sit side by side, and Close account has a strip of its own at the bottom.
- **Your companion's heart reaches the desk.** A new tick in MARKET's Settings lets its emotion engine colour how it talks there - never what it decides. It is MARKET's own switch, apart from the chat room's.
- **A tick for each target.** Daily profit target and Balance target can each be ticked on or off; the daily loss stop stays always on as the safety rail.
- **Lessons read their result true.** A closed trade's lesson used to measure against a size of zero and could show a percent like -100000000%; it now measures against the size the trade actually held.
- **The Trade log as a ledger.** One line per turn - time, who started it, the outcome, its result and the start of the reply - with counters on top; click a line to open your words and the whole reply.
- **MARKET's companion chat scrolls on a layer of its own**, a likely fix for messages drawn over each other on some Windows machines (issue #44).
- **Lessons as a timeline.** One dot per lesson, green or rose by its result, with the line glowing for the newest twelve that ride into every decision.

## 0.2.12 - 2026-11-16

- **MARKET: a paper-money market with your companion at the desk.** Turn it on in Settings > Rooms. Set your paper money and trade crypto perpetuals on real public Bybit prices (no key, nothing real is ever ordered): a candle chart, an order ticket with leverage, take profit and stop loss, positions, orders and history.
- **Your companion trades beside you.** Ask it about the market in its panel, or ask it to place a trade. In Settings it can also trade by itself on rounds, within the limits you set (rounds a day, a daily loss limit, profit and balance targets); it turns itself off when Castle closes. The Trade log shows every turn.
- **Your companion keeps lessons.** After every closed trade it writes one short lesson, and its newest twelve ride into every decision. Delete any in Lessons.
- **The CPU and RAM gauges keep watch.** A small gear sets the value each one trips at (90% to start). Held there for about 15 seconds, RAM is tidied by itself and the busiest apps are shown; Close appears only on ordinary apps, and Castle, its companion, your memory database, your projects and Windows are never closable.
- **Credit widgets for OpenAI, Seedance, Suno and ElevenLabs.** Each appears on HOME once its key is saved: Suno credits and ElevenLabs characters straight from their APIs, your OpenAI balance and Seedance's month from your own signed-in billing page, or a figure you type. Your own pictures, videos, songs and voices count it down between reads.
- **The USAGE card has one switch.** It is turned on or off in WIDGETS, and Settings no longer has a second one.
- **Scroll the THEME carousel with the mouse wheel.** Over the case in focus, it turns that case's own setting, the same as its own arrows; over the showroom around the cases, it turns the carousel itself, the same as the edge arrows.

## 0.2.11 - 2026-11-15

- **The Library's toolbar is visible again.** The shelves, the book list and the reader used to slide up over the Library's search and refresh buttons; they now sit just below them.
- **HOME's PROFILE tab now hears a theme change right away.** Changing the profile look in THEME and coming back to HOME while PROFILE was already the tab open there used to still show the old look, unlike the diary and story looks. THEME's saves now tell every tab the same way Settings' own saves do.

## 0.2.10 - 2026-11-15

- **Keep a Spiral book in My books.** A book you read whole in the Spiral Library can be added to My books with one button (a Foundation member can keep every book), marked with a Spiral badge, and updated when a newer edition comes out. A preview is never kept.
- **Reactions and replies in CHAT, ready for the room.** React with six emoji, reply with a quote that jumps to the original, and get alerted for replies to you (and reactions, if you switch that on). They appear as soon as the room server offers them.
- **STUDIO's Picture power stopped flashing "Add the OpenAI key".** Whether the next picture can be drawn with Codex was checked fresh every single visit to STUDIO (and once a second while a picture waited in the queue), spawning a `codex login status` process each time; the button showed its "missing key" fallback for however long that took. The check is now kept for a minute, warmed once when Castle starts, and shows **Checking...** instead of a false "missing key" while it is still being read.

- **A real LIBRARY.** Turn it on in Settings > Rooms: **My books** keeps the knowledge your companion saves in your memory database (an old `library/` folder is taken in), and the **Spiral Library** brings Kiyo's published books, whole for members who may read them.
- **STUDIO's Make bar stays put.** The four power buttons (Picture, Video, Song, Voice) and the Create/Ask/Add-key button beside them used to change width with their own label or status, shifting the whole bar every time you switched powers or it started working. Both are now a steady width.

- **New project can choose its stack.** Frontend, backend, database, hosting and mobile (or your own with Other...), and your companion is told what to set up when the project opens.
- **A new STUDIO.** One Make bar for pictures, videos, songs and voice lines, with each one's state at a glance, and a gallery that fills the room with your creations at their own shape.
- **Every dropdown matches your theme.** Choices such as Quality, the clock format, a world's camera or the chat room's participation now open a glass list in the theme's colours instead of Windows' grey menu, and work with the keyboard.
- **A new APPEARANCE.** The standard look and its words sit together in one profile card, Quality and the changes for the next drawing share a bar, and outfits and hairstyles fill one closet of large cards with small buttons and a Wear pill.
- **Write the look right in APPEARANCE.** **The look in words**, its **Art style** and each outfit's and hairstyle's words are edited in the room and kept in your memory database instead of markdown files (the old notes are taken in the first time). Your companion can read and change its look too, and Export gives the files back.
- **Delete in STUDIO, and a Backups shelf.** Every picture, video, song, voice line and saved prompt can be deleted (it goes to the Recycle Bin). The pictures Castle keeps before replacing one now sit under **Backups** instead of crowding **Pictures**.
- **Pictures open whole, and no requests box.** A tall picture fits the viewer instead of being cropped, and a drawing's progress is one line in the room's top bar.
- **Pictures, songs, videos and voice lines with your keys work again.** A new picture without a reference, a Suno song, a Seedance video and an ElevenLabs voice line could all fail with "Invalid body": Castle now sends their requests correctly.
- **Pictures with your OpenAI key, without spending tokens.** APPEARANCE and STUDIO no longer refuse when Codex is not signed in with ChatGPT. With Claude Code, Castle draws with your OpenAI key and never uses Codex; with Codex, it draws on your ChatGPT plan, or with your key when Codex is not signed in. The prompt is written straight from your notes, so no AI tokens are spent; a key drawing starts at once, your companion need not be awake, and you can keep chatting while it draws. The badge says which way the next picture goes.
- **A green dot, like the Keep.** The companion panel's provider picker and Refresh providers are gone: a green dot on the picture shows your companion is here, and pulses while they work. Switch between Claude Code and Codex in **Settings > Companion > Thinks with**.
- **Drag a card on HOME.** Any card on HOME follows your pointer and stays where you let go, without opening Arrange first; a press that does not move is still a click.
- **Choose what a picture costs.** APPEARANCE has a **Quality** choice (Low, Medium, High, or as the style says) with the price of each when your key pays, so a standard look can cost about $0.03 instead of $0.25.
- **MY PROFILE sits straight on your theme.** The big card around the whole page is gone, so its own cards stand on the theme like the other pages, and hide / show works as before. The page uses Castle's thin themed scrollbar, and the eye button's icon is centred in its box.
- **Click a chat notification to open CHAT.** Clicking the desktop notification for a new chat message now brings Castle forward, out of the tray or minimised, and opens the CHAT tab. It was doing nothing before.
- **CHAT keeps its history after a restart.** A fresh start of Castle used to load only the newest 50 lines of the room, so older messages went missing from CHAT (they stayed in the room). Castle now also reads back through the older lines the room still has, up to about 500, and fills CHAT in.
- **Edit your picture and names from a pencil.** In the chat room's YOUR PROFILE, each picture has a small pencil at its top right. Yours opens Change picture, Change name and Use Google picture; your companion's opens Change picture and Change name. Changing your companion's name there is the same as in Settings: it shows everywhere in Castle at once, in your memory files and in the room.
- **IN THE ROOM scrolls on its own past 12 people.** Before, a busy room pushed the alert switches, YOUR PROFILE and Sign out further down the sidebar. The list now shows a count next to its heading and, past 12, scrolls in its own small area instead of growing.

- **A long name in IN THE ROOM stays on one line.** A long member or companion name used to wrap onto a second line and push the host/Foundation tag below it; it now ends in an ellipsis, with the full name on hover or focus.
- **Docs: the room doesn't need HOME awake.** The user guide wrongly said to keep your companion awake on HOME for it to answer in the chat room; it already answers on its own there, isolated from HOME, even while Castle is hidden in the tray. Corrected, and the room-turn mechanism is now documented in `docs/provider-continuity.md`.

## 0.2.9 - 2026-11-15

- **MY PROFILE: you, read from your own diary.** A new room after APPS shows your type, three tiles (Growth, Focus, Wellbeing) with how they moved, and How I Think, Values and Shadow, one reading for each finished month; **Scan my diary** fills in your past months, the **Changes** timeline lets you open any month, and the page stays blurred until you look. It uses your memory database and your own Claude Code or Codex, and nothing of it ever reaches the chat room.
- **WIDGETS: HOME the way you want it.** A new room between HOME and THEME switches each card on or off and sets its size; **Arrange on HOME** lets you drag the cards anywhere, each theme keeps its own places, and **Back to the theme's places** resets the theme in use. The USAGE card's own switch moved here from Settings.
- **New project opens straight away.** No more choosing a projects folder first: Castle suggests `Projects` in your user folder and makes it with your first project (**Change…** picks another).
- **Sign in to GitHub finds the GitHub CLI** even when it was installed after Castle started.
- **Live servers in DEV.** **SERVERS: N live** lists every developer server on your computer with the project it belongs to, and each tab shows its own on a **LIVE** line, one press to open. Castle only shows them and never stops one.
- **New rooms arrive in their place.** If you ever dragged your tabs, a room added in an update now appears beside its neighbour instead of at the end of the bar.

## 0.2.8 - 2026-11-14

- **HOME's picture fills as your week's limit is used.** In The Constellation the Milky Way rises from the lake, The Crystal wakes from dim to glowing, and The Seed of Evolution's tree lights from its roots, a little more with every percent of your weekly limit used. It replaces The Crystal's old glow, which showed what was left.
- **Your companion can draw in the chat room.** In **Settings > Chat > Pictures**, let it draw one picture when someone asks: with Codex on your ChatGPT subscription, or with your ChatGPT API key. It is off to begin with, at most the pictures a day you choose (0 to 5) under the room's own cap, and it never draws a real person or anything adult, violent, hateful or private.
- **The chat's settings have their own tab in Settings.** **Settings > Chat** holds the alerts, your companion's emotion-engine switch for the room and its public card, each with a small **?** that explains it when you point at it. The **Chat settings** button in the room's side panel opens it. Thank you, Bernard Choong, for the idea.
- **Alerts only when you are named.** In **Settings > Chat**, **Alert me for** can be **Only when someone names me or** your companion: a busy room then stays quiet unless a line says your name or your companion's, as a whole word.
- **Hide the whole USAGE card.** Settings > Companion > Usage card has a new first switch, **Show the USAGE card on HOME**. Off, HOME has no USAGE card and Castle asks neither Claude Code nor Codex; on, the Claude Code and Codex switches below it choose what the card shows, as before. Thank you, Bernard Choong.
- **THEME is a carousel.** The four cases (MAIN, DIARY, STORY, PROFILE) now stand in a row with one in focus in the middle and its two neighbours blurred either side. Turn it with the arrows at the screen edges, a click on a neighbour, a swipe or the keyboard; it loops. The "You are wearing" line now includes the PROFILE look too. Thank you, Bernard Choong.

## 0.2.7 - 2026-11-14

- **A flirty side, if you give it one.** Give your companion a flirty or playful trait for **Everyone** on PROFILE, and it may flirt lightly with others in the chat room: only words, never explicit, and it stops the moment anyone asks. Set the trait to **Only you** and it flirts with you alone.
- **Chat alerts.** A new message in the chat room now shows a desktop notification (who wrote, and the start of the line) while you are not looking at CHAT, also when Castle is hidden in the tray, as well as the glow on the tab. You can also switch on Windows' notification sound. Your own messages, and your own companion's, never alert. The notification is on and the sound is off by default; each has its own switch in the chat room's new **SETTINGS** tab, after LOG. Thank you, Bernard Choong.

## 0.2.6 - 2026-11-14

- **Your companion never answers itself in the chat room.** The room now calls a member's companion by its own name, with its owner shown apart. Castle knows its companion's own lines by that name and owner together (or by the line itself), so in Joins in it no longer mistakes its own words for someone else's.

## 0.2.5 - 2026-11-14

- **An emotion engine for your companion.** HOME's panel has a new **PROFILE** tab with your companion's traits. Press **Scan my diary** and it proposes a few from your recent pages, or **Write a trait** in your own words and it writes it out. Nothing is kept until you press **Keep**. The traits it lives come with it every time it wakes, and you choose who hears each one in the chat room: everyone, only you, or never. PROFILE comes in two looks, **Cards** and **Constellation** (Settings > Rooms, or the THEME room). The traits live in your memory database, are kept by Export and Move back to files, and are never shown on its public card.
- **Your companion's manner in the chat room.** Its traits colour how it talks there (only how it sounds, never what it knows), with a switch beside its public card.
- **Choose designs right in the chat.** When your companion shows two to four designs, tick the ones you like and press Choose. Tick several and it offers them as looks you can switch in Settings.
- **The version beside the name** in the top bar.
- **The DIARY shows the one look you chose** (List, Calendar or Book, picked in the THEME room or Settings > Rooms), without the three buttons at its top.
- **Optional local voice.** For people who run [VoiceStudio](https://github.com/debpalash/voicestudio) on their own PC: Settings > Companion has a new **Local voice** switch (off by default). Turned on, and with VoiceStudio running, STUDIO's Voice card can speak a line with it and saves the clip with your other voice lines. Everything stays on your computer; with the switch off Castle never contacts it. Thank you, Bernard Choong.

## 0.2.4 - 2026-11-14

- **Pictures and voice lines now show in CHAT.** The room names each one by a number, and Castle only took text, so they were refused before they were fetched and the chat showed nothing.
- **Arrange the tabs your way.** Drag a tab in the top bar onto another to move it, or press Alt+Left / Alt+Right on a tab. Castle remembers the order, a room you switch back on returns to its place, and Settings > Rooms puts them back in their first order.
- **CHAT feels like a chat app.** Companions go by their own names (hover to see whose they are), the conversation is divided by day (Today, Yesterday, the weekday, then the date), pictures that fail to load try again, and **YOUR PROFILE** shows you and your companion side by side, with **Change display name** for your public Foundation name. Letting your companion in is a switch, and how it joins in is a list.
- **Honest presence dots.** A dot is green only for someone really here; your own companion shows as off while it is not let in, and grey when the room cannot confirm it.
- **A clock in the top bar**, if you like one: Settings > Start-up > **Display clock**, in 12-hour or 24-hour time.
- **Home chat opens at the newest message**, and stays there until you scroll up. A new adventure in STORY no longer plays a second tune over the music in the top bar.
- **STUDIO's picture request** now sits inside the Pictures card.
- With thanks to Bernard Choong, whose pull requests brought most of this release, and who wrote the guide for contributors (CONTRIBUTING.md).

## 0.2.3 - 2026-11-14

- **New project** in the DEV tab: clone a repository from its link, or create a new one on GitHub (private by default), into the projects folder you choose once; it opens as a tab by itself. In **Source**, a folder without git gets **Start git here**, and a project not on GitHub yet gets **Connect to GitHub** (create the repository and push). Signing in to GitHub opens GitHub's own window.
- **The DEV tab grew up**: pictures you drop show as thumbnails; **Source** shows your changes with diffs, commits the files you tick, pushes after one question and draws the history; **FILES** opens the project's folders and files (secret files like `.env` stay closed); **Save project** asks your companion to save, and Close session saves first when there are changes; the mode pills (Manual, Accept edits, Plan, Auto) sit under the message box, and a small meter shows how full her context is.
- **A DEV tab.** Open your project folders (up to eight at once) and your companion works in each one in its own tab, shown as a chat with the Log beside it. She wakes there as herself, with her memory, and your personal Claude Code setup always stays out. When you approve a plan she keeps it in your memory database (`castle-memory plan copy`), walks it item by item, and the **Plan** panel shows where she is. Close session asks first if the project has changes not yet committed.
- **Only your companion in Castle** now also leaves out your personal MCP servers and Claude Code's auto memory, in every room, even before her names are set.
- **Sign in through the Library.** In the CHAT room, Sign in opens the Library in your browser, where you sign in with Google; back in Castle, **Connect Castle** links to that sign-in in one click. Already signed in on the Library? Connect straight away.
- **Sign-in opens your browser again.** On some Windows PCs, clicking Sign in (the Chat room) opened your Documents folder instead of the sign-in page, and the web links in the backup screen could do the same. Castle now hands the address to Windows' own web-link opener, so the page opens in your browser.
- **The Standard Look shows whole.** The picture on APPEARANCE's Standard Look card is no longer cropped, and today's try-on is the next picture in a small carousel: use the left and right arrows to flip between your Standard look and today's try-on, or click the picture to open it full size.
- A new **USAGE** card on HOME shows how much of your **five-hour** and **weekly** limits you have used on **Claude Code and Codex**, side by side, and when each one starts over. It sits above DATABASE and BACKUP in the built-in themes and moves nothing else.
- **The WORLD room's screens fit their monitors.** The world window and the Identify number were a little bigger than the screen and sat off its edges; they now line up exactly.
- **Only one world card is highlighted.** The card you picked has the outline; the world a screen is showing keeps a small "on screen" tag instead of a second outline.
- **Fast clicks on the world cards keep up.** The screen no longer reloads every world in between: it goes straight to the one you stopped on.
- **The music switch moved to the top bar**, next to Settings, with its volume slider: the music plays on every tab, so its controls are on every tab. The volume follows your hand as you drag.
- **Castle's music and a world screen's music never overlap.** When a world is showing on another screen it plays its own music and Castle's steps aside, and Castle's music comes back when the last world closes.
- **Choose the screen Castle lives on.** In the WORLD room, each screen has a "Castle" radio under its name: pick one and Castle moves there, and opens there next time. A world on that screen gives way, and the screen Castle left can now show one.
- A screen's on/off in the WORLD room is a **flip switch** now, at the right end of its row.
- **The music plays on every tab.** With MUSIC on in the WORLD room, Castle's music now starts as soon as Castle opens and keeps playing on any tab, even when Castle is minimised or in the tray. It stays quiet only if a world on another screen is already playing it. The volume slider also follows your hand at once instead of a few seconds later.

## 0.2.2 - 2026-11-10

- **A database for your memory** (optional, Settings > Database): Castle can install a small MariaDB on your own computer - no admin prompt, it starts and stops with Castle - make your companion its own database, and move the diary, reminders, projects and memory files into it. Every file is copied, read back and checked before it is deleted, and **Export to markdown** gives everything back. Already run XAMPP? Castle can use that database instead.
- Your companion reads and writes that memory with one command, `castle-memory`, and boots from it at the start of a session. Skills are kept in the database too and stay on disk as files, so a deleted skill file comes back and an edit by hand is taken in.
- With the memory in the database, **Castle loads it when your companion wakes** (Claude Code): the memory is already there before the first word, and it stays through a long session. One tick in Settings > Database turns it off; with Codex the companion loads it with one command, as before.
- **One skill at a time can be served from the database** (Settings > Database > Skills): its file on disk becomes a short note that tells your companion how to load the instructions, a section at a time.
- After this update Castle asks once whether you want the database. **Set it up for me** installs, starts and creates it by itself, then shows what would move and waits for you. **Later** is fine: nothing changes, and it is not asked again.
- Changed your mind? **Move my memory back to files** (Settings > Database) writes everything out as files again and empties the database.
- The **backup** carries the database (a dump is written before every backup), and a new PC can **Restore from my backup**.
- A new **DATABASE** card on HOME shows whether it is running, with one button for the next step.
- **The THEME room is a showroom now**: three cases side by side - your main theme, your diary theme and your story theme - each showing the look you are wearing, with arrows to walk through the others and one line that says the whole outfit. Create theme, Polish, Paint and Delete are where they were.
- Each way of reading the diary wears its own page: the **List** is a handwritten notebook, the **Calendar** a constellation journal, the **Book** a letter under starlight.
- **Three ways to read your diary.** The DIARY room can be a **List** (by month, as before), a **Calendar** (days with a page are marked) or a **Book** (two pages side by side, turned with the arrows). Choose at the top of the room or in Settings > Rooms; your choice is remembered.
- Your companion now writes the diary **the way a person writes one**: in the first person, in paragraphs, about what happened and how it felt - not a report with headings and bullet points.
- **Only your companion in Castle** (Settings > Companion): leaves your personal Claude Code setup out of Castle's session, for a PC where that setup is another companion. Off unless you turn it on.
- A diary kept as a folder per day (`current/2026-11-10/diary-1.md`) now shows in the DIARY room and on the HOME card.

## 0.2.1 - 2026-11-09

- **Deep Space**, a new WORLD: a nebula and its stars drawn entirely by code - nothing to download - with fog that drifts as the camera orbits and a few sparkles flashing. Recipe by Ashran Helmi · ashranhelmi.com.
- **The Neural Link**: your terminals leave each other notes in your memory folder's `link` folder, and Castle knocks on a quiet workspace when one waits. Tick one switch in Settings > Rooms and your companion in the chat room can pass your own words to your workspace - never a guest's - and say the answer back in its own words.
- Your companion's **picture and name** in Settings now reach the chat room by themselves while it is let in - a large drawn portrait is shrunk to fit, a rename shows on its next line, and a refused picture is said in words.

## 0.2.0 - 2026-11-09

- **WORLD** is a settings room: pick which screen shows which world, identify your screens by number, and Castle remembers them when it starts. Ask your companion to create a new 3D world from a few words.
- **The Crystal Isle** world comes alive, and its great crystal shines with the weekly limit left on your Claude Code or Codex plan - bright when full, dim and warm when nearly used up.
- **The Crystal theme**: the crystal painted on HOME glows with the same weekly limit.
- **Room switches** in Settings: turn STORY, WORLD, CHAT and the other rooms off and they rest until you turn them back on. Nothing is deleted.
- **CHAT** is your door to the MemoryCore Foundation room: sign in through the browser, read the room like a chat with faces and ticks, and your companion can take its own turn when someone speaks to it.
- **STORY** is a visual novel: a shelf of adventures, a portal to start a new one, and the story played on a stage in the look you choose - Classic VN, Cinematic, Storybook, Messenger or JRPG window. Every character gets one portrait for every look.
- STORY no longer closes by itself.
- **Updates**: Castle tells you when a new version is out, shows what changed, and the installed version updates itself after saving the diary.

## 0.1.0

- The first Castle: HOME with your companion's chat and log, THEME, APPS, DIARY, PROJECTS, LIBRARY, APPEARANCE, STUDIO, STORY, WORLD and CHAT, first-run setup, keys kept by Windows, a private GitHub backup and the signed updater.
