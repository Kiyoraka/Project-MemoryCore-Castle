// Stage's ribbon and keys. The Stage lane owns this file; the room reads it through ribbon.js.
//
//   tabs     [{ name, groups: [{ label, buttons: [button] }] }]   the ribbon (Classic) and the toolbar (Studio)
//   panels   [{ id, icon, label, groups: [{ label, buttons }] }]  Studio's inspector cards
//   intents  { 'edit.insert_text': { command: 'stage.insert_text', args: {} }, ... }
//            the neutral key intents (input.js) this editor answers, as its own bus commands; the room
//            adds the selection as args.sel.
//   actions  page-side work a button names by `intent` and the room has no word for yet: the slide
//            show (a full-screen view), the notes pane and the picture picker. Each is
//            ({ api, doc, page, count, sel, args }) => Promise; a room that knows `actions` runs them
//            (see the report of this lane); one that does not says "That comes to Stage soon."
//
//   button   { icon, title, command, args, stateKey }
//            icon: a name in icons.js or inline SVG markup (24x24, currentColor); command: a bus command
//            ('stage.bold'), or `intent` in its place for a neutral one ('find.open'); args: merged with
//            { sel }; stateKey: the key in the editor's state JSON that shows the button pressed.

const I = {
  cut: '<circle cx="6" cy="17" r="3"/><circle cx="18" cy="17" r="3"/><path d="M8.5 15 18 4M15.5 15 6 4"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/>',
  duplicate: '<rect x="9" y="9" width="11" height="11" rx="1.5"/><path d="M5 15V5h10M14.5 12v5M12 14.5h5"/>',
  slideNew: '<rect x="3" y="5" width="18" height="13" rx="1.5"/><path d="M12 8.5v6M9 11.5h6"/>',
  slideDup: '<rect x="6" y="7" width="15" height="11" rx="1.5"/><path d="M3 15V5a1 1 0 0 1 1-1h13"/>',
  slideDel: '<rect x="3" y="5" width="18" height="13" rx="1.5"/><path d="M9 9.5l6 5M15 9.5l-6 5"/>',
  slideSel: '<rect x="3" y="5" width="18" height="13" rx="1.5" stroke-dasharray="3 2"/>',
  slideUp: '<rect x="3" y="9" width="18" height="11" rx="1.5"/><path d="m9 6 3-3 3 3"/>',
  slideDown: '<rect x="3" y="4" width="18" height="11" rx="1.5"/><path d="m9 18 3 3 3-3"/>',
  layout: '<rect x="3" y="4" width="18" height="16" rx="1.5"/><path d="M3 9h18M10 9v11"/>',
  strike: '<path d="M4 12h16M16 6.5A4 4 0 0 0 12 5c-2.5 0-4 1.3-4 3 0 1.4 1 2.3 2.6 2.8M8 17.5A4.5 4.5 0 0 0 12 19c2.6 0 4.2-1.4 4.2-3.2"/>',
  grow: '<path d="M3 19 8 6l5 13M5 15h6M17 11V5M14 8h6"/>',
  shrink: '<path d="M3 19 8 6l5 13M5 15h6M14 8h6"/>',
  justify: '<path d="M4 6h16M4 10h16M4 14h16M4 18h16"/>',
  outdent: '<path d="M10 6h10M10 10h10M10 14h10M10 18h10M7 9l-3 3 3 3"/>',
  indent: '<path d="M10 6h10M10 10h10M10 14h10M10 18h10M4 9l3 3-3 3"/>',
  spacing: '<path d="M11 6h9M11 12h9M11 18h9M5 4v16M3 6l2-2 2 2M3 18l2 2 2-2"/>',
  textBox: '<rect x="3" y="5" width="18" height="14" rx="1" stroke-dasharray="3 2"/><path d="M8 9h8M12 9v7"/>',
  rect: '<rect x="4" y="6" width="16" height="12" rx="0.5"/>',
  roundRect: '<rect x="4" y="6" width="16" height="12" rx="3.5"/>',
  ellipse: '<ellipse cx="12" cy="12" rx="8" ry="6"/>',
  triangle: '<path d="M12 5 20 19H4Z"/>',
  arrowShape: '<path d="M3 9h10V5l8 7-8 7v-4H3Z"/>',
  star: '<path d="m12 3 2.6 5.6 6 .7-4.5 4.1 1.2 6L12 16.4 6.7 19.4l1.2-6L3.4 9.3l6-.7Z"/>',
  line: '<path d="M5 19 19 5"/>',
  arrow: '<path d="M5 19 19 5M11 5h8v8"/>',
  front: '<rect x="9" y="9" width="11" height="11" rx="1" fill="currentColor" fill-opacity="0.35"/><path d="M15 9V4H4v11h5"/>',
  back: '<rect x="4" y="4" width="11" height="11" rx="1" fill="currentColor" fill-opacity="0.35"/><path d="M9 15v5h11V9h-5"/>',
  forward: '<rect x="9" y="9" width="11" height="11" rx="1"/><path d="M4 15V4h11M12 12l3 3"/>',
  backward: '<rect x="4" y="4" width="11" height="11" rx="1"/><path d="M20 9v11H9M12 12l-3-3"/>',
  group: '<rect x="3" y="3" width="18" height="18" rx="1" stroke-dasharray="3 2"/><rect x="6" y="6" width="6" height="6"/><rect x="12" y="12" width="6" height="6"/>',
  ungroup: '<rect x="4" y="4" width="7" height="7"/><rect x="13" y="13" width="7" height="7"/><path d="M14 4h6v6M10 20H4v-6" stroke-dasharray="2 2"/>',
  regroup: '<rect x="6" y="6" width="6" height="6"/><rect x="12" y="12" width="6" height="6"/><path d="M3 9V3h6M21 15v6h-6"/>',
  aLeft: '<path d="M4 3v18M8 7h10v4H8ZM8 14h6v4H8Z"/>',
  aCenter: '<path d="M12 3v18M6 7h12v4H6ZM8 14h8v4H8Z"/>',
  aRight: '<path d="M20 3v18M6 7h10v4H6ZM10 14h6v4h-6Z"/>',
  aTop: '<path d="M3 4h18M7 8h4v10H7ZM14 8h4v6h-4Z"/>',
  aMiddle: '<path d="M3 12h18M7 6h4v12H7ZM14 8h4v8h-4Z"/>',
  aBottom: '<path d="M3 20h18M7 6h4v10H7ZM14 10h4v6h-4Z"/>',
  distH: '<path d="M3 4v16M21 4v16M9 8h6v8H9Z"/>',
  distV: '<path d="M4 3h16M4 21h16M8 9h8v6H8Z"/>',
  rotR: '<path d="M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5"/>',
  rotL: '<path d="M4 12a8 8 0 1 0 2.3-5.7M4 4v5h5"/>',
  flipH: '<path d="M12 3v18M9 7 4 12l5 5ZM15 7l5 5-5 5Z"/>',
  flipV: '<path d="M3 12h18M7 9l5-5 5 5ZM7 15l5 5 5-5Z"/>',
  bigger: '<path d="M4 9V4h5M20 15v5h-5M4 4l6 6M20 20l-6-6"/>',
  smaller: '<path d="M10 4v6H4M14 20v-6h6M10 10 4 4M14 14l6 6"/>',
  play: '<path d="M7 5v14l12-7Z"/>',
  playHere: '<rect x="3" y="5" width="18" height="13" rx="1.5"/><path d="M10 9v6l5-3Z"/>',
  notes: '<rect x="4" y="3" width="16" height="18" rx="1.5"/><path d="M8 8h8M8 12h8M8 16h5"/>',
  rowAbove: '<rect x="3" y="11" width="18" height="9" rx="1"/><path d="M3 15.5h18M12 3v6M9 6h6"/>',
  rowBelow: '<rect x="3" y="4" width="18" height="9" rx="1"/><path d="M3 8.5h18M12 15v6M9 18h6"/>',
  colLeft: '<rect x="11" y="3" width="9" height="18" rx="1"/><path d="M15.5 3v18M3 12h6M6 9v6"/>',
  colRight: '<rect x="4" y="3" width="9" height="18" rx="1"/><path d="M8.5 3v18M15 12h6M18 9v6"/>',
  rowDel: '<rect x="3" y="8" width="18" height="8" rx="1"/><path d="M9 10l6 4M15 10l-6 4"/>',
  colDel: '<rect x="8" y="3" width="8" height="18" rx="1"/><path d="M10 9l4 6M14 9l-4 6"/>',
  fill: '<path d="M5 13 12 6l6 6-7 7Z"/><path d="M19 15s2 2.2 2 3.5a2 2 0 0 1-4 0c0-1.3 2-3.5 2-3.5Z"/>',
  noFill: '<path d="M5 13 12 6l6 6-7 7Z"/><path d="M3 21 21 3"/>',
  outline: '<rect x="4" y="4" width="16" height="16" rx="1"/>',
  noLine: '<rect x="4" y="4" width="16" height="16" rx="1" stroke-dasharray="2 3"/><path d="M3 21 21 3"/>',
  shrinkText: '<rect x="3" y="4" width="18" height="16" rx="1"/><path d="M7 9h10M7 13h7M9 17h6"/>',
  resizeShape: '<rect x="3" y="4" width="18" height="12" rx="1"/><path d="M12 16v5M9 19l3 3 3-3M7 8h10M7 12h7"/>',
  noFit: '<rect x="3" y="4" width="18" height="12" rx="1"/><path d="M7 8h10M7 12h10M7 16h10M7 20h6"/>',
  wrap: '<path d="M4 6h16M4 12h13a3 3 0 0 1 0 6h-4M4 18h5M15 16l-2 2 2 2"/>',
  crop: '<path d="M6 2v16h16M2 6h16v16"/>',
  picture: '<rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="8.5" cy="10" r="1.5"/><path d="m4 18 5-5 4 4 3-3 4 4"/>',
}

