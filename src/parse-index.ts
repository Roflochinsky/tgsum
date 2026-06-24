import type { ChatIndex, RawMessage, TopicIndex } from './types.js'
import { groupByTopic, isForum, stripService } from './model.js'
import { streamChats } from './stream-chats.js'

function dateRange(msgs: RawMessage[]): { firstDate?: string; lastDate?: string } {
  const dates = msgs.map(m => m.date).filter((d): d is string => !!d)
  if (!dates.length) return {}
  return { firstDate: dates[0], lastDate: dates[dates.length - 1] }
}

export async function streamIndex(path: string): Promise<ChatIndex[]> {
  const out: ChatIndex[] = []
  for await (const chat of streamChats(path)) {
    const content = stripService(chat.messages)
    const base = dateRange(content)
    let topics: TopicIndex[] = []
    if (isForum(chat.messages)) {
      topics = groupByTopic(chat.messages).map(g => ({
        topicId: g.topicId,
        title: g.title,
        count: g.messages.length,
        ...dateRange(g.messages),
      }))
    }
    out.push({
      chatId: String(chat.id),
      name: chat.name ?? `(no name ${chat.id})`,
      type: chat.type,
      count: content.length,
      ...base,
      topics,
    })
  }
  return out
}
