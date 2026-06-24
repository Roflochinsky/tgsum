# tgsum — Design Spec

**Status:** approved (design), pre-implementation
**Date:** 2026-06-25
**Owner:** Roflochinsky

## Problem

A non-technical product manager needs to research a pilot project from an **internal team Telegram chat** — where our colleagues (field reps, developers) discuss the pilot. The PM wants to answer: what did the customer dislike / like, who among the customer's decision-makers (ЛПР) is blocking us, how the people on site feel, what problems came up during the pilot.

A full Telegram account export is a huge JSON file (hundreds of chats, years of history, up to ~2 GB without media). It is unusable as-is: too big to read, too big to paste into an AI chat, full of noise.

`tgsum` is a **local extractor + formatter**: it lets the PM pick the relevant chats/topics from the export and produces clean, AI-ready `.md` files. The actual analysis (who-is-who, blockers, sentiment) is done by the PM in an AI chat (Claude/ChatGPT) using a separate library of prompt-skills (out of scope of this tool).

## Decisions (with the why)

- **No LLM, no network, no API key, no embeddings.** Summarization is the job of the AI chat, not this tool. Embeddings are a retrieval tool; summarization needs *coverage*, not *relevance*, so RAG would actively drop content and bias the result. → `tgsum` only extracts and formats.
- **Output is AI-ready Markdown files**, designed to be pasted into an AI chat that does the summary. Quality of that summary, not raw token-economy, drives the format (speakers, replies, dates, header context).
- **Form factor = guided TUI wizard + double-click launcher.** The target user "can barely launch it", so a bare CLI with flags is a wall. A 3-step wizard (pick file → check chats/topics → done) plus `.command`/`.bat` launchers removes the terminal barrier.
- **Cross-platform macOS + Windows is mandatory.**
- **Streaming parser is mandatory.** Exports reach multi-GB; `JSON.parse` on the whole file is impossible (Node max string ~512 MB + memory). Two-pass streaming: light index pass, then extract-selected pass.
- **Parse off `text_entities`, key identity off `from_id`.** `text` mixes strings and objects; `text_entities` is uniform. `from` (display name) can be `null` and is unstable; `from_id` is stable.
- **Topic = real forum topic.** No semantic clustering. Topic grouping reconstructed from the export (see Research + Risks).
- **Privacy: not a concern for this use case.** PM may paste customer data into cloud AI. No anonymization in v1. (Recorded as an explicit, revisitable decision.)
- **Voice/screenshots → markers only.** The export contains **no voice transcription field at all** (verified against tdesktop source), so markers (`[voice 0:42]`, `[photo]`) are the only possible behavior. Captions live in `text` and are preserved.
- **Role mapping is NOT in the CLI.** The summarized chat is our internal team chat — all participants are "ours". The people being discussed (ЛПР vs users) are named *inside* messages, not chat participants. Distinguishing them is interpretation → belongs to the **skill** (separate repo), which should identify who-is-who where derivable and emit an "open questions — clarify with colleagues" block for ambiguous identities.

## Scope (v1)

1. Read a Telegram Desktop `result.json` full-account export.
2. Guided TUI wizard:
   - Step ① choose the export file (path prompt, drag-and-drop friendly, validated).
   - Step ② searchable multi-select of chats and (where present) forum topics, with per-item metadata (message count, date range).
   - Step ③ confirmation + output folder report.