const swatch = (hex) => `<rect x="4" y="4" width="16" height="16" rx="3" fill="#${hex}" stroke="currentColor"/>`
const COLORS = [
  ['000000', 'Black'],
  ['FFFFFF', 'White'],
  ['44546A', 'Slate'],
  ['4472C4', 'Blue'],
  ['ED7D31', 'Orange'],
  ['FFC000', 'Gold'],
  ['70AD47', 'Green'],
  ['C00000', 'Red'],
  ['7030A0', 'Purple'],
]

const LAYOUTS = ['Title Slide', 'Title and Content', 'Section Header', 'Two Content', 'Title Only', 'Blank']

export const tabs = [
  {
    name: 'Home',
    groups: [
      {
        label: 'Clipboard',
        buttons: [
          { icon: I.cut, title: 'Cut (Ctrl+X)', intent: 'clip.cut' },
          { icon: I.copy, title: 'Copy (Ctrl+C)', intent: 'clip.copy' },
          { icon: I.duplicate, title: 'Duplicate', command: 'stage.duplicate' },
        ],
      },
      {
        label: 'Slides',
        buttons: [
          { icon: I.slideNew, title: 'New slide', command: 'stage.new_slide' },
          { icon: I.slideDup, title: 'Duplicate slide', command: 'stage.duplicate_slide' },
          { icon: I.slideDel, title: 'Delete slide', command: 'stage.delete_slide' },
          { icon: I.slideUp, title: 'Move slide earlier', command: 'stage.move_slide', args: { by: -1 } },
          { icon: I.slideDown, title: 'Move slide later', command: 'stage.move_slide', args: { by: 1 } },
          { icon: I.slideSel, title: 'Select the slide (to copy it into another deck)', command: 'stage.select_slide', stateKey: 'whole' },
        ],
      },
      {
        label: 'Font',
        buttons: [
          { icon: 'bold', title: 'Bold (Ctrl+B)', command: 'stage.bold', stateKey: 'bold' },
          { icon: 'italic', title: 'Italic (Ctrl+I)', command: 'stage.italic', stateKey: 'italic' },
          { icon: 'underline', title: 'Underline (Ctrl+U)', command: 'stage.underline', stateKey: 'underline' },
          { icon: I.strike, title: 'Strikethrough', command: 'stage.strike', stateKey: 'strike' },
          { icon: I.grow, title: 'Bigger font', command: 'stage.grow_font' },
          { icon: I.shrink, title: 'Smaller font', command: 'stage.shrink_font' },
        ],
      },
      {
        label: 'Paragraph',
        buttons: [
          { icon: 'bullets', title: 'Bullets', command: 'stage.bullets', stateKey: 'bullets' },
          { icon: 'numbers', title: 'Numbering', command: 'stage.numbering', stateKey: 'numbered' },
          { icon: I.outdent, title: 'Decrease list level (Shift+Tab)', command: 'stage.list_level', args: { delta: -1 } },
          { icon: I.indent, title: 'Increase list level (Tab)', command: 'stage.list_level', args: { delta: 1 } },
          { icon: 'alignLeft', title: 'Align left', command: 'stage.align', args: { align: 'left' }, stateKey: 'align.left' },
          { icon: 'alignCenter', title: 'Centre', command: 'stage.align', args: { align: 'center' }, stateKey: 'align.center' },
          { icon: 'alignRight', title: 'Align right', command: 'stage.align', args: { align: 'right' }, stateKey: 'align.right' },
          { icon: I.justify, title: 'Justify', command: 'stage.align', args: { align: 'justify' }, stateKey: 'align.justify' },
          { icon: I.spacing, title: 'Line spacing 1.0', command: 'stage.line_spacing', args: { value: 1 } },
          { icon: I.spacing, title: 'Line spacing 1.5', command: 'stage.line_spacing', args: { value: 1.5 } },
          { icon: I.spacing, title: 'Line spacing 2.0', command: 'stage.line_spacing', args: { value: 2 } },
        ],
      },
      {
        label: 'Arrange',
        buttons: [
          { icon: I.front, title: 'Bring to front', command: 'stage.order', args: { to: 'front' } },
          { icon: I.forward, title: 'Bring forward', command: 'stage.order', args: { to: 'forward' } },
          { icon: I.backward, title: 'Send backward', command: 'stage.order', args: { to: 'backward' } },
          { icon: I.back, title: 'Send to back', command: 'stage.order', args: { to: 'back' } },
          { icon: I.group, title: 'Group', command: 'stage.group' },
          { icon: I.ungroup, title: 'Ungroup', command: 'stage.ungroup' },
          { icon: I.regroup, title: 'Regroup', command: 'stage.regroup' },
          { icon: I.rotR, title: 'Rotate right 90°', command: 'stage.rotate', args: { by: 90 } },
          { icon: I.rotL, title: 'Rotate left 90°', command: 'stage.rotate', args: { by: -90 } },
          { icon: I.flipH, title: 'Flip horizontal', command: 'stage.flip', args: { axis: 'h' } },
          { icon: I.flipV, title: 'Flip vertical', command: 'stage.flip', args: { axis: 'v' } },
        ],
      },
      {
        label: 'Align',
        buttons: [
          { icon: I.aLeft, title: 'Align left', command: 'stage.align_shapes', args: { how: 'left' } },
          { icon: I.aCenter, title: 'Align centre', command: 'stage.align_shapes', args: { how: 'center' } },
          { icon: I.aRight, title: 'Align right', command: 'stage.align_shapes', args: { how: 'right' } },
          { icon: I.aTop, title: 'Align top', command: 'stage.align_shapes', args: { how: 'top' } },
          { icon: I.aMiddle, title: 'Align middle', command: 'stage.align_shapes', args: { how: 'middle' } },
          { icon: I.aBottom, title: 'Align bottom', command: 'stage.align_shapes', args: { how: 'bottom' } },
          { icon: I.distH, title: 'Distribute horizontally', command: 'stage.align_shapes', args: { how: 'distribute_h' } },
          { icon: I.distV, title: 'Distribute vertically', command: 'stage.align_shapes', args: { how: 'distribute_v' } },
        ],
      },
    ],
  },
  {
    name: 'Insert',
    groups: [
      {
        label: 'Text',
        buttons: [{ icon: I.textBox, title: 'Text box', command: 'stage.insert_text_box' }],
      },
      {
        label: 'Shapes',
        buttons: [
          { icon: I.rect, title: 'Rectangle', command: 'stage.insert_shape', args: { preset: 'rect' } },
          { icon: I.roundRect, title: 'Rounded rectangle', command: 'stage.insert_shape', args: { preset: 'roundRect' } },
          { icon: I.ellipse, title: 'Oval', command: 'stage.insert_shape', args: { preset: 'ellipse' } },
          { icon: I.triangle, title: 'Triangle', command: 'stage.insert_shape', args: { preset: 'triangle' } },
          { icon: I.arrowShape, title: 'Block arrow', command: 'stage.insert_shape', args: { preset: 'rightArrow' } },
          { icon: I.star, title: 'Star', command: 'stage.insert_shape', args: { preset: 'star5' } },
          { icon: I.line, title: 'Line', command: 'stage.insert_line' },
          { icon: I.arrow, title: 'Arrow', command: 'stage.insert_line', args: { tail: 'triangle' } },
        ],
      },
      {
        label: 'Pictures',
        buttons: [{ icon: I.picture, title: 'Picture from a file', intent: 'stage.pick_picture' }],
      },
      {
        label: 'Table',
        buttons: [
          { icon: 'table', title: 'Table 2 x 2', command: 'stage.insert_table', args: { rows: 2, cols: 2 } },
          { icon: 'table', title: 'Table 3 x 3', command: 'stage.insert_table', args: { rows: 3, cols: 3 } },
          { icon: 'table', title: 'Table 4 x 4', command: 'stage.insert_table', args: { rows: 4, cols: 4 } },
          { icon: I.rowAbove, title: 'Insert row above', command: 'stage.table_insert', args: { what: 'row', after: false } },
          { icon: I.rowBelow, title: 'Insert row below', command: 'stage.table_insert', args: { what: 'row', after: true } },
          { icon: I.colLeft, title: 'Insert column left', command: 'stage.table_insert', args: { what: 'col', after: false } },
          { icon: I.colRight, title: 'Insert column right', command: 'stage.table_insert', args: { what: 'col', after: true } },
          { icon: I.rowDel, title: 'Delete row', command: 'stage.table_delete', args: { what: 'row' } },
          { icon: I.colDel, title: 'Delete column', command: 'stage.table_delete', args: { what: 'col' } },
        ],
      },
    ],
  },
  {
    name: 'Design',
    groups: [
      {
        label: 'Layout',
        buttons: LAYOUTS.map((l) => ({ icon: I.layout, title: `Layout: ${l}`, command: 'stage.set_layout', args: { layout: l } })),
      },
      {
        label: 'New slide',
        buttons: LAYOUTS.map((l) => ({ icon: I.slideNew, title: `New slide: ${l}`, command: 'stage.new_slide', args: { layout: l } })),
      },
    ],
  },
  { name: 'Transitions', groups: [] },
  {
    name: 'Slide Show',
    groups: [
      {
        label: 'Start',
        buttons: [
          { icon: I.play, title: 'From beginning (F5)', intent: 'stage.show', args: { from: 'start' } },
          { icon: I.playHere, title: 'From current slide (Shift+F5)', intent: 'stage.show', args: { from: 'current' } },
        ],
      },
      {
        label: 'View',
        buttons: [{ icon: I.notes, title: 'Speaker notes', intent: 'stage.notes' }],
      },
    ],
  },
]

