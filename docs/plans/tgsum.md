# tgsum Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A local cross-platform CLI that turns a Telegram Desktop `result.json` export into clean, AI-ready Markdown files for selected chats/forum-topics, via a guided wizard.

**Architecture:** Two-pass streaming over `chats.list` using `stream-json` (one chat held in memory at a time). Pass 1 builds a lightweight chat/topic index for the wizard; pass 2 re-streams and extracts only the selected chats/topics. A pure formatter turns messages into Markdown and splits oversized output into parts. A thin `@inquirer/prompts` wizard wires it together; output is `.md` files in a folder.

**Tech Stack:** Node 18+, TypeScript (ESM), `tsx` (dev), `tsup` (bundle), `vitest` (tests), `stream-json` (streaming parser), `@inquirer/prompts` (TUI).

**Spec:** `docs/specs/tgsum.md`. **Issues:** epic `nikitatrubaev-9nf` + children (`pd7, ggi, lsh, 99u, jjq, 6nh, 5l7, 2my`).

**Key design decisions baked in:**
- Parse off `text_entities` (uniform), fall back to `text`. Identity = `from_id` (fallback `from`).
- Topic = `id` of its `topic_created` service message; General topic = id `1`. Group messages by walking `reply_to_message_id` up to a topic root.
- Ids kept as **strings** to avoid JS 53-bit float precision loss.
- `streamArray` over `chats.list` holds **one chat at a time**. `// ponytail: one chat in memory at a time; if a single chat ever exceeds RAM, switch to nested message streaming`.
- Token budget for splitting = `chars / 4` heuristic.

---

## File Structure

```
package.json            # bin: tgsum, scripts, deps
tsconfig.json           # ESM, strict
tsup.config.ts          # bundle src/index.ts -> dist
vitest.config.ts
src/
  types.ts              # shared types (RawMessage, ChatIndex, TopicIndex, Selection)
  model.ts              # flattenText, resolveName, topic grouping, date helpers
  parse-index.ts        # pass 1: streamIndex(path) -> ChatIndex[]
  parse-extract.ts      # pass 2: extractSelection(path, selection) -> ExtractedUnit[]
  format.ts             # formatUnit(unit) -> {filename, parts: string[]}
  write-output.ts       # writeUnits(units, outDir) -> written paths
  wizard.ts             # runWizard(): orchestrate prompts + passes + writing
  index.ts             # #!/usr/bin/env node bin entry -> runWizard()
tests/
  model.test.ts
  parse-index.test.ts
  parse-extract.test.ts
  format.test.ts
  write-output.test.ts
  fixtures/sample-export.json
launchers/
  tgsum.command         # macOS double-click
  tgsum.bat             # Windows double-click
README.md
```

---

## Task 1: Project setup (issue `nikitatrubaev-pd7`)

**Files:**
- Create: `package.json`, `tsconfig.json`, `tsup.config.ts`, `vitest.config.ts`, `src/index.ts`

- [ ] **Step 1: Create `package.json`**

```json
{
  "name": "tgsum",
  "version": "0.1.0",
  "description": "Telegram export -> AI-ready Markdown CLI",
  "type": "module",
  "bin": { "tgsum": "dist/index.js" },
  "files": ["dist", "launchers", "README.md"],
  "engines": { "node": ">=18" },
  "scripts": {
    "dev": "tsx src/index.ts",
    "build": "tsup",
    "test": "vitest run",
    "typecheck": "tsc --noEmit"
  },
  "dependencies": {
    "stream-json": "^1.8.0",
    "@inquirer/prompts": "^7.0.0"
  },
  "devDependencies": {
    "@types/node": "^22.0.0",
    "@types/stream-json": "^1.7.7",
    "tsup": "^8.0.0",
    "tsx": "^4.0.0",
    "typescript": "^5.5.0",
    "vitest": "^2.0.0"
  }
}
```

- [ ] **Step 2: Create `tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "outDir": "dist",
    "types": ["node"]
  },
  "include": ["src", "tests"]
}
```

- [ ] **Step 3: Create `tsup.config.ts`**

```ts
import { defineConfig } from 'tsup'

export default defineConfig({
  entry: ['src/index.ts'],
  format: ['esm'],
  target: 'node18',
  clean: true,
  banner: { js: '#!/usr/bin/env node' },
})
```

- [ ] **Step 4: Create `vitest.config.ts`**

```ts
import { defineConfig } from 'vitest/config'

export default defineConfig({ test: { include: ['tests/**/*.test.ts'] } })
```

