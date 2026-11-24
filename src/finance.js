// FINANCE's words and sums: money in the member's own format, a month's totals per currency
// (never added across currencies), and the one line a drop answers with.

export const CATEGORIES = ['food', 'groceries', 'transport', 'bills', 'shopping', 'health', 'home', 'fun', 'other']

/** "Groceries" from "groceries". */
export const categoryLabel = (id) => (id ? id[0].toUpperCase() + id.slice(1) : '')

/** 1250 cents in MYR -> "MYR 12.50" (the member's locale, with the code always shown). */
export function money(cents, currency, locale = undefined) {
  const value = (Number(cents) || 0) / 100
  try {
    const n = new Intl.NumberFormat(locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(value)
    return `${currency} ${n}`
  } catch {
    return `${currency} ${value.toFixed(2)}`
  }
}

/** "12.50" from 1250 cents: what the edit box holds and the core reads back. */
export const amountText = (cents) => `${Math.floor(Math.abs(cents) / 100)}.${String(Math.abs(cents) % 100).padStart(2, '0')}`

/**
 * The month's totals grouped by currency, biggest first, each category with its share of that
 * currency's total (for its bar).
 */
export function byCurrency(totals = []) {
  const groups = new Map()
  for (const t of totals) {
    if (!groups.has(t.currency)) groups.set(t.currency, { currency: t.currency, cents: 0, rows: [] })
    const g = groups.get(t.currency)
    g.cents += t.cents
    g.rows.push({ category: t.category, cents: t.cents, count: t.count })
  }
  return [...groups.values()]
    .map((g) => ({ ...g, rows: g.rows.sort((a, b) => b.cents - a.cents).map((r) => ({ ...r, share: g.cents ? r.cents / g.cents : 0 })) }))
    .sort((a, b) => b.cents - a.cents)
}

/** "2026-11" moved by `delta` months. */
export function shiftMonth(month, delta) {
  const [y, m] = month.split('-').map(Number)
  const d = new Date(Date.UTC(y, m - 1 + delta, 1))
  return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, '0')}`
}

/** "November 2026". */
export function monthTitle(month, locale = undefined) {
  const [y, m] = month.split('-').map(Number)
  return new Date(Date.UTC(y, m - 1, 1)).toLocaleDateString(locale, { month: 'long', year: 'numeric', timeZone: 'UTC' })
}

/** The edit form for a waiting receipt (what Keep sends back). */
export function draftOf(r) {
  return { shop: r.shop, total: amountText(r.totalCents), currency: r.currency, date: r.spentOn, category: r.category, last4: r.last4 ?? '', note: r.note ?? '' }
}

/** Whether the member changed anything in the form (Keep then sends the edits). */
export function edited(r, draft) {
  const was = draftOf(r)
  return Object.keys(was).some((k) => String(draft[k] ?? '').trim() !== String(was[k] ?? '').trim())
}

/** The one line a drop answers with. */
export function dropLine(result, companion) {
  if (!result) return ''
  const parts = []
  if (result.sent) parts.push(`Sent ${result.sent} picture${result.sent === 1 ? '' : 's'} to ${companion}; each waits here once read.`)
  for (const line of result.already ?? []) parts.push(`${line}.`)
  for (const line of result.refused ?? []) parts.push(`${line}.`)
  if (result.left) parts.push(`Drop the other ${result.left} after these are read.`)
  return parts.join(' ') || 'Nothing to send.'
}