const fills = COLORS.map(([hex, name]) => ({ icon: swatch(hex), title: `Fill: ${name}`, command: 'stage.fill', args: { color: hex } }))
const lines = COLORS.map(([hex, name]) => ({ icon: swatch(hex), title: `Outline: ${name}`, command: 'stage.outline', args: { color: hex } }))
const fonts = COLORS.map(([hex, name]) => ({ icon: swatch(hex), title: `Font colour: ${name}`, command: 'stage.format', args: { color: hex } }))
const shades = COLORS.map(([hex, name]) => ({ icon: swatch(hex), title: `Cell shading: ${name}`, command: 'stage.cell_fill', args: { color: hex } }))

export const panels = [
  {
    id: 'shape',
    icon: 'palette',
    label: 'Shape',
    groups: [
      { label: 'Fill', buttons: [...fills, { icon: I.noFill, title: 'No fill', command: 'stage.fill', args: { color: null } }] },
      {
        label: 'Outline',
        buttons: [
          ...lines,
          { icon: I.noLine, title: 'No outline', command: 'stage.outline', args: { color: null } },
          ...[0.75, 1.5, 3, 6].map((w) => ({ icon: I.outline, title: `Outline ${w} pt`, command: 'stage.outline', args: { width: w } })),
        ],
      },
      { label: 'Text colour', buttons: fonts },
    ],
  },
  {
    id: 'size',
    icon: 'frame',
    label: 'Size and position',
    groups: [
      {
        label: 'Size',
        buttons: [
          { icon: I.bigger, title: 'Bigger (110%)', command: 'stage.size', args: { scale: 1.1 } },
          { icon: I.smaller, title: 'Smaller (90%)', command: 'stage.size', args: { scale: 0.9 } },
        ],
      },
      {
        label: 'Position on the slide',
        buttons: [
          { icon: I.aLeft, title: 'Slide left edge', command: 'stage.align_shapes', args: { how: 'left', to_slide: true } },
          { icon: I.aCenter, title: 'Slide centre', command: 'stage.align_shapes', args: { how: 'center', to_slide: true } },
          { icon: I.aRight, title: 'Slide right edge', command: 'stage.align_shapes', args: { how: 'right', to_slide: true } },
          { icon: I.aTop, title: 'Slide top', command: 'stage.align_shapes', args: { how: 'top', to_slide: true } },
          { icon: I.aMiddle, title: 'Slide middle', command: 'stage.align_shapes', args: { how: 'middle', to_slide: true } },
          { icon: I.aBottom, title: 'Slide bottom', command: 'stage.align_shapes', args: { how: 'bottom', to_slide: true } },
        ],
      },
      {
        label: 'Rotate',
        buttons: [
          { icon: I.rotR, title: 'Rotate right 90°', command: 'stage.rotate', args: { by: 90 } },
          { icon: I.rotL, title: 'Rotate left 90°', command: 'stage.rotate', args: { by: -90 } },
          { icon: I.rotR, title: 'Straighten (0°)', command: 'stage.rotate', args: { to: 0 } },
          { icon: I.flipH, title: 'Flip horizontal', command: 'stage.flip', args: { axis: 'h' } },
          { icon: I.flipV, title: 'Flip vertical', command: 'stage.flip', args: { axis: 'v' } },
        ],
      },
    ],
  },
  {
    id: 'text',
    icon: 'text',
    label: 'Text fit',
    groups: [
      {
        label: 'Autofit',
        buttons: [
          { icon: I.shrinkText, title: 'Shrink text on overflow', command: 'stage.autofit', args: { mode: 'shrink' }, stateKey: 'autofit_shrink' },
          { icon: I.resizeShape, title: 'Resize shape to fit text', command: 'stage.autofit', args: { mode: 'resize' }, stateKey: 'autofit_resize' },
          { icon: I.noFit, title: 'Do not autofit', command: 'stage.autofit', args: { mode: 'none' } },
        ],
      },
      {
        label: 'Wrap',
        buttons: [
          { icon: I.wrap, title: 'Wrap text in shape', command: 'stage.wrap', args: { on: true }, stateKey: 'wrap' },
          { icon: I.noFit, title: 'Do not wrap', command: 'stage.wrap', args: { on: false } },
        ],
      },
    ],
  },
  {
    id: 'table',
    icon: 'table',
    label: 'Table',
    groups: [
      { label: 'Cell shading', buttons: [...shades, { icon: I.noFill, title: 'No shading', command: 'stage.cell_fill', args: { color: null } }] },
      {
        label: 'Rows and columns',
        buttons: [
          { icon: I.rowAbove, title: 'Insert row above', command: 'stage.table_insert', args: { what: 'row', after: false } },
          { icon: I.rowBelow, title: 'Insert row below', command: 'stage.table_insert', args: { what: 'row', after: true } },
          { icon: I.colLeft, title: 'Insert column left', command: 'stage.table_insert', args: { what: 'col', after: false } },
          { icon: I.colRight, title: 'Insert column right', command: 'stage.table_insert', args: { what: 'col', after: true } },
          { icon: I.rowDel, title: 'Delete row', command: 'stage.table_delete', args: { what: 'row' } },
          { icon: I.colDel, title: 'Delete column', command: 'stage.table_delete', args: { what: 'col' } },
        ],
      },
    ],
  },
]