- [ ] **Step 5: Create `src/index.ts` (temporary stub)**

```ts
// Real wizard wired in Task 7.
console.log('tgsum — run the wizard (not yet wired). See docs/plans/tgsum.md')
```

- [ ] **Step 6: Install and verify build**

Run: `npm install && npm run typecheck && npm run build && node dist/index.js`
Expected: typecheck passes, `dist/index.js` created, running it prints the stub line.

- [ ] **Step 7: Commit**

```bash
git add package.json tsconfig.json tsup.config.ts vitest.config.ts src/index.ts package-lock.json
git commit -m "chore: project setup (ts, tsx, tsup, vitest, bin)"
```

---

## Task 2: Domain model (issue `nikitatrubaev-ggi`)

**Files:**
- Create: `src/types.ts`, `src/model.ts`, `tests/model.test.ts`

- [ ] **Step 1: Create `src/types.ts`**

```ts
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
```

- [ ] **Step 2: Write failing tests `tests/model.test.ts`**

```ts
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `npx vitest run tests/model.test.ts`
Expected: FAIL — `flattenText`/etc. not exported.

- [ ] **Step 4: Implement `src/model.ts`**

```ts
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/model.test.ts`
Expected: PASS (all 4 describe blocks green).

- [ ] **Step 6: Commit**

```bash
git add src/types.ts src/model.ts tests/model.test.ts
git commit -m "feat: domain model — text flatten, name resolve, topic grouping"
```

---

## Task 3: Pass 1 — streaming index (issue `nikitatrubaev-lsh`)

**Files:**
- Create: `src/parse-index.ts`, `tests/parse-index.test.ts`, `tests/fixtures/sample-export.json`

- [ ] **Step 1: Create fixture `tests/fixtures/sample-export.json`**

```json
{
  "about": "export",
  "chats": {
    "about": "chats",
    "list": [
      {
        "name": "Direct with Bob",
        "type": "personal_chat",
        "id": 111,
        "messages": [
          { "id": 1, "type": "message", "date": "2026-06-18T10:00:00", "from": "Alice", "from_id": "user1", "text": "hi bob" },
          { "id": 2, "type": "message", "date": "2026-06-19T11:00:00", "from": "Bob", "from_id": "user2", "reply_to_message_id": 1, "text": "hey alice" },
          { "id": 3, "type": "service", "date": "2026-06-19T11:01:00", "action": "pin_message", "message_id": 1 }
        ]
      },
      {
        "name": "Pilot Forum",
        "type": "private_supergroup",
        "id": 222,
        "messages": [
          { "id": 50, "type": "message", "date": "2026-06-18T09:00:00", "from": "Alice", "from_id": "user1", "text": "general hello" },
          { "id": 100, "type": "service", "date": "2026-06-18T09:30:00", "action": "topic_created", "title": "Bugs", "actor": "Alice", "actor_id": "user1" },
          { "id": 101, "type": "message", "date": "2026-06-18T09:31:00", "from": "Bob", "from_id": "user2", "reply_to_message_id": 100, "text": "found a bug" },
          { "id": 102, "type": "message", "date": "2026-06-20T09:31:00", "from": "Alice", "from_id": "user1", "reply_to_message_id": 101, "text": "fixing it", "media_type": "voice_message", "duration_seconds": 42, "file": "voice/x.ogg" }
        ]
      }
    ]
  }
}
```

- [ ] **Step 2: Write failing test `tests/parse-index.test.ts`**

```ts
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
```

- [ ] **Step 3: Run test to verify it fails**

Run: `npx vitest run tests/parse-index.test.ts`
Expected: FAIL — `streamIndex` not found.

- [ ] **Step 4: Implement `src/parse-index.ts`**

```ts
import { createReadStream } from 'node:fs'
import StreamJsonParser from 'stream-json'
import Pick from 'stream-json/filters/Pick.js'
import StreamArray from 'stream-json/streamers/StreamArray.js'
import type { ChatIndex, RawChat, RawMessage, TopicIndex } from './types.js'
import { groupByTopic, isForum, stripService } from './model.js'

