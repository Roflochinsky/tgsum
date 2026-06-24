import { createReadStream } from 'node:fs'
import { parser } from 'stream-json'
import { pick } from 'stream-json/filters/pick.js'
import { streamArray } from 'stream-json/streamers/stream-array.js'
import chain from 'stream-chain'
import type { RawChat } from './types.js'

// stream-json 3.x functional API: chain([readStream, parser(), pick(...), streamArray()]).
// pick('chats.list') + streamArray() emits { key, value } per chat — one chat assembled at a time.
// ponytail: one chat in memory at a time; if a single chat ever exceeds RAM, switch to a
// message-level pick({ filter: (stack) => ... }) that streams each message individually.
export async function* streamChats(path: string): AsyncIterable<RawChat> {
  const pipeline = chain([
    createReadStream(path),
    parser(),
    pick({ filter: 'chats.list' }),
    streamArray(),
  ])
  for await (const item of pipeline as AsyncIterable<{ value: RawChat }>) {
    yield item.value
  }
}
