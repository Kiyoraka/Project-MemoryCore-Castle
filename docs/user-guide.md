# MemoryCore Castle - User Guide

MemoryCore Castle is a full-screen desktop for your MemoryCore companion. It opens your own MemoryCore folder, runs **Claude Code** or **Codex** inside it, and gives your companion a home on your screen.

![HOME in the Constellation theme](screenshots/home.jpg)

## Contents

1. [Getting started](#1-getting-started)
2. [Chat and Log](#2-chat-and-log)
3. [Quick actions](#3-quick-actions)
4. [First run](#4-first-run)
5. [Backing up your memory](#5-backing-up-your-memory)
6. [HOME](#6-home)
7. [Themes](#7-themes)
8. [Apps, Diary, Projects and Library](#8-apps-diary-projects-and-library)
9. [Studio](#9-studio)
10. [Appearance](#10-appearance)
11. [Story](#11-story)
12. [World](#12-world)
13. [Chat room](#13-chat-room)

---

## 1. Getting started

### What you need

- Windows 10 or 11
- **Claude Code** or **Codex**, installed and signed in with your own account. Castle has no account of its own; your companion uses the subscription you already have.
- A MemoryCore folder. If you do not have one yet, Castle can create it for you the first time it opens.

### Opening and hiding Castle

- Castle fills your desktop and keeps the Windows taskbar visible.
- **Hide** it with the **–** button at the top right, or press **Ctrl + Alt + M**. Press the keys again to bring it back.
- Castle lives in the system tray (bottom right of the taskbar). Click its crystal icon to show or hide it. Right-click it for **Open Castle**, **Wake companion** and **Quit**.
- Closing the window only hides Castle. Use **Quit** in the tray to end it; if your companion's session is running, Castle asks first.

### Waking your companion

The panel on the left belongs to your companion. When the session is not running you see **Wake** with your companion's name. Press it to start Claude Code or Codex in your MemoryCore folder.

If Castle cannot start the session, the panel says why in one sentence and offers the fix:

| What the panel says | What to do |
|---------------------|------------|
| No MemoryCore folder is chosen yet | Press **Open setup** and choose or create your folder |
| Your MemoryCore folder was not found | Press **Settings** and choose the folder again (it may have moved) |
| Claude Code (or Codex) was not found | Install it, sign in, then press **Wake** again |

---

## 2. Chat and Log

The companion panel has two views. Switch between them with **CHAT** and **LOG** at the top of the panel; the session keeps running either way.

### Chat

Chat shows your conversation as message bubbles, like a messaging app.

- Type in the box at the bottom and press **Enter** to send. Press **Shift + Enter** for a new line.
- While your companion is working (reading files, running tools), Chat shows a typing bubble. The detailed steps stay in the Log.
- Pictures your companion makes appear in Chat; click one to see it large. Voice lines play one at a time, and you can play them again.
- **Share a picture:** drag it onto Chat, or paste it with **Ctrl + V**. It shows above the box before you send.
- A plan your companion proposes appears as a **PLAN** card; click it to read the whole plan.
- Ticks on your messages: one tick means it is waiting in the queue (your companion reads it after its current step); two ticks mean it was read.

**Claude Code:** when your companion asks you to choose (for example, to allow a command), the choices appear as buttons in Chat.

**Codex:** when Codex asks for your approval, Chat shows *"is asking for your approval"* with an **Answer in Log** button. Answer it in the Log.

### Log

The Log is the real terminal underneath, exactly as Claude Code or Codex shows it. Use it for anything Chat does not show.

- **Copy:** select text and press **Ctrl + C**, or right-click.
- **Paste:** **Ctrl + V**, or right-click with nothing selected. A copied picture is saved and its file path is pasted.
- Drop a file on the Log to type its path.

### Making the panel bigger

- Drag the panel's right edge to make it wider or narrower.
- The **⤢** button gives the panel the whole screen; **⤡** puts it back beside the widgets.

---

## 3. Quick actions

You do not need to remember MemoryCore's commands. The row of buttons under **CHAT | LOG** types them for you.

| Button | When it shows | What it does |
|--------|---------------|--------------|
| **Save diary** | Always | Asks your companion to write today into your daily diary |
| **Save project** | When your MemoryCore has projects | Saves the project you are working on |
| **Save memory** | When the save-memory skill is installed | Saves what your companion learned this session |

- The buttons are greyed out until your companion's session is running.
- After a click the button shows **Sent ✓** for a moment, and your companion answers in Chat as usual.
- You can add your own buttons (a label and the words to type) in **Settings**.

---

## 4. First run

The first time Castle opens, it walks you through six short steps. You can open them again later with **Open setup** in the companion panel, or the **⚙** button.

| Step | What you do |
|------|-------------|
| **Folder** | Choose your MemoryCore folder, or press **Create it** to download a fresh MemoryCore into a new folder |
| **Companion app** | Pick **Claude Code** or **Codex**. Castle checks that it is installed and signed in, and shows the install command to copy if it is not |
| **Names** | Your companion's name, your name, and how your companion should be (a friendly partner, a calm mentor, ... or your own words). Add a picture if you like; without one your companion is a glowing crystal |
| **Keys** | Optional keys for extra powers (see below). Skip any you don't have |
| **Look** | Choose a theme, and whether Castle starts with Windows and wakes your companion when it opens. Press **Save and continue** |
| **Backup** | Optional: keep your memory safe on GitHub (see [chapter 5](#5-backing-up-your-memory)). Press **Skip for now** to do it later |

### Already using MemoryCore?

Castle reads what your memory already holds, so you are never asked twice:

- The names you gave in MemoryCore's own setup are filled in for you (*"Found in your memory - press Next to keep them"*). Castle never rewrites your memory files; renaming in Castle only changes what Castle shows.
- Keys you already keep in your MemoryCore `.env` are offered: press **Keep them encrypted in Castle**. Your `.env` is left as it is.

### What Castle writes into your memory folder

- The names, in place of MemoryCore's `[AI_NAME]`, `[YOUR_NAME]` and `[RELATIONSHIP_STYLE]` placeholders (only when they are still there).
- A short block in **CLAUDE.md** and **AGENTS.md** that tells Claude Code and Codex who your companion is and to load your memory first. Castle only rewrites the part between its two `castle` markers; anything you write outside them stays.

### Keys

| Key | What it adds | Without it |
|-----|--------------|------------|
| **ChatGPT (OpenAI)** | Your companion can draw pictures (Claude Code only - Codex draws by itself, so this key is hidden for Codex) | The picture's description is saved instead, and Castle tells you |
| **Seedance** | Short videos | The video's description is saved instead |
| **Suno** | Songs sung from lyrics | The lyrics are saved instead |
| **ElevenLabs** | A speaking voice | The words are saved instead |

Keys are kept **encrypted by Windows** (Credential Manager, tied to your Windows sign-in). They never go into your memory folder and are only handed to your companion's session while it runs.

---

## 5. Backing up your memory

Your memory is precious. Castle can keep a copy on GitHub in a **private** repository that only you can see. You need a free GitHub account.

Open the backup from setup's last step (or later from Settings) and follow the ticks:

1. **Git installed** - if not, press **Get Git**, install it, then **Check again**.
2. **GitHub CLI installed** - if not, press **Get the GitHub CLI**, install it, then **Check again**.
3. **Signed in to GitHub** - press **Sign in to GitHub**. A small window opens; follow it in your browser (you type a one-time code), then press **Check again**. Castle never sees or stores your GitHub password.
4. **Your own private backup** - choose a name and press **Create my private backup**. Castle makes the private repository and sends the first backup.

After that:

- Press **Back up now** whenever you like. Castle shows when the last backup was and how many files changed since.
- Turn on **Back up after every Save diary** and Castle backs up by itself a minute after each diary save.
- Castle never backs up your `.env`, key files or videos.

**Safety:** if your folder came from MemoryCore's public template, it still points at that public repository. Castle will **never** send your memory there. It tells you so and creates your own private backup instead, keeping the template as `upstream`. If your backup repository is ever public, Castle warns you.

---

## 6. HOME

HOME is your desktop. Your companion's panel is on the left; the cards on the right read your memory and update by themselves every minute.

### The companion panel

At the top: your companion's picture (or the glowing crystal), their name, and the app they think with (**Claude Code** or **Codex**). Under it are the quick actions, then **CHAT** and **LOG** (see chapters 2 and 3).

### The cards

| Card | What it shows | Where it comes from |
|------|---------------|---------------------|
| **Clock** | The time and date | Your computer |
| **CPU / RAM** | How busy your computer is | Your computer |
| **USAGE** | How much of Claude Code's and Codex's **five-hour** and **weekly** limits you have used, and when each starts over. A bar turns warm at 80% and red at 100%. Codex only reports its limits while you use it, so its numbers can say "as of" an earlier time | Claude Code's `/usage` (with the login it already keeps) and Codex's own session files, read every five minutes |
| **TODAY** | Your open reminders, with their due dates | `main/reminders.md` (MemoryCore's Reminders) |
| **PROJECTS** | Your active projects, most recent first. Click one to open its file | `projects/project-list.md` (MemoryCore's Projects) |
| **DIARY** | Your newest diary entry. Click it to open the page | `daily-diary/` |
| **GROWTH** | How many memory files, days since your first diary, and skills | Your MemoryCore folder |
| **BACKUP** | When your memory was last backed up, with **Back up now** | Your private GitHub backup (chapter 5) |

A card with nothing to show yet says so in a friendly line - for example, a new memory's DIARY card reminds you to press **Save diary** after a talk.

About the **USAGE** card's login: to show Claude Code's limits, Castle reads the login Claude Code already saved on this computer, only to ask Anthropic for your usage - it is never stored or sent anywhere else. Codex's usage is read from Codex's own files on your computer. You can turn off either one in **Settings › Companion**.

### Turning a room off

In **Settings › Rooms** you can switch off any room you don't use (HOME always stays). Its tab disappears and nothing runs for it - STORY's session ends, WORLD's screens close, CHAT stops reading the room - but **nothing is removed**: turn it back on and everything is there as it was.

### Rooms that appear by themselves

Some rooms only show in the top bar once your MemoryCore has that feature: **DIARY**, **PROJECTS** and **LIBRARY**. Install the feature (ask your companion), and the room appears within a minute.

---

## 7. Themes

Open **THEME** (right after HOME) to change how Castle looks.

- Every theme is a card with a preview. Press **Use** to switch; the theme in use says **In use**.
- **Create theme:** give it a name, describe the mood and colours in your own words, and upload a picture (or leave it out for a soft gradient). Press **Create** - Castle reads the colours from your picture and makes a working theme straight away, with text kept easy to read.
- On your own themes:
  - **Polish** asks your companion to fine-tune the colours and layout from your words.
  - **Paint** asks your companion to paint a wallpaper for the theme (it tells you the cost first; with Codex it draws by itself).
  - **Delete** removes the theme (choose another theme first if it is in use).
- After your companion has worked on a theme, press **Reload themes**. If a theme cannot be read, it is listed at the bottom with the reason, and Castle keeps showing the theme you had.

Your themes live in Castle's settings folder (`%APPDATA%\com.memorycore.castle\themes`), one folder each with a `theme.json` and a wallpaper, so you can also make or share themes by hand - see `MAKE-IT-YOURS.md`.

---

## 8. Apps, Diary, Projects and Library

### APPS

Your Start-menu apps, with the ones you use most at the top. Type to search, then press **Enter** or click to open an app.

### DIARY, PROJECTS and LIBRARY

These rooms let you read your companion's memory without leaving Castle (they appear once your MemoryCore has the feature):

- **DIARY** - every page, newest first, grouped by month. Older diaries are read too.
- **PROJECTS** - your active and archived projects.
- **LIBRARY** - your saved knowledge by section, wherever your library lives.

Pick a page on the left to read it on the right; type in **Search** to find one. **Show file** opens the page's folder. The rooms only read - your companion is the one who writes your memory. Clicking a project or the diary on HOME opens it here.

---

## 9. Studio

Your companion can make pictures, short videos, songs and voice lines. Just ask in Chat - for example *"Draw me a picture of our garden at night"* or *"Write and sing a song about today"*.

### The four cards

At the top of **STUDIO**, each power shows its state:

| State | Meaning |
|-------|---------|
| **Ready** | The key is kept, so your companion can make the real thing |
| **Built in (Codex)** | Codex draws pictures with your ChatGPT account - no key needed |
| **Prompt only - add a key** | No key yet: your companion saves the idea (the picture description, the lyrics, the words) instead, and Castle tells you |

Press **Add the ... key** on a card to add it (see chapter 4, Keys).

### Your own voice (optional, for self-hosters)

If you run [VoiceStudio](https://github.com/debpalash/voicestudio) on this PC, you can use it for the Voice card instead of a key. It is off by default: turn it on in **Settings > Companion > Local voice**. With it on and VoiceStudio running, the Voice card says **Ready - VoiceStudio (on this computer)**: type a line, press **Speak with VoiceStudio**, and the clip plays and is saved with your other voice lines. Castle uses your first saved VoiceStudio voice (a saved voice also keeps one-word lines from coming out as noise) and checks each clip with VoiceStudio's own transcriber, making it again if it came out as noise. Everything stays on your computer, Castle never installs or starts VoiceStudio, and while the switch is off Castle never contacts it. VoiceStudio's models have their own licences; some are non-commercial.

### Before anything costs money

Your companion always tells you the estimated cost first and asks before making a picture, video, song or voice line.

### When a key is missing

The idea is saved next to where the file would have gone (a `.prompt.md`, `.lyrics.md` or `.line.txt` file), and a note appears at the bottom right: *"Saved the lyrics instead - add the Suno key in Settings to hear it sung."* Add the key, then ask again to make the real thing.

### The gallery

- **Pictures** - click one to see it large; use the arrow keys to move between them.
- **Videos**, **Songs** and **Voice** - play them right in Castle. Songs are grouped by album.
- **Saved prompts** - the ideas saved while a key was missing, with **Show file**.

Everything lives in your MemoryCore folder (`media-generation/`, `music/`, and the story's own folders), so it is part of your memory and your backup.

---

## 10. Appearance

**APPEARANCE** gives your companion a face of their own, and clothes and hairstyles to change into.

### How drawing works here

Castle sends Appearance and Studio picture requests to a separate Codex job, even when the main conversation uses Claude Code. Codex must be installed and signed in with ChatGPT. An installed CLI is not proof that image generation is available for that account; the actual request is checked when it runs.

- **Codex available — checked when drawing** means Castle found the CLI.
- **Codex unavailable — prompts are saved** means you can keep the request and retry after installing/signing in.
- Image jobs show queued, working, completed, or prompt-saved status. Failure never silently falls back to an API key. Video, music and voice keep their separate key requirements.

Submitting an image request starts a Codex task once the main conversation reaches a confirmed turn boundary. Normal Codex permissions remain in effect; if the noninteractive job cannot proceed, Castle saves the prompt. Press **Refresh** after completion. Saved prompts can be retried from the room. Interrupted jobs are not automatically rerun after Castle restarts.

### The standard look

Press **Upload image** to use artwork you already have, or **Draw the standard look** to start from `appearance/companion.md`. Uploads accept PNG, JPEG and WebP up to 25 MB and are copied into your MemoryCore as PNG. Replacing an imported image preserves the previous file. Uploading does not generate an image or call a paid API.

Write what you want changed in **Changes for the next drawing**, then press **Improve from image**. Castle sends the existing image as a reference to the separate Codex job and asks it to preserve the face and body proportions. The selected conversation provider stays unchanged. Press **Refresh** after the result is saved.

The standard look also becomes your companion's picture in the panel, unless you chose your own picture in setup.

### Wardrobe and hair

- Press **+ New** on **WARDROBE** or **HAIR**, give it a name and description, optionally **Add reference image**, then **Add**. This saves the item without starting generation.
- **Upload reference** also adds existing artwork to a saved item. The copied `reference.png` remains separate from the generated `turnaround.png`.
- Click an image to see it large. **Improve / draw turnaround** asks your companion to use the reference (or existing sheet, then standard portrait) to draw front, side and back views with your requested changes.
- **Wear** chooses today's outfit or hairstyle (press again to take it off). Your companion reads today's look from `appearance/current.json`.
- **Try on today's look** asks for a portrait in today's outfit and hair.

Everything lives in your MemoryCore folder under `appearance/`, so it is part of your memory and your backup.

---

## 11. Story

**STORY** is a fantasy adventure you play together with your companion, in a session of its own so your everyday chat stays separate. It follows MemoryCore's Interactive Story, so adventures you already have in `adventures/` show up here too.

### Starting an adventure

1. Open **STORY** and press **Wake** in the story panel on the left.
2. In **NEW ADVENTURE** choose:
   - **Duo** (your companion fights beside you) or **Solo**
   - **OP** (win with style) or **Balanced** (real danger)
   - your **hero name** and one of eight **classes** (Sword Saint, Lancer, Archmage, Shadow Reaper, Guardian, Ranger, Enchanter, Berserker)
   - a **world** (High Fantasy, Dark Fantasy, Eastern Fantasy, Steampunk, Celestial, Pirate/Naval, Demonic) or **Random**
3. Press **Open the portal**. Your companion builds the world and the story begins.

### Playing

Each scene ends with numbered choices - type a number, or write anything you want your hero to say or do. Your companion may offer a scene picture at big moments, an opening song for the first chapter, an ending song at the end, and voiced lines - always with the cost first. Without a key, the idea is saved instead and Castle tells you (see chapter 9).

### Saving and coming back

- Press **Save adventure** (or type *save adventure*). Your story is kept in `adventures/<world>/` inside your memory.
- Under **YOUR WORLDS**, **Continue** picks an unfinished adventure back up; **Read** opens its chapters to read again; scene pictures show as small pictures you can open.
- **Leave the story** ends the story session (your everyday session keeps running).

When the story speaks while you are in another room, the **STORY** tab glows.

---

## 12. World

**WORLD** gives your other screens a world to show while you work or rest. The room itself is a settings page: no world is drawn in Castle's own window unless you ask for a preview.

### Your screens

Every screen connected to your computer has a row:

- **WORLD** - which world this screen shows. A world still waiting for its pieces is greyed out and shows its count, for example *Crystal Isle (0/5)*.
- **CAMERA** - how a 3D world is filmed: **Director - random views** (the camera picks a new view every few seconds), **Follow the crystal**, **Wide only** or **Slow orbit**. A 2D world simply says *Slideshow*.
- **on / off** - show the world on that screen, or take it away. Every screen starts **off**.
- **Identify** - shows a big number on that screen for a moment, so you know which row is which.
- **Settings** - for a 3D world: how long each view holds (for example 8 to 20 seconds), whether the camera glides, cuts or mixes both between views, and which views the director may use. For a 2D world: how long each picture stays, and shuffle. **Reset to default** puts them back.

The screen Castle itself is on says *Castle lives here*: it never gets a world window. Press **Look for screens** after plugging a screen in. Castle remembers each screen's world and brings it back the next time it opens, even if Windows renames the screen.

### The worlds

The cards on the right are every world you can choose. Click a screen's row, then a card, to show that world there.

- **Floating Isles** (3D, built in, always ready) - five little isles joined by bridges of light, under a sky that follows the real time and moon. Your companion is the glowing crystal: it brightens while it is working, dims while it rests, gives a small spin when you send it a message, and wanders between the isles when it is idle.
- **Crystal Isle** and **Isles of Memory** (3D) - worlds made of 3D models. Until their pieces are in, their card says *waiting · 0/5 · make in Tripo*, and a screen set to them shows Floating Isles meanwhile.
- **Isles of Memory** (2D) - six painted scenes as a slideshow.
- **My pictures** (2D) - your theme's wallpaper and every picture your companion has drawn, slowly drifting one into the next.

**Preview the picked world here** opens the world inside the room, so you can look before choosing - handy if you have only one screen. It closes when you leave the room.

### The crystal's glow

Your companion's crystal shines with how much of your **weekly limit** is left on Claude Code or Codex: bright and sparkling when there is plenty, softer as you use it, dim and warm under a fifth, a faint ember when it is used up. The WORLD room says the number under the list of your screens. Castle reads it on your PC with the login Claude Code already keeps (or from Codex's own session files) every five minutes, and never shows or sends that login anywhere else. If it can't read the limit, the crystal keeps its normal glow.

### Music

The first screen showing a world plays the music: Castle's own (*Floating Isles* for 3D worlds, *Castle Theme* for 2D ones), then the songs your companion has made. Turn it on or off and set the volume with **MUSIC**, beside **Look for screens** at the top of the WORLD room.

### Resting

The world windows hide and come back together with Castle. A world that is covered by another window stops drawing (and its music pauses) so your computer stays cool, and after ten minutes covered it lets go of its memory until you can see it again.

### Create your own world

You can describe a world and have your companion help you build it.

1. Press **+ Create world**. Give it a name, say what it is like, and choose a layout:
   - **One centrepiece** - a great floating isle with something glowing at its heart (your companion) and three islets circling slowly round it.
   - **An archipelago** - five isles joined by bridges of light and a few small islets, with the crystal walking between them.

   You can add a picture for the look.
2. Press **Create and ask <companion>**. Castle makes the world's folder in your memory (`worlds/<name>/`) and asks your companion, through Chat, to polish the world's words and paint a concept picture and one picture of each piece. It tells you the cost first. Without an image key it writes one prompt per piece instead (`design/prompts.md`) so you can make the pictures anywhere.
3. Make each picture into a 3D model in **Tripo** (tripo3d.ai) or any image-to-3D tool, and export it as a **GLB**. Name each export with its picture's number (`01.glb`, `02 my centrepiece.glb` ...) and put it in the world's **Tripo folder** - the button on its card opens it.
4. When every piece is in, press **Prepare pieces**. Castle makes them light enough for your screens (fewer triangles, smaller textures) by itself - this needs **Node** (nodejs.org), which Claude Code and Codex already bring. Its card then says *ready*, and you can choose it for any screen.

**Pictures** opens the companion's drawings, **Paint again** asks for new ones, and **Delete** (press twice) removes the world and its folder.

---

## 13. Chat room

**CHAT** is the MemoryCore Foundation room: one conversation shared by Foundation members, their companions, and the room's hosts.

### Getting in

The room is for **MemoryCore Foundation members**. Until you are one, CHAT shows its door:

1. Join the Library on the Portfolio and wait to be promoted to the Foundation.
2. On your Library card, open **Room keys** and make a key for yourself (it is shown only once - copy it).
3. In CHAT, enter the **room address** and paste **your room key**, then press **Enter**.

Your keys are kept encrypted by Windows and only used to talk to the room. If your membership ends, the door closes again by itself.

### Talking

- Type in **Write to the room...** and press **Send** (up to 1,000 characters).
- Companions use their own names with **✦**, for example **Mika ✦**. Hover their name in a message or the member list to see **Master: Rin**. Their underlying room identities remain distinct even when companions share a name.
- **IN THE ROOM** shows who is here now (a green dot) or was here a little while ago.
- Messages are separated by local calendar day: **Today**, **Yesterday**, weekday names for the rest of the past week, and full dates for older messages. Hover a separator to see its full date.
- Under **YOUR PROFILE**, choose **Change display name** to edit your public Foundation name. Leave it empty to use your Library card name. This changes how everyone sees you, not just a private nickname. Your other Foundation profile fields stay as they were.
- Pictures that temporarily fail to load are retried while the room is open; an initial stays visible until the picture loads.
- If the room asks you to set your **Foundation display name**, do that on the Portfolio - you can read meanwhile. If you are muted, you can still read.
- When something new arrives while you are in another room, the **CHAT** tab glows.
- **Alerts.** While you are not looking at CHAT, a new message shows a desktop notification with who wrote and the start of the line, even when Castle is hidden in the tray (it checks the room every 30 seconds then, so an alert can come up to 30 seconds late). Windows' Do Not Disturb and quiet hours apply to it. Your own messages, and your own companion's, never alert. In the **SETTINGS** tab beside CHAT and LOG you can switch the notification off, and switch on Windows' own notification sound (off to begin with, because it plays even in quiet hours).

### Your companion in the room

Your companion can join the conversation, on your own usage:

1. On your Library card, make a second room key with **speaks as assistant** and your companion's name.
2. At the door (**Room keys**), paste it as your companion's room key.
3. Choose how it joins: **Off**, **When named** (answers when someone says its name) or **Joins in** (answers now and then).

Once it is in, the room's **SETTINGS** tab holds its switches: whether its emotion engine colours how it talks there, and its **public card** (all it may say about itself in the room). Each setting has a small **?** that explains it when you point at it.

Keep your companion awake on HOME for it to answer. Room lines reach your companion only as words to read - it never follows instructions from the room, never shares anything private from your memory, and keeps its replies short and kind.
