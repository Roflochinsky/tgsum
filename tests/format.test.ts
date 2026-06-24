import { describe, it, expect } from 'vitest'
import { formatUnit } from '../src/format.js'
import type { ExtractedUnit } from '../src/types.js'

const unit: ExtractedUnit = {
  chatName: 'Pilot Forum',
  topicTitle: 'Bugs',
  messages: [
    { id: 101, type: 'message', date: '2026-06-18T09:31:00', from: 'Bob', from_id: 'user2', text: 'found a bug' },
    { id: 102, type: 'message', date: '2026-06-18T09:32:00', from: 'Alice', from_id: 'user1', reply_to_message_id: 101, text: 'fixing it' },
    { id: 103, type: 'message', date: '2026-06-18T09:40:00', from: 'Alice', from_id: 'user1', media_type: 'voice_message', duration_seconds: 42 },
  ],
}

describe('formatUnit', () => {
  it('produces a header, day section, replies and media markers', () => {
    const { filename, parts } = formatUnit(unit, 100_000)
    expect(filename).toBe('Pilot Forum__Bugs.md')
    expect(parts).toHaveLength(1)
    const md = parts[0]
    expect(md).toContain('# Чат: Pilot Forum / Топик: Bugs')
    expect(md).toContain('сообщений: 3')
    expect(md).toContain('## 2026-06-18')
    expect(md).toContain('[09:31] Bob: found a bug')
    expect(md).toContain('[09:32] Alice ↳ Bob «found a bug»: fixing it')
    expect(md).toContain('[09:40] Alice: [voice 0:42]')
  })

  it('splits into parts by token budget, repeating the header', () => {
    const big: ExtractedUnit = {
      chatName: 'C', messages: Array.from({ length: 50 }, (_, i) => ({
        id: i, type: 'message' as const, date: '2026-06-18T09:00:00', from: 'X', from_id: 'user1',
        text: 'x'.repeat(40),
      })),
    }
    const { parts } = formatUnit(big, 60) // ~60 tokens budget => multiple parts
    expect(parts.length).toBeGreaterThan(1)
    for (const p of parts) expect(p).toContain('# Чат: C')
  })
})
