import { describe, it, expect } from 'vitest'
import { fileURLToPath } from 'node:url'
import { extractSelection } from '../src/parse-extract.js'

const fixture = fileURLToPath(new URL('./fixtures/sample-export.json', import.meta.url))

describe('extractSelection', () => {
  it('extracts a whole non-forum chat', async () => {
    const units = await extractSelection(fixture, [{ chatId: '111' }])
    expect(units).toHaveLength(1)
    expect(units[0].chatName).toBe('Direct with Bob')
    expect(units[0].topicTitle).toBeUndefined()
    expect(units[0].messages.map(m => m.id)).toEqual([1, 2]) // service excluded
  })

  it('extracts a single forum topic', async () => {
    const units = await extractSelection(fixture, [{ chatId: '222', topicIds: ['100'] }])
    expect(units).toHaveLength(1)
    expect(units[0].topicTitle).toBe('Bugs')
    expect(units[0].messages.map(m => m.id)).toEqual([101, 102])
  })
})