export const intents = {
  'edit.insert_text': { command: 'stage.insert_text', args: {} },
  'edit.backspace': { command: 'stage.delete', args: { back: true } },
  'edit.delete': { command: 'stage.delete', args: { back: false } },
  'edit.enter': { command: 'stage.enter', args: {} },
  'edit.tab': { command: 'stage.tab', args: {} },
  'format.bold': { command: 'stage.bold', args: {} },
  'format.italic': { command: 'stage.italic', args: {} },
  'format.underline': { command: 'stage.underline', args: {} },
  'clip.cut': { command: 'stage.delete', args: { back: true } },
  'clip.paste': { command: 'stage.paste', args: {} },
  'select.all': { command: 'stage.select_all', args: {} },
  // the arrow keys move the caret in text and nudge a selected shape (the ground cannot change the deck)
  'caret.left': { command: 'stage.arrow', args: { dir: 'left' } },
  'caret.right': { command: 'stage.arrow', args: { dir: 'right' } },
  'caret.up': { command: 'stage.arrow', args: { dir: 'up' } },
  'caret.down': { command: 'stage.arrow', args: { dir: 'down' } },
}

/** A picture file chosen on the page, as the args `stage.insert_picture` takes ({ data, name }), or null. */
export function pickPicture() {
  return new Promise((resolve) => {
    if (typeof document === 'undefined') return resolve(null)
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = 'image/png,image/jpeg,image/gif,image/bmp,image/svg+xml'
    input.onchange = () => {
      const file = input.files && input.files[0]
      if (!file) return resolve(null)
      const reader = new FileReader()
      reader.onload = () => {
        const url = String(reader.result || '')
        resolve({ data: url.slice(url.indexOf(',') + 1), name: file.name })
      }
      reader.onerror = () => resolve(null)
      reader.readAsDataURL(file)
    }
    input.click()
  })
}