// streamArray over chats.list -> one chat object at a time.
// ponytail: one chat in memory at a time; if a single chat ever exceeds RAM, switch to nested message streaming.
function streamChats(path: string): AsyncIterable<RawChat> {
  const pipeline = createReadStream(path)
    .pipe(StreamJsonParser.parser())
    .pipe(Pick.pick({ filter: 'chats.list' }))
    .pipe(StreamArray.streamArray())
  async function* gen() {
    for await (const item of pipeline as AsyncIterable<{ value: RawChat }>) {
      yield item.value
    }
  }
  return gen()
}

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
```

- [ ] **Step 5: Run test to verify it passes**

Run: `npx vitest run tests/parse-index.test.ts`
Expected: PASS. If the `stream-json` submodule import paths or `pick`/`streamArray` shapes differ from the installed version, fix the imports here (this is the R4 validation point) until the test is green — the public `streamIndex` signature must not change.

- [ ] **Step 6: Commit**

```bash
git add src/parse-index.ts tests/parse-index.test.ts tests/fixtures/sample-export.json
git commit -m "feat: pass 1 streaming chat/topic index"
```

---

## Task 4: Pass 2 — streaming extract (issue `nikitatrubaev-99u`)

**Files:**
- Create: `src/parse-extract.ts`, `tests/parse-extract.test.ts`

- [ ] **Step 1: Write failing test `tests/parse-extract.test.ts`**

```ts
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/parse-extract.test.ts`
Expected: FAIL — `extractSelection` not found.

- [ ] **Step 3: Implement `src/parse-extract.ts`**

```ts
import { createReadStream } from 'node:fs'
import StreamJsonParser from 'stream-json'
import Pick from 'stream-json/filters/Pick.js'
import StreamArray from 'stream-json/streamers/StreamArray.js'
import type { ExtractedUnit, RawChat, Selection } from './types.js'
import { groupByTopic, stripService } from './model.js'

function streamChats(path: string): AsyncIterable<RawChat> {
  const pipeline = createReadStream(path)
    .pipe(StreamJsonParser.parser())
    .pipe(Pick.pick({ filter: 'chats.list' }))
    .pipe(StreamArray.streamArray())
  async function* gen() {
    for await (const item of pipeline as AsyncIterable<{ value: RawChat }>) yield item.value
  }
  return gen()
}

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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/parse-extract.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/parse-extract.ts tests/parse-extract.test.ts
git commit -m "feat: pass 2 streaming extract of selected chats/topics"
```

---

## Task 5: Formatter + part-splitting (issue `nikitatrubaev-jjq`)

**Files:**
- Create: `src/format.ts`, `tests/format.test.ts`

- [ ] **Step 1: Write failing test `tests/format.test.ts`**

```ts
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/format.test.ts`
Expected: FAIL — `formatUnit` not found.

- [ ] **Step 3: Implement `src/format.ts`**

```ts
import type { ExtractedUnit, RawMessage } from './types.js'
import { flattenText, resolveName } from './model.js'

const estTokens = (s: string) => Math.ceil(s.length / 4) // ponytail: chars/4 heuristic, no tokenizer dep

