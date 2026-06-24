import { describe, it, expect } from 'vitest'
import { fileURLToPath } from 'node:url'
import { streamIndex } from '../src/parse-index.js'

const fixture = fileURLToPath(new URL('./fixtures/sample-export.json', import.meta.url))

describe('streamIndex', () => {
  it('indexes chats with counts, dates, and forum topics', async () => {
    const idx = await streamIndex(fixture)
    expect(idx.map(c => c.name).sort()).toEqual(['Direct with Bob', 'Pilot Forum'])

    const direct = idx.find(c => c.chatId === '111')!
    expect(direct.count).toBe(2) // service pin excluded
    expect(direct.topics).toEqual([])
    expect(direct.firstDate).toBe('2026-06-18T10:00:00')
    expect(direct.lastDate).toBe('2026-06-19T11:00:00')

    const forum = idx.find(c => c.chatId === '222')!
    const titles = forum.topics.map(t => t.title).sort()
    expect(titles).toEqual(['Bugs', 'General'])
    const bugs = forum.topics.find(t => t.title === 'Bugs')!
    expect(bugs.count).toBe(2)         // 101 + 102
    expect(bugs.topicId).toBe('100')
  })
})
