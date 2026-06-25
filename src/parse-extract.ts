import type { ExtractedUnit, Selection } from './types.js'
import { groupByTopic, stripService } from './model.js'
import { streamChats } from './stream-chats.js'

export async function extractSelection(path: string, selection: Selection[]): Promise<ExtractedUnit[]> {
  // Group by chat so multiple topics of the SAME chat (and a whole-chat pick)
  // all survive — a plain Map keyed by chatId would drop all but the last.
  const byChat = new Map<string, { whole: boolean; topicIds: Set<string> }>()
  for (const s of selection) {
    const e = byChat.get(s.chatId) ?? { whole: false, topicIds: new Set<string>() }
    if (!s.topicIds || !s.topicIds.length) e.whole = true
    else for (const t of s.topicIds) e.topicIds.add(t)
    byChat.set(s.chatId, e)
  }
  const units: ExtractedUnit[] = []
  for await (const chat of streamChats(path)) {
    const e = byChat.get(String(chat.id))
    if (!e) continue
    const name = chat.name ?? `(no name ${chat.id})`
    if (e.whole) {
      units.push({ chatName: name, messages: stripService(chat.messages) })
    }
    if (e.topicIds.size) {
      const groups = groupByTopic(chat.messages)
      for (const topicId of e.topicIds) {
        const g = groups.find(x => x.topicId === topicId)
        if (g) units.push({ chatName: name, topicTitle: g.title, messages: g.messages })
      }
    }
  }
  return units
}
