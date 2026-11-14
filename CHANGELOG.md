# Changelog

What changed in each MemoryCore Castle release, in plain words. The newest is first. Castle shows the section of the version you just updated to, once, and Settings > About shows it any time. Write each change as one line.

## Unreleased

- **Optional local voice.** For people who run [VoiceStudio](https://github.com/debpalash/voicestudio) on their own PC: Settings > Companion has a new **Local voice** switch (off by default). Turned on, and with VoiceStudio running, STUDIO's Voice card can speak a line with it and saves the clip with your other voice lines. Everything stays on your computer; with the switch off Castle never contacts it.

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