3. Two-pass streaming: pass 1 builds a lightweight index; pass 2 streams only the selected chats/topics.
4. Format selected content to AI-ready Markdown (see Output Format).
5. Write one `.md` file per selected chat/topic into an output folder; split oversized output into `…part-N.md` (≈100k-token budget per part, chars/4 heuristic), each part repeating the header.
6. **Distribution via npm**: `npm install -g tgsum`, then run `tgsum` (launches the wizard). Optional double-click launchers (`.command`/`.bat`) as a convenience bonus. (Trade-off: npm install needs Node + a one-time terminal command; acceptable as a one-off setup, possibly with a colleague's help, after which the PM only runs `tgsum`.)

## Non-goals (explicitly NOT in this tool)

- LLM calls, network, embeddings, semantic topic clustering.
- Prompt-skills (separate repository).
- Participant role mapping / anonymization.
- Voice transcription, OCR of screenshots, media content extraction.
- The actual summary (done by the AI chat + skill).

## Output Format

```
# Чат: <chat name> / Топик: <topic title>
# Период: 2026-06-18 — 2026-06-20 | сообщений: 142 | участники: Алиса, Боб, Вера

## 2026-06-20
[14:02] Алиса: когда катим релиз?
[14:03] Боб: давай в 15:00, сначала смёржь PR #210
[14:05] Вера ↳ Боб «смёржь PR #210»: смёржила
[14:06] Боб: [photo] логи деплоя
```

- Header: chat/topic, period, message count, participants (resolved names).
- `## <date>` day headers; `[HH:MM] Name: text`; replies as `↳ Name «quote start»`.
- Stripped: service messages, reactions, stickers. Media → `[photo]` / `[voice 0:42]` / `[file: name]`. Links, mentions, photo captions preserved.
- Names resolved from `from` (fallback to `from_id` when `from` is null).

## Acceptance criteria

- Given a real multi-GB `result.json`, pass 1 builds the chat/topic index without exhausting memory (no full-file `JSON.parse`).
- The wizard lets a non-technical user pick file → search/select chats/topics → get files, with no flags required.
- Forum topics are listed separately with titles; General topic shown as such.
- Selecting N chats/topics produces N independent `.md` files; oversized ones split into `part-N` with repeated headers.
- Output matches the format above on a hand-checked sample: correct speakers, replies, dates; service/reaction noise removed; media as markers; captions kept.
- Double-click launcher opens the wizard on macOS and Windows.
- No network calls occur (verifiable: tool runs fully offline).

## Affected layers / components

1. **Parser/stream** — streaming read of `result.json`; index pass + extract pass.
2. **Domain model** — chat, topic, message, participant; `text_entities` flattening; topic grouping via reply-chain walk-up.
3. **Formatter** — message → Markdown lines; header; part-splitting by token estimate.
4. **TUI wizard** — file prompt, searchable multi-select, confirmation.
5. **Output writer** — folder + per-chat/topic files + parts.
6. **Launchers / packaging** — `.command` / `.bat`; optional single-executable.

## Assumptions

- ASSUMPTION: input is a current Telegram Desktop full-account export (`chats.list[]`); old exports (bare-integer `from_id`, no `text_entities`) are tolerated but not primary.
- ASSUMPTION: the PM can produce the export themselves (Telegram Desktop → Export). This step is outside the tool.
- ASSUMPTION: ~100k tokens is a safe per-part budget for the target AI chats; configurable.
- ASSUMPTION: the relevant pilot information is text (per US); voice/screenshot loss is acceptable.

## Tech stack (verified via research; confirm once via Context7 before coding)

- **Runtime:** Node **≥22** (required by stream-json 3.x), TypeScript (ESM); `tsx` in dev; bundle with `tsup` (target node22); `bin` entry `tgsum`.
- **Streaming parser:** `stream-json@3.x` + `stream-chain` — **functional API** `parser()`/`pick()`/`streamArray()` composed via `chain([...])` (3.x is a breaking ESM-only rewrite of the old 1.x classes). Pick at `chats.list` → one chat assembled per element (ceiling: one chat in memory; upgrade path = message-level pick).
- **TUI:** `@clack/prompts` — built-in `autocompleteMultiselect` (filter-as-you-type + multi-select), which `@inquirer/checkbox` lacks. One wizard, no plugin.
- **Distribution:** npm global package (`bin: tgsum`); primary path `npm install -g tgsum`. Optional `.command`/`.bat` launchers as a bonus. Future no-Node option: `bun build --compile` (Node SEA cannot cross-compile → dropped).
- **Token estimate:** chars/**2.5** heuristic (Cyrillic-aware; chars/4 under-counts Russian ~2×) with a 90k soft cap under the 100k ceiling. No tokenizer dependency.

## Research

### Telegram Desktop `result.json` schema (verified against tdesktop source)

Sources: tdesktop `export_output_json.cpp`, `export_data_types.cpp`; `core.telegram.org/api/forum`, `/api/threads`. (SEO-blog claims of a "Machine JSON" toggle / "Include topic headers" checkbox are fabricated — do not exist.)

- **Top level:** root object; chats at **`chats.list[]`** (also `left_chats.list[]`). Each chat: `name` (string, nullable), `id` (int), `type`, `messages[]`.
- **`type` values:** `saved_messages`, `replies`, `verification_codes`, `personal_chat`, `bot_chat`, `private_group`, `private_supergroup`, `public_supergroup`, `private_channel`, `public_channel`.
- **Message:** `id`, `type` (`"message"`|`"service"`), `date` (ISO local, no TZ), `date_unixtime` (**string** epoch), `from` (nullable), `from_id` (prefixed string e.g. `"user653911985"`), `reply_to_message_id` (int), `text`, `text_entities`, `edited`, `reactions`, `forwarded_from`.
- **`text` dual nature:** either a plain string, or an array of (string | `{type, text, …}`). **Prefer `text_entities`** — always an array of uniform `{type, text}` (plain runs are `type:"plain"`). Flatten by concatenating `text` values in order, no separators.
  - Entity types: `plain, mention, hashtag, bot_command, link, url, email, bold, italic, code, pre, text_link, mention_name, phone, cashtag, underline, strikethrough, blockquote, bank_card, spoiler, custom_emoji`. Extra fields: `text_link→href`, `pre→language`, `mention_name→user_id`, `custom_emoji→document_id`.
- **Reply:** `reply_to_message_id` = id within same chat; build `id→message` index; target may be outside the exported slice (tolerate orphans). `reply_to_peer_id` for cross-peer.
- **Service messages** (`type:"service"`, `action`): noise to strip — `pin_message`, `invite_members`, `remove_members`, `join_group_by_link/request`, `edit_group_*`, `*_call*`, `take_screenshot`, `clear_history`, `set_messages_ttl`, `edit_chat_theme`, `migrate_*`. **Keep** `topic_created` / `topic_edit` (only source of topic titles). Service messages use `actor`/`actor_id` instead of `from`/`from_id`.
- **Forum topics (critical):** **no explicit per-message topic field.** Topic identity = the `id` of its `topic_created` service message (which carries `title`); **General topic = `id` 1** and its messages carry no topic reference. Grouping algorithm:
  1. Collect `topic_created` → `{id → title}`, plus implicit General (1).
  2. Build `id→message` index and `reply_to_message_id` parent map.
  3. For each message, walk `reply_to_message_id` up until hitting a known `topic_created` id (→ that topic) or no parent / id 1 (→ General).
- **Media:** fields on the message — `photo`/`file` (relative path), `file_name`, `file_size`, `media_type` (`sticker`, `video_message`, `voice_message`, `animation`, `video_file`, `audio_file`), `mime_type`, `duration_seconds`, `sticker_emoji`. **No `caption` field** — caption is the message `text`. **No transcription field anywhere** (verified).
- **`from_id`:** prefixed string (`user…`/`chat…`/`channel…`); stable identity. `from` nullable/unstable. Old exports: bare integer `from_id`, no `text_entities`/`date_unixtime` — support both.
- **Encoding/size:** UTF-8 (`\uXXXX`-escaped); single pretty-printed JSON doc (NOT NDJSON) → needs a real streaming parser; 64-bit ids → parse carefully to avoid JS 53-bit float precision loss.

### Risks (from research)

- **R1 — topic grouping not proven on live forum exports.** The reply-chain walk-up model is verified from the API, but it is **UNVERIFIED** that *every* in-topic message's chain terminates cleanly at the `topic_created` id in a real export (intermediate replies chain to immediate parent). **Mitigation:** validate against a real forum export early; provide a fallback (messages whose chain resolves to no known topic root → General) and surface a count of unresolved messages.
- **R2 — id precision.** 64-bit ids exceed JS safe integers → keep ids as strings / BigInt where used as keys.
- **R3 — old export format** (bare-int `from_id`, no `text_entities`) → parser must accept both shapes.
- **R4 — nested streaming ergonomics** of `stream-json` for `chats.list[].messages[]` → validate the exact Pick/SAX wiring in implementation; two-pass disk re-read is acceptable.

### Tech-stack research

The dedicated stack-research agent stalled (no output). Stack choices above are made from established knowledge; they are low-risk and reversible, and exact library APIs (nested streaming, searchable-multiselect) are validated against installed versions during execution (R4).

## Verification

Verification is performed against the Acceptance criteria above. Per-issue: independent reviewer sub-agent checks the diff vs spec/plan. Epic-level: run the wizard end-to-end on a real export and hand-check formatted output. Notes recorded here as each issue closes.