function safeName(s: string): string {
  return s.replace(/[\/\\:*?"<>|]/g, '_').trim() || 'chat'
}

function dayOf(date?: string): string {
  return date ? date.slice(0, 10) : 'unknown'
}
function timeOf(date?: string): string {
  return date ? date.slice(11, 16) : '--:--'
}

function mediaMarker(m: RawMessage): string | null {
  if (m.media_type === 'voice_message' || m.media_type === 'video_message' || m.media_type === 'audio_file') {
    const d = m.duration_seconds ?? 0
    const mm = Math.floor(d / 60), ss = String(d % 60).padStart(2, '0')
    const kind = m.media_type === 'voice_message' ? 'voice' : m.media_type === 'video_message' ? 'video' : 'audio'
    return `[${kind} ${mm}:${ss}]`
  }
  if (m.media_type === 'sticker') return `[sticker ${m.sticker_emoji ?? ''}]`.trim()
  if (m.media_type === 'animation' || m.media_type === 'video_file') return '[video]'
  if (m.photo) return '[photo]'
  if (m.file) return `[file: ${m.file_name ?? 'attachment'}]`
  return null
}

function lineFor(m: RawMessage, byId: Map<string, RawMessage>): string {
  const name = resolveName(m)
  const time = timeOf(m.date)
  let reply = ''
  if (m.reply_to_message_id != null) {
    const target = byId.get(String(m.reply_to_message_id))
    if (target) {
      const q = flattenText(target).slice(0, 40)
      reply = ` ↳ ${resolveName(target)} «${q}»`
    }
  }
  const text = flattenText(m)
  const marker = mediaMarker(m)
  const body = [marker, text].filter(Boolean).join(' ').trim()
  return `[${time}] ${name}${reply}: ${body}`
}

export function formatUnit(unit: ExtractedUnit, maxTokens = 100_000): { filename: string; parts: string[] } {
  const byId = new Map(unit.messages.map(m => [String(m.id), m]))
  const dates = unit.messages.map(m => m.date).filter((d): d is string => !!d)
  const participants = [...new Set(unit.messages.map(resolveName))].join(', ')
  const period = dates.length ? `${dayOf(dates[0])} — ${dayOf(dates[dates.length - 1])}` : 'n/a'
  const titleLine = unit.topicTitle
    ? `# Чат: ${unit.chatName} / Топик: ${unit.topicTitle}`
    : `# Чат: ${unit.chatName}`
  const header = `${titleLine}\n# Период: ${period} | сообщений: ${unit.messages.length} | участники: ${participants}\n`

  // Build body blocks per day; keep day header attached to its first line.
  const blocks: string[] = []
  let curDay = ''
  for (const m of unit.messages) {
    const day = dayOf(m.date)
    if (day !== curDay) { blocks.push(`\n## ${day}`); curDay = day }
    blocks.push(lineFor(m, byId))
  }

  // Pack blocks into parts under the token budget; header repeats per part.
  const headerCost = estTokens(header)
  const parts: string[] = []
  let cur: string[] = []
  let cost = headerCost
  for (const b of blocks) {
    const c = estTokens(b) + 1
    if (cur.length && cost + c > maxTokens) { parts.push(header + cur.join('\n')); cur = []; cost = headerCost }
    cur.push(b); cost += c
  }
  if (cur.length) parts.push(header + cur.join('\n'))

  const topicPart = unit.topicTitle ? `__${safeName(unit.topicTitle)}` : ''
  const filename = `${safeName(unit.chatName)}${topicPart}.md`
  return { filename, parts }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/format.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/format.ts tests/format.test.ts
git commit -m "feat: markdown formatter with header, replies, media markers, part-splitting"
```

---

## Task 6: Output writer (issue `nikitatrubaev-6nh`)

**Files:**
- Create: `src/write-output.ts`, `tests/write-output.test.ts`

- [ ] **Step 1: Write failing test `tests/write-output.test.ts`**

```ts
import { describe, it, expect, afterEach } from 'vitest'
import { mkdtempSync, rmSync, readdirSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { writeUnits } from '../src/write-output.js'
import type { ExtractedUnit } from '../src/types.js'

let dir = ''
afterEach(() => { if (dir) rmSync(dir, { recursive: true, force: true }) })

describe('writeUnits', () => {
  it('writes one file per single-part unit, parts suffix for split units', () => {
    dir = mkdtempSync(join(tmpdir(), 'tgsum-'))
    const units: ExtractedUnit[] = [
      { chatName: 'A', messages: [{ id: 1, type: 'message', date: '2026-06-18T09:00:00', from: 'X', from_id: 'u1', text: 'hi' }] },
      { chatName: 'B', messages: Array.from({ length: 30 }, (_, i) => ({ id: i, type: 'message' as const, date: '2026-06-18T09:00:00', from: 'X', from_id: 'u1', text: 'y'.repeat(40) })) },
    ]
    const written = writeUnits(units, dir, 60)
    const files = readdirSync(dir).sort()
    expect(files).toContain('A.md')
    expect(files.some(f => f === 'B.part-1.md')).toBe(true)
    expect(written.length).toBe(files.length)
    expect(readFileSync(join(dir, 'A.md'), 'utf8')).toContain('# Чат: A')
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/write-output.test.ts`
Expected: FAIL — `writeUnits` not found.

- [ ] **Step 3: Implement `src/write-output.ts`**

```ts
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import type { ExtractedUnit } from './types.js'
import { formatUnit } from './format.js'

export function writeUnits(units: ExtractedUnit[], outDir: string, maxTokens = 100_000): string[] {
  mkdirSync(outDir, { recursive: true })
  const written: string[] = []
  for (const unit of units) {
    const { filename, parts } = formatUnit(unit, maxTokens)
    if (parts.length === 1) {
      const p = join(outDir, filename)
      writeFileSync(p, parts[0], 'utf8')
      written.push(p)
    } else {
      const stem = filename.replace(/\.md$/, '')
      parts.forEach((content, i) => {
        const p = join(outDir, `${stem}.part-${i + 1}.md`)
        writeFileSync(p, content, 'utf8')
        written.push(p)
      })
    }
  }
  return written
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/write-output.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/write-output.ts tests/write-output.test.ts
git commit -m "feat: output writer — folder, per-unit files, part suffixes"
```

---

## Task 7: TUI wizard (issue `nikitatrubaev-5l7`)

**Files:**
- Create: `src/wizard.ts`
- Modify: `src/index.ts`

Interactive prompts are verified manually (no unit test for the inquirer flow).

- [ ] **Step 1: Implement `src/wizard.ts`**

```ts
import { existsSync } from 'node:fs'
import { resolve } from 'node:path'
import { input, checkbox, confirm } from '@inquirer/prompts'
import { streamIndex } from './parse-index.js'
import { extractSelection } from './parse-extract.js'
import { writeUnits } from './write-output.js'
import type { ChatIndex, Selection } from './types.js'

// Drag-and-drop in terminals often wraps the path in quotes — strip them.
const cleanPath = (s: string) => s.trim().replace(/^['"]|['"]$/g, '')

// Build searchable choices: one entry per whole-chat, plus one per forum topic.
function toChoices(idx: ChatIndex[]) {
  const choices: { name: string; value: Selection }[] = []
  for (const c of idx) {
    if (c.topics.length) {
      for (const t of c.topics) {
        choices.push({
          name: `${c.name} › ${t.title} (${t.count} msgs)`,
          value: { chatId: c.chatId, topicIds: [t.topicId] },
        })
      }
    } else {
      choices.push({ name: `${c.name} (${c.count} msgs, ${c.type})`, value: { chatId: c.chatId } })
    }
  }
  return choices
}

export async function runWizard(): Promise<void> {
  console.log('\n  tgsum — выгрузка Telegram → файлы для ИИ\n')

  // Step 1: file
  let path = ''
  for (;;) {
    path = cleanPath(await input({ message: 'Шаг 1/3 — путь к result.json (можно перетащить файл):' }))
    if (path && existsSync(path)) break
    console.log('  ✗ файл не найден, попробуйте ещё раз')
  }

  console.log('  …читаю список чатов (это может занять время на большой выгрузке)')
  const idx = await streamIndex(path)
  const choices = toChoices(idx)

  // Step 2: searchable multi-select. @inquirer checkbox supports type-to-filter via `source`-less
  // filtering is limited, so we expose a filter prompt then a checkbox of matches when the list is large.
  let pool = choices
  if (choices.length > 30) {
    const term = (await input({ message: `Шаг 2/3 — найдено ${choices.length} чатов/топиков. Фильтр по названию (Enter — показать все):` })).toLowerCase().trim()
    if (term) pool = choices.filter(c => c.name.toLowerCase().includes(term))
  }
  const selection = await checkbox<Selection>({
    message: 'Отметьте чаты/топики (пробел — выбрать, Enter — подтвердить):',
    choices: pool,
    pageSize: 20,
    loop: false,
  })
  if (!selection.length) { console.log('  Ничего не выбрано. Выход.'); return }

  // Step 3: output dir + confirm
  const outDir = resolve(cleanPath(await input({ message: 'Шаг 3/3 — папка для файлов:', default: 'tgsum-output' })))
  const ok = await confirm({ message: `Записать ${selection.length} выбранных в ${outDir}?`, default: true })
  if (!ok) { console.log('  Отменено.'); return }

  console.log('  …извлекаю и форматирую')
  const units = await extractSelection(path, selection)
  const written = writeUnits(units, outDir)
  console.log(`\n  ✓ Готово: ${written.length} файл(ов) в ${outDir}`)
  for (const p of written) console.log(`    - ${p}`)
  console.log('\n  Вставьте нужный файл в чат с ИИ вместе со скиллом-промптом.\n')
}
```

- [ ] **Step 2: Wire `src/index.ts`**

```ts
import { runWizard } from './wizard.js'

runWizard().catch((err) => {
  if (err?.name === 'ExitPromptError') { console.log('\n  Прервано.'); process.exit(0) }
  console.error('  Ошибка:', err?.message ?? err)
  process.exit(1)
})
```

- [ ] **Step 3: Typecheck + build**

Run: `npm run typecheck && npm run build`
Expected: PASS, `dist/index.js` rebuilt.

- [ ] **Step 4: Manual end-to-end verify against the fixture**

Run: `npm run dev` then enter `tests/fixtures/sample-export.json`, select "Pilot Forum › Bugs", output dir `tgsum-output`, confirm.
Expected: prints "✓ Готово", `tgsum-output/Pilot Forum__Bugs.md` exists and contains the formatted Bugs topic. Then: `rm -rf tgsum-output`.

- [ ] **Step 5: Commit**

```bash
git add src/wizard.ts src/index.ts
git commit -m "feat: guided TUI wizard wiring passes + writer"
```

---

## Task 8: npm distribution + launchers (issue `nikitatrubaev-2my`)

**Files:**
- Create: `launchers/tgsum.command`, `launchers/tgsum.bat`, `README.md`

- [ ] **Step 1: Create `launchers/tgsum.command` (macOS)**

```bash
#!/bin/bash
# Double-click launcher. If quarantined by Gatekeeper, run once:
#   xattr -d com.apple.quarantine tgsum.command
cd "$(dirname "$0")" || exit 1
if command -v tgsum >/dev/null 2>&1; then tgsum; else npx tgsum; fi
echo ""; read -r -p "Нажмите Enter чтобы закрыть…" _
```

- [ ] **Step 2: Create `launchers/tgsum.bat` (Windows)**

```bat
@echo off
cd /d "%~dp0"
where tgsum >nul 2>nul
if %errorlevel%==0 ( tgsum ) else ( npx tgsum )
echo.
pause
```

- [ ] **Step 3: Create `README.md`**

````markdown
# tgsum

Локальная утилита: превращает выгрузку Telegram Desktop (`result.json`) в чистые `.md` файлы для вставки в чат с ИИ. Не ходит в сеть, не зовёт LLM.

## Установка

```bash
npm install -g tgsum
```

## Запуск

```bash
tgsum
```

Дальше — пошаговый мастер: путь к `result.json` → выбор чатов/топиков → папка с файлами.

Не хотите терминал — используйте двойной-клик: `launchers/tgsum.command` (mac) или `launchers/tgsum.bat` (win).

## Как получить result.json

Telegram Desktop → Settings → Advanced → Export Telegram data → формат **JSON**, медиа можно не включать.
````

- [ ] **Step 4: Make launcher executable + verify packaging**

Run:
```bash
chmod +x launchers/tgsum.command
npm run build && npm pack
```
Expected: `npm pack` produces `tgsum-0.1.0.tgz` containing `dist/`, `launchers/`, `README.md` (check with `tar tzf tgsum-0.1.0.tgz`). Then remove the tarball: `rm tgsum-0.1.0.tgz`.

- [ ] **Step 5: Commit**

```bash
git add launchers/tgsum.command launchers/tgsum.bat README.md
git commit -m "feat: npm distribution metadata + double-click launchers + readme"
```

---

## Self-Review

**Spec coverage:**
- Read `result.json` streaming → Tasks 3, 4 (`stream-json` `Pick`+`streamArray`). ✓
- Guided 3-step wizard (file → searchable multi-select → output) → Task 7. ✓
- Two-pass (index then extract) → Tasks 3, 4. ✓
- AI-ready Markdown format (header, dates, replies, media markers, stripped noise) → Tasks 2 (strip/flatten), 5 (format). ✓
- One file per chat/topic + part-splitting → Tasks 5, 6. ✓
- Forum topics via reply walk-up, General=1 → Task 2 (`groupByTopic`), tested. ✓
- `text_entities` parse, `from_id` identity, old-export tolerance → Task 2. ✓
- npm distribution + optional launchers → Task 8. ✓
- No network/LLM/embeddings → nothing in the plan adds them. ✓
- Risk R4 (stream-json API) → explicit validation note in Task 3 Step 5, public signature fixed. ✓
- Risk R1 (topic walk-up on real exports) → `groupByTopic` returns unresolved messages to General; validate on a real forum export during epic verification. ✓

**Placeholder scan:** No TBD/TODO; every code step has full code. The `src/index.ts` stub in Task 1 is replaced in Task 7 (noted). ✓

**Type consistency:** `Selection`, `ChatIndex`, `TopicIndex`, `ExtractedUnit`, `RawMessage`, `RawChat` defined in Task 2 `src/types.ts` and used unchanged in Tasks 3–7. `formatUnit(unit, maxTokens)`, `writeUnits(units, dir, maxTokens)`, `streamIndex(path)`, `extractSelection(path, selection)`, `groupByTopic(msgs)`, `flattenText(m)`, `resolveName(m)`, `stripService(msgs)` — signatures consistent across tasks. ✓

**Note for executor:** Tasks 3 & 4 both define a private `streamChats` helper. If the `stream-json` wiring needs adjustment (R4), keep both in sync (or extract to a shared `src/stream-chats.ts` — acceptable small refactor).
