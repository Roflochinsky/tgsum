import type { ExtractedUnit, RawMessage } from './types.js'
import { flattenText, resolveName } from './model.js'

// ponytail: chars/2.5 heuristic (Cyrillic-aware; chars/4 under-counts Russian ~2×), no tokenizer dep
const estTokens = (s: string) => Math.ceil(s.length / 2.5)

function safeName(s: string): string {
  return s.replace(/[\/\\:*?"<>|]/g, '_').trim() || 'chat'
}

function dayOf(date?: string): string {
  return date ? date.slice(0, 10) : 'unknown'
}
function timeOf(date?: string): string {
  return date ? date.slice(11, 16) : '--:--'
}

function mediaMarker(m: RawMessage): string | null {
  if (m.media_type === 'voice_message' || m.media_type === 'video_message' || m.media_type === 'audio_file') {
    const d = m.duration_seconds ?? 0
    const mm = Math.floor(d / 60), ss = String(d % 60).padStart(2, '0')
    const kind = m.media_type === 'voice_message' ? 'voice' : m.media_type === 'video_message' ? 'video' : 'audio'
    return `[${kind} ${mm}:${ss}]`
  }
  if (m.media_type === 'sticker') return `[sticker ${m.sticker_emoji ?? ''}]`.trim()
  if (m.media_type === 'animation' || m.media_type === 'video_file') return '[video]'
  if (m.photo) return '[photo]'
  if (m.file) return `[file: ${m.file_name ?? 'attachment'}]`
  return null
}

function lineFor(m: RawMessage, byId: Map<string, RawMessage>): string {
  const name = resolveName(m)
  const time = timeOf(m.date)
  let reply = ''
  if (m.reply_to_message_id != null) {
    const target = byId.get(String(m.reply_to_message_id))
    if (target) {
      const q = flattenText(target).slice(0, 40)
      reply = ` ↳ ${resolveName(target)} «${q}»`
    }
  }
  const text = flattenText(m)
  const marker = mediaMarker(m)
  const body = [marker, text].filter(Boolean).join(' ').trim()
  return `[${time}] ${name}${reply}: ${body}`
}

export function formatUnit(unit: ExtractedUnit, maxTokens = 90_000): { filename: string; parts: string[] } {
  const byId = new Map(unit.messages.map(m => [String(m.id), m]))
  const dates = unit.messages.map(m => m.date).filter((d): d is string => !!d)
  const participants = [...new Set(unit.messages.map(resolveName))].join(', ')
  const period = dates.length ? `${dayOf(dates[0])} — ${dayOf(dates[dates.length - 1])}` : 'n/a'
  const titleLine = unit.topicTitle
    ? `# Чат: ${unit.chatName} / Топик: ${unit.topicTitle}`
    : `# Чат: ${unit.chatName}`
  const header = `${titleLine}\n# Период: ${period} | сообщений: ${unit.messages.length} | участники: ${participants}\n`

  // Build body blocks per day; keep day header attached to its first line.
  const blocks: string[] = []
  let curDay = ''
  for (const m of unit.messages) {
    const day = dayOf(m.date)
    if (day !== curDay) { blocks.push(`\n## ${day}`); curDay = day }
    blocks.push(lineFor(m, byId))
  }

  // Pack blocks into parts under the token budget; header repeats per part.
  const headerCost = estTokens(header)
  const parts: string[] = []
  let cur: string[] = []
  let cost = headerCost
  for (const b of blocks) {
    const c = estTokens(b) + 1
    if (cur.length && cost + c > maxTokens) { parts.push(header + cur.join('\n')); cur = []; cost = headerCost }
    cur.push(b); cost += c
  }
  if (cur.length) parts.push(header + cur.join('\n'))

  const topicPart = unit.topicTitle ? `__${safeName(unit.topicTitle)}` : ''
  const filename = `${safeName(unit.chatName)}${topicPart}.md`
  return { filename, parts }
}
