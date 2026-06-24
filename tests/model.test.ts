import { describe, it, expect } from 'vitest'
import { flattenText, resolveName, stripService, groupByTopic } from '../src/model.js'
import type { RawMessage } from '../src/types.js'

describe('flattenText', () => {
  it('handles plain string', () => {
    expect(flattenText({ id: 1, type: 'message', text: 'hi' })).toBe('hi')
  })
  it('flattens entity array via text_entities', () => {
    const m: RawMessage = {
      id: 1, type: 'message',
      text: ['Check ', { type: 'bold', text: 'this' }],
      text_entities: [
        { type: 'plain', text: 'Check ' },
        { type: 'bold', text: 'this' },
        { type: 'link', text: 'http://x' },
      ],
    }
    expect(flattenText(m)).toBe('Check thishttp://x')
  })
  it('flattens array text when no text_entities (old export)', () => {
    expect(flattenText({ id: 1, type: 'message', text: ['a', { type: 'bold', text: 'b' }] })).toBe('ab')
  })
})

describe('resolveName', () => {
  it('uses from when present', () => {
    expect(resolveName({ id: 1, type: 'message', from: 'Alice', from_id: 'user1' })).toBe('Alice')
  })
  it('falls back to from_id when from is null', () => {
    expect(resolveName({ id: 1, type: 'message', from: null, from_id: 'user9' })).toBe('user9')
  })
})

describe('stripService', () => {
  it('drops noise service actions but keeps real messages', () => {
    const msgs: RawMessage[] = [
      { id: 1, type: 'service', action: 'pin_message' },
      { id: 2, type: 'message', text: 'real' },
    ]
    expect(stripService(msgs).map(m => m.id)).toEqual([2])
  })
})

describe('groupByTopic', () => {
  it('groups via reply walk-up to topic_created; General=1', () => {
    const msgs: RawMessage[] = [
      { id: 100, type: 'service', action: 'topic_created', title: 'Bugs' },
      { id: 101, type: 'message', text: 'in bugs', reply_to_message_id: 100 },
      { id: 102, type: 'message', text: 'reply in bugs', reply_to_message_id: 101 },
      { id: 5, type: 'message', text: 'general msg' },
    ]
    const groups = groupByTopic(msgs)
    const bugs = groups.find(g => g.topicId === '100')!
    expect(bugs.title).toBe('Bugs')
    expect(bugs.messages.map(m => m.id)).toEqual([101, 102])
    const general = groups.find(g => g.topicId === '1')!
    expect(general.messages.map(m => m.id)).toEqual([5])
  })
})
