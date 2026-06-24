import type { ExtractedUnit, Selection } from './types.js'
import { groupByTopic, stripService } from './model.js'
import { streamChats } from './stream-chats.js'

export async function extractSelection(path: string, selection: Selection[]): Promise<ExtractedUnit[]> {
  const wanted = new Map(selection.map(s => [s.chatId, s]))
  const units: ExtractedUnit[] = []
  for await (const chat of streamChats(path)) {
    const sel = wanted.get(String(chat.id))
    if (!sel) continue
    const name = chat.name ?? `(no name ${chat.id})`
    if (!sel.topicIds || !sel.topicIds.length) {
      units.push({ chatName: name, messages: stripService(chat.messages) })
    } else {
      const groups = groupByTopic(chat.messages)
      for (const topicId of sel.topicIds) {
        const g = groups.find(x => x.topicId === topicId)
        if (g) units.push({ chatName: name, topicTitle: g.title, messages: g.messages })
      }
    }
  }
  return units
}
