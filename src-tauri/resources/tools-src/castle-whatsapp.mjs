// MemoryCore in your pocket, WhatsApp: a small local sidecar Castle spawns, speaking Baileys
// (a WhatsApp Web client) on one side and a plain localhost HTTP door on the other, so the Rust
// side never needs to know anything about WhatsApp's own protocol - only this door's shape.
//
// Env vars Castle sets when it starts this: WA_AUTH_DIR (where the paired session is kept, so a
// restart does not ask for the QR again), WA_PORT (the local port this door listens on),
// WA_MEDIA_DIR (where a picture sent in a chat is saved for Castle to take; Castle moves it on or
// deletes it).
//
// This file is a SOURCE, not what Castle ships. It is bundled into one dependency-free file with
// esbuild (Baileys needs real npm packages; bundling means Castle ships one script, the same way
// it ships castle-image.mjs, with nothing to npm install on the member's machine):
//
//   npm install @whiskeysockets/baileys
//   npx esbuild castle-whatsapp.mjs --bundle --platform=node --format=cjs --outfile=../tools/castle-whatsapp.cjs
//
// (--format=cjs, not esm: some of Baileys' own dependencies use a dynamic require() for Node's
// builtin modules, which only resolves correctly in a CommonJS bundle.)

import http from 'node:http'
import fs from 'node:fs'
import path from 'node:path'
import { makeWASocket, useMultiFileAuthState, DisconnectReason, isJidGroup, isJidBroadcast, isJidStatusBroadcast, downloadContentFromMessage } from '@whiskeysockets/baileys'

// v1: one-to-one chats only, no groups/broadcasts/status updates
const isOneToOne = (jid) => !!jid && !isJidGroup(jid) && !isJidBroadcast(jid) && !isJidStatusBroadcast(jid)

const AUTH_DIR = process.env.WA_AUTH_DIR ?? './castle-whatsapp-auth'
const PORT = Number(process.env.WA_PORT ?? 51847)
const MEDIA_DIR = process.env.WA_MEDIA_DIR ?? null
// a picture over this is not saved (Castle takes receipts up to 25 MB)
const MAX_PICTURE = 25 * 1024 * 1024

// A chat's picture saved for Castle, or null (no folder, too big, or the download failed).
async function savePicture(id, image) {
  try {
    if (!MEDIA_DIR || !image || Number(image.fileLength ?? 0) > MAX_PICTURE) return null
    const parts = []
    let size = 0
    for await (const chunk of await downloadContentFromMessage(image, 'image')) {
      size += chunk.length
      if (size > MAX_PICTURE) return null
      parts.push(chunk)
    }
    fs.mkdirSync(MEDIA_DIR, { recursive: true })
    const file = path.join(MEDIA_DIR, `${String(id).replace(/[^A-Za-z0-9]/g, '') || Date.now()}.img`)
    fs.writeFileSync(file, Buffer.concat(parts))
    return file
  } catch {
    return null
  }
}

let sock = null
let state = null
let saveCreds = null
let latestQr = null
let connected = false
let lastProblem = null
const incoming = []

async function connect() {
  const auth = await useMultiFileAuthState(AUTH_DIR)
  state = auth.state
  saveCreds = auth.saveCreds
  sock = makeWASocket({ auth: state, printQRInTerminal: false, syncFullHistory: false })

  sock.ev.on('creds.update', saveCreds)

  sock.ev.on('connection.update', (update) => {
    if (update.qr) latestQr = update.qr
    if (update.connection === 'open') {
      connected = true
      latestQr = null
      lastProblem = null
    }
    if (update.connection === 'close') {
      connected = false
      const code = update.lastDisconnect?.error?.output?.statusCode
      if (code === DisconnectReason.loggedOut) {
        lastProblem = 'Logged out on the phone. Scan a new QR to pair again.'
      } else {
        lastProblem = 'Reconnecting…'
        setTimeout(connect, 3000)
      }
    }
  })

  sock.ev.on('messages.upsert', async ({ messages }) => {
    for (const m of messages) {
      if (m.key.fromMe) continue
      const chatId = m.key.remoteJid
      if (!isOneToOne(chatId)) continue
      // the phone number behind the chat when WhatsApp gives it (a chat id can be a hidden-number one)
      const pn = m.key.senderPn ?? m.key.remoteJidAlt ?? null
      const name = m.pushName ?? 'Someone'
      const reaction = m.message?.reactionMessage
      if (reaction) {
        // a heart on one of Castle's own messages; an empty one means it was taken back
        incoming.push({ id: m.key.id, chatId, pn, name, kind: 'reaction', text: '', emoji: reaction.text ?? '', onId: reaction.key?.id ?? null })
        continue
      }
      const text = (m.message?.conversation ?? m.message?.extendedTextMessage?.text ?? '').trim()
      if (text) {
        incoming.push({ id: m.key.id, chatId, pn, name, kind: 'text', text })
        continue
      }
      // a picture is saved for Castle (`file`); a voice note or video arrives as a line only
      const media = m.message?.imageMessage ? 'a picture' : m.message?.audioMessage ? 'a voice note' : m.message?.videoMessage ? 'a video' : null
      if (media) {
        const caption = (m.message?.imageMessage?.caption ?? m.message?.videoMessage?.caption ?? '').trim()
        const file = m.message?.imageMessage ? await savePicture(m.key.id, m.message.imageMessage) : null
        incoming.push({ id: m.key.id, chatId, pn, name, kind: 'media', text: `(sent ${media})${caption ? ' ' + caption : ''}`, file })
      }
    }
  })
}

function json(res, code, body) {
  res.writeHead(code, { 'Content-Type': 'application/json' })
  res.end(JSON.stringify(body))
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let body = ''
    req.on('data', (chunk) => { body += chunk })
    req.on('end', () => resolve(body))
    req.on('error', reject)
  })
}

const server = http.createServer(async (req, res) => {
  try {
    // local-only by design: this door is bound to 127.0.0.1 below, never the network
    if (req.method === 'GET' && req.url === '/status') {
      return json(res, 200, { paired: !!(state?.creds?.me || state?.creds?.registered), connected, qr: latestQr, problem: lastProblem })
    }
    if (req.method === 'GET' && req.url === '/incoming') {
      const drained = incoming.splice(0, incoming.length)
      return json(res, 200, { messages: drained })
    }
    if (req.method === 'POST' && req.url === '/send') {
      const { to, text } = JSON.parse(await readBody(req))
      const sent = await sock.sendMessage(to, { text })
      return json(res, 200, { ok: true, id: sent?.key?.id ?? null })
    }
    json(res, 404, { error: 'not found' })
  } catch (error) {
    json(res, 500, { error: String(error?.message ?? error) })
  }
})

server.listen(PORT, '127.0.0.1', () => {
  console.log(`castle-whatsapp listening on 127.0.0.1:${PORT}`)
})

connect().catch((error) => {
  console.error('castle-whatsapp could not start', error)
  process.exit(1)
})
