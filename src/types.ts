// A raw text run inside text/text_entities.
export type TextRun = string | { type: string; text: string; href?: string }

export interface RawMessage {
  id: number | string
  type: 'message' | 'service'
  date?: string
  date_unixtime?: string
  from?: string | null
  from_id?: string | number
  actor?: string | null
  actor_id?: string | number
  action?: string
  title?: string
  reply_to_message_id?: number | string
  text?: string | TextRun[]
  text_entities?: TextRun[]
  media_type?: string
  photo?: string
  file?: string
  file_name?: string
  duration_seconds?: number
  sticker_emoji?: string
}

export interface RawChat {
  name: string | null
  type: string
  id: number | string
  messages: RawMessage[]
}

export interface TopicIndex {
  topicId: string       // "1" for General, else topic_created message id
  title: string
  count: number
  firstDate?: string
  lastDate?: string
}

export interface ChatIndex {
  chatId: string
  name: string
  type: string
  count: number
  firstDate?: string
  lastDate?: string
  topics: TopicIndex[]  // empty when not a forum; General-only forums get one
}

// What the wizard passes to pass 2.
export interface Selection {
  chatId: string
  topicIds?: string[]   // undefined = whole chat; else specific forum topics
}

export interface ExtractedUnit {
  chatName: string
  topicTitle?: string
  messages: RawMessage[]
}
