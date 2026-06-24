import type { RawMessage, TextRun } from './types.js'

function runsToText(runs: TextRun[]): string {
  return runs.map(r => (typeof r === 'string' ? r : r.text)).join('')
}

export function flattenText(m: RawMessage): string {
  if (Array.isArray(m.text_entities) && m.text_entities.length) return runsToText(m.text_entities)
  if (typeof m.text === 'string') return m.text
  if (Array.isArray(m.text)) return runsToText(m.text)
  return ''
}

export function resolveName(m: RawMessage): string {
  if (m.from != null && m.from !== '') return m.from
  if (m.from_id != null) return String(m.from_id)
  if (m.actor != null && m.actor !== '') return m.actor
  if (m.actor_id != null) return String(m.actor_id)
  return 'unknown'
}

// Keep normal messages; drop all service messages (non-content; topic titles are
// captured separately by groupByTopic).
export function stripService(msgs: RawMessage[]): RawMessage[] {
  return msgs.filter(m => m.type !== 'service')
}

interface Group { topicId: string; title: string; messages: RawMessage[] }

export function groupByTopic(msgs: RawMessage[]): Group[] {
  const byId = new Map<string, RawMessage>()
  const topicTitles = new Map<string, string>([['1', 'General']])
  for (const m of msgs) {
    byId.set(String(m.id), m)
    if (m.type === 'service' && m.action === 'topic_created') {
      topicTitles.set(String(m.id), m.title ?? 'Untitled')
    }
  }

  const resolveTopic = (m: RawMessage): string => {
    let cur: RawMessage | undefined = m
    const seen = new Set<string>()
    while (cur) {
      const cid = String(cur.id)
      if (topicTitles.has(cid) && cur.type === 'service') return cid // hit a topic root
      if (cur.reply_to_message_id == null) break
      const parentId = String(cur.reply_to_message_id)
      if (topicTitles.has(parentId)) return parentId // parent is a topic root
      if (seen.has(parentId)) break // cycle guard
      seen.add(parentId)
      cur = byId.get(parentId)
    }
    return '1' // General / unresolved
  }

  const groups = new Map<string, Group>()
  for (const m of msgs) {
    if (m.type === 'service') continue // titles captured; not content
    const topicId = resolveTopic(m)
    let g = groups.get(topicId)
    if (!g) { g = { topicId, title: topicTitles.get(topicId) ?? 'General', messages: [] }; groups.set(topicId, g) }
    g.messages.push(m)
  }
  return [...groups.values()]
}

export function isForum(msgs: RawMessage[]): boolean {
  return msgs.some(m => m.type === 'service' && m.action === 'topic_created')
}
