// The studio's colours: each section of the song and each stem lane has its own (as in the chosen
// design: violet, teal, amber, blue, rose...).
export const PALETTE = ['#b46cff', '#2fd3c4', '#f5a64a', '#4f7dff', '#ff5fa8', '#8fe3b8', '#ffd27a', '#7fb4ff']

export const sectionColor = (n) => PALETTE[((n % PALETTE.length) + PALETTE.length) % PALETTE.length]

// A glyph for a stem by its name (no picture files needed).
const GLYPHS = [
  [/vocal|voice|vox|sing/i, '♪'],
  [/drum|perc|kick|snare/i, '◎'],
  [/bass/i, '♩'],
  [/guitar/i, '♬'],
  [/piano|key/i, '♮'],
  [/synth|fx|pad/i, '✦'],
]
export const stemGlyph = (name) => GLYPHS.find(([re]) => re.test(name))?.[1] ?? '♫'
