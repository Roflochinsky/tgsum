import type { RawMessage, TextRun, ExtractedUnit } from './types.js'

const NOISE_ACTIONS = new Set([
  'pin_message', 'invite_members', 'remove_members', 'join_group_by_link',
  'join_group_by_request', 'edit_group_title', 'edit_group_photo', 'delete_group_photo',
  'phone_call', 'group_call', 'group_call_scheduled', 'invite_to_group_call',
  'take_screenshot', 'clear_history', 'set_messages_ttl', 'edit_chat_theme',
  'migrate_to_supergroup', 'migrate_from_group', 'create_group', 'create_channel',
])

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

// Keep normal messages; drop noise service messages. topic_created is handled
// by groupByTopic and is not emitted as a content message.
export function stripService(msgs: RawMessage[]): RawMessage[] {
  return msgs.filter(m => {
    if (m.type !== 'service') return true
    return false // all service messages are non-content; titles captured separately
  })
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

export function dateOf(m: RawMessage): string | undefined {
  return m.date
}

export type { ExtractedUnit }