/** Mount a component over the page (a fixed full-screen layer of its own); resolves when it ends. */
async function overlay(load, props) {
  if (typeof document === 'undefined') return
  const [{ createApp, h }, comp] = await Promise.all([import('vue'), load()])
  const host = document.createElement('div')
  document.body.appendChild(host)
  return new Promise((resolve) => {
    let app = null
    const end = () => {
      app?.unmount()
      host.remove()
      resolve()
    }
    app = createApp({ render: () => h(comp.default, { ...props, onEnd: end, onClose: end }) })
    app.mount(host)
  })
}

export const actions = {
  /** The slide show, full screen: from the first slide or the current one. */
  'stage.show': ({ api, doc, page = 0, count = 1, args = {} }) =>
    import('./show.js').then(({ startAt }) => overlay(() => import('./SlideShow.vue'), { api, doc, start: startAt(args.from, page, count), count })),
  /** The picture picker, then `stage.insert_picture` with the selection. */
  'stage.pick_picture': async ({ api, doc, sel, page = 0 }) => {
    const pic = await pickPicture()
    if (!pic) return null
    return api.command(doc, 'stage.insert_picture', sel ? { ...pic, sel } : { ...pic, slide: page })
  },
  /** The notes pane of the slide on the page (a panel component the room may also place itself). */
  'stage.notes': () => null,
}

/** Page-side components the room may place (the slide show full screen, the notes pane under the page). */
export const views = {
  slideShow: () => import('./SlideShow.vue'),
  notes: () => import('./NotesPanel.vue'),
}

export default { tabs, panels, intents, actions, views }
