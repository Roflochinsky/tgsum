# tgsum — Design Spec

**Status:** v0.1 shipped as a Node/TypeScript TUI; **v0.2 rewritten as a Rust + Tauri desktop app** (see "Revision v0.2" below)
**Date:** 2026-06-25
**Owner:** Roflochinsky

## Problem

A non-technical product manager needs to research a pilot project from an **internal team Telegram chat** — where our colleagues (field reps, developers) discuss the pilot. The PM wants to answer: what did the customer dislike / like, who among the customer's decision-makers (ЛПР) is blocking us, how the people on site feel, what problems came up during the pilot.

A full Telegram account export is a huge JSON file (hundreds of chats, years of history, up to ~2 GB without media). It is unusable as-is: too big to read, too big to paste into an AI chat, full of noise.

`tgsum` is a **local extractor + formatter**: it lets the PM pick the relevant chats/topics from the export and produces clean, AI-ready `.md` files. The actual analysis (who-is-who, blockers, sentiment) is done by the PM in an AI chat (Claude/ChatGPT) using a separate library of prompt-skills (out of scope of this tool).

## Decisions (with the why)

- **No LLM, no network, no API key, no embeddings.** Summarization is the job of the AI chat, not this tool. Embeddings are a retrieval tool; summarization needs *coverage*, not *relevance*, so RAG would actively drop content and bias the result. → `tgsum` only extracts and formats.
- **Output is AI-ready Markdown files**, designed to be pasted into an AI chat that does the summary. Quality of that summary, not raw token-economy, drives the format (speakers, replies, dates, header context).
- **Form factor = desktop app with a guided 3-step flow** (v0.2; v0.1 was a TUI wizard + double-click launchers). The target user "can barely launch it", so a bare CLI with flags is a wall — and so is a terminal. An installable app (`.dmg` / `.exe` / `.AppImage`) with drag-and-drop of `result.json`, a searchable chat list and an "Open folder" button removes the terminal entirely.
- **Cross-platform macOS + Windows is mandatory.**
- **Streaming parser is mandatory.** Exports reach multi-GB; `JSON.parse` on the whole file is impossible (Node max string ~512 MB + memory). Two-pass streaming: light index pass, then extract-selected pass.
- **Parse off `text_entities`, key identity off `from_id`.** `text` mixes strings and objects; `text_entities` is uniform. `from` (display name) can be `null` and is unstable; `from_id` is stable.
- **Topic = real forum topic.** No semantic clustering. Topic grouping reconstructed from the export (see Research + Risks).
- **Privacy: not a concern for this use case.** PM may paste customer data into cloud AI. No anonymization in v1. (Recorded as an explicit, revisitable decision.)
- **Voice/screenshots → markers only.** The export contains **no voice transcription field at all** (verified against tdesktop source), so markers (`[voice 0:42]`, `[photo]`) are the only possible behavior. Captions live in `text` and are preserved.
- **Role mapping is NOT in the CLI.** The summarized chat is our internal team chat — all participants are "ours". The people being discussed (ЛПР vs users) are named *inside* messages, not chat participants. Distinguishing them is interpretation → belongs to the **skill** (separate repo), which should identify who-is-who where derivable and emit an "open questions — clarify with colleagues" block for ambiguous identities.

## Scope (v1)

1. Read a Telegram Desktop `result.json` full-account export.
2. Guided desktop flow (v0.1: TUI wizard):
   - Step ① choose the export file (drag-and-drop of the file or the export folder, or a native file dialog).
   - Step ② searchable multi-select of chats and (where present) forum topics, with per-item metadata (message count, date range).
   - Step ③ confirmation + output folder report.
3. Two-pass streaming: pass 1 builds a lightweight index; pass 2 streams only the selected chats/topics.
4. Format selected content to AI-ready Markdown (see Output Format).
5. Write one `.md` file per selected chat/topic into an output folder; split oversized output into `…part-N.md` (≈100k-token budget per part, chars/4 heuristic), each part repeating the header.
6. **Distribution (v0.2): installers from GitHub Releases** — `.dmg` (macOS, Apple Silicon + Intel), `.exe`/`.msi` (Windows), `.AppImage`/`.deb`/`.rpm` (Linux), built by CI on tag push. No Node, no terminal. For people with Rust, `cargo install tgsum` (crates.io, published by the same tag) is the `npm install -g` of v0.1; on Linux such a build adds its own launcher entry on first start.

## Revision v0.2 — Rust + Tauri desktop app

The v0.1 TypeScript CLI worked but kept the terminal barrier and needed Node ≥ 22. v0.2 is a rewrite:

- **`core/` (`tgsum-core`)** — pure Rust library: serde-visitor streaming over `chats.list` (one chat in memory at a time; the index pass keeps only per-message metadata), lenient field coercion (ids as numbers or strings, `text` as string or runs, old exports), topic grouping, formatter, writer. Output is byte-for-byte identical to v0.1 (verified on 174 randomized exports / 19k files), except where v0.1 cut an emoji in half inside a reply quote (it wrote U+FFFD; v0.2 drops the whole character).
- **`src-tauri/`** — the app: Tauri 2 commands `index_export` / `export_selection` run on a blocking thread, emit throttled `progress` events, support cancellation; native dialogs; "open folder" / "reveal file".
- **`src-tauri/ui/`** — static HTML/CSS/JS (no framework, no build step), served from the app bundle. The design is "Night in Van Gogh's manner", from the author's site: a generative painting (`paint.js`, ported from the site) writes a night sky of swirls, stars, a moon and a cypress stroke by stroke behind solid panels; facts about the app sit under three of its stars; a neon "tgsum" sign (Caveat outlines, lit letter by letter); Literata headings, Onest text, JetBrains Mono only for code and paths (OFL, bundled with Cyrillic); one solid yellow button per screen, secondary actions are underlined links. No backdrop-filter, blend modes or animated filters.
- **R2 (id precision) resolved:** ids are read as exact 64-bit integers (or strings), never floats.
- **Performance:** 200 MB export indexed in 0.8 s / 15 MB RSS (v0.1: 16.9 s / 546 MB); 1 GB in ~4 s.
- **Omarchy (Arch + Hyprland) support:** a native pacman package (`packaging/arch/PKGBUILD`, built in an Arch container by CI and attached to releases); on tiling Wayland compositors the window has no system title bar (the app header is the title bar); with `TGSUM_THEME=omarchy` the UI wears the active Omarchy theme instead of the painting (`colors.toml`, Omarchy 3 and 4 layouts) and follows theme switches live.
- **Small fixes over v0.1:** duplicate output names are compared case-insensitively (macOS/Windows file systems), a dropped export folder resolves to its `result.json`, selecting nothing is not a dead end, the index shows date ranges.

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
- Stripped: service messages, reactions. Media → markers `[photo]` / `[voice 0:42]` / `[file: name]` / `[sticker 👍]` (stickers kept as a marker — the emoji is a useful signal). Links, mentions, photo captions preserved.
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

## Tech stack

**v0.2 (current):**

- **Core:** Rust (`tgsum-core`), `serde` + `serde_json` streaming visitors over `chats.list` — no DOM of the whole file, one chat in memory at a time.
- **App:** Tauri 2 (system webview: WebView2 / WKWebView / WebKitGTK), plugins `dialog` (native pickers) and `opener` (open folder / reveal file). Installers via `cargo tauri build`.
- **UI:** static `src-tauri/ui/` (inside the app crate, so it ships to crates.io) — HTML, CSS, vanilla JS module; talks to Rust via `window.__TAURI__` (`withGlobalTauri`). No npm, no bundler.
- **Token estimate:** chars/**2.5** heuristic (UTF-16 length, Cyrillic-aware; chars/4 under-counts Russian ~2×) with a 90k default soft cap; the UI offers 30k / 60k / 90k / 150k / no split.

**v0.1 (historical):** Node ≥ 22, TypeScript, `stream-json@3` + `stream-chain`, `@clack/prompts` TUI, distributed via npm.

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
- **R2 — id precision.** 64-bit ids exceed JS safe integers → keep ids as strings / BigInt where used as keys. **v1 known limitation:** `stream-json`'s parser returns numeric `id` as a JS float, so ids ≥ 2^53 (16+ digits) are rounded. Practical impact is low (message ids are small; `from_id` is already a string; chat ids are typically ≤13 digits; corruption is deterministic and identical in both passes so selection still round-trips). Fix deferred — needs a custom number tokenizer/assembler option. `// ponytail: float ids; switch to numberAsString tokenizer if real ids ever exceed 2^53`.
- **R3 — old export format** (bare-int `from_id`, no `text_entities`) → parser must accept both shapes.
- **R4 — nested streaming ergonomics** of `stream-json` for `chats.list[].messages[]` → validate the exact Pick/SAX wiring in implementation; two-pass disk re-read is acceptable.

### Tech-stack research

The dedicated stack-research agent stalled (no output). Stack choices above are made from established knowledge; they are low-risk and reversible, and exact library APIs (nested streaming, searchable-multiselect) are validated against installed versions during execution (R4).

## Verification

Verification is performed against the Acceptance criteria above. Per-issue: independent reviewer sub-agent checks the diff vs spec/plan. Epic-level: run the wizard end-to-end on a real export and hand-check formatted output. Notes recorded here as each issue closes.
