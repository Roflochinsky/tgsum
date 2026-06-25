#!/usr/bin/env node

// src/wizard.ts
import { existsSync } from "fs";
import { resolve } from "path";
import { intro, outro, text, note, autocompleteMultiselect, confirm, isCancel, cancel, spinner } from "@clack/prompts";

// src/model.ts
function runsToText(runs) {
  return runs.map((r) => typeof r === "string" ? r : r.text).join("");
}
function flattenText(m) {
  if (Array.isArray(m.text_entities) && m.text_entities.length) return runsToText(m.text_entities);
  if (typeof m.text === "string") return m.text;
  if (Array.isArray(m.text)) return runsToText(m.text);
  return "";
}
function resolveName(m) {
  if (m.from != null && m.from !== "") return m.from;
  if (m.from_id != null) return String(m.from_id);
  if (m.actor != null && m.actor !== "") return m.actor;
  if (m.actor_id != null) return String(m.actor_id);
  return "unknown";
}
function stripService(msgs) {
  return msgs.filter((m) => m.type !== "service");
}
function groupByTopic(msgs) {
  const byId = /* @__PURE__ */ new Map();
  const topicTitles = /* @__PURE__ */ new Map([["1", "General"]]);
  for (const m of msgs) {
    byId.set(String(m.id), m);
    if (m.type === "service" && m.action === "topic_created") {
      topicTitles.set(String(m.id), m.title ?? "Untitled");
    }
  }
  const resolveTopic = (m) => {
    let cur = m;
    const seen = /* @__PURE__ */ new Set();
    while (cur) {
      const cid = String(cur.id);
      if (topicTitles.has(cid) && cur.type === "service") return cid;
      if (cur.reply_to_message_id == null) break;
      const parentId = String(cur.reply_to_message_id);
      if (topicTitles.has(parentId)) return parentId;
      if (seen.has(parentId)) break;
      seen.add(parentId);
      cur = byId.get(parentId);
    }
    return "1";
  };
  const groups = /* @__PURE__ */ new Map();
  for (const m of msgs) {
    if (m.type === "service") continue;
    const topicId = resolveTopic(m);
    let g = groups.get(topicId);
    if (!g) {
      g = { topicId, title: topicTitles.get(topicId) ?? "General", messages: [] };
      groups.set(topicId, g);
    }
    g.messages.push(m);
  }
  return [...groups.values()];
}
function isForum(msgs) {
  return msgs.some((m) => m.type === "service" && m.action === "topic_created");
}

// src/stream-chats.ts
import { createReadStream } from "fs";
import { parser } from "stream-json";
import { pick } from "stream-json/filters/pick.js";
import { streamArray } from "stream-json/streamers/stream-array.js";
import chain from "stream-chain";
async function* streamChats(path) {
  const pipeline = chain([
    createReadStream(path),
    parser(),
    pick({ filter: "chats.list" }),
    streamArray()
  ]);
  for await (const item of pipeline) {
    yield item.value;
  }
}

// src/parse-index.ts
function dateRange(msgs) {
  const dates = msgs.map((m) => m.date).filter((d) => !!d);
  if (!dates.length) return {};
  return { firstDate: dates[0], lastDate: dates[dates.length - 1] };
}
async function streamIndex(path) {
  const out = [];
  for await (const chat of streamChats(path)) {
    const content = stripService(chat.messages);
    const base = dateRange(content);
    let topics = [];
    if (isForum(chat.messages)) {
      topics = groupByTopic(chat.messages).map((g) => ({
        topicId: g.topicId,
        title: g.title,
        count: g.messages.length,
        ...dateRange(g.messages)
      }));
    }
    out.push({
      chatId: String(chat.id),
      name: chat.name ?? `(no name ${chat.id})`,
      type: chat.type,
      count: content.length,
      ...base,
      topics
    });
  }
  return out;
}

// src/parse-extract.ts
async function extractSelection(path, selection) {
  const wanted = new Map(selection.map((s) => [s.chatId, s]));
  const units = [];
  for await (const chat of streamChats(path)) {
    const sel = wanted.get(String(chat.id));
    if (!sel) continue;
    const name = chat.name ?? `(no name ${chat.id})`;
    if (!sel.topicIds || !sel.topicIds.length) {
      units.push({ chatName: name, messages: stripService(chat.messages) });
    } else {
      const groups = groupByTopic(chat.messages);
      for (const topicId of sel.topicIds) {
        const g = groups.find((x) => x.topicId === topicId);
        if (g) units.push({ chatName: name, topicTitle: g.title, messages: g.messages });
      }
    }
  }
  return units;
}

// src/write-output.ts
import { mkdirSync, writeFileSync } from "fs";
import { join } from "path";

// src/format.ts
var estTokens = (s) => Math.ceil(s.length / 2.5);
function safeName(s) {
  const cleaned = s.replace(/[\/\\:*?"<>|]/g, "_").trim();
  return /[\p{L}\p{N}]/u.test(cleaned) ? cleaned : "chat";
}
function dayOf(date) {
  return date ? date.slice(0, 10) : "unknown";
}
function timeOf(date) {
  return date ? date.slice(11, 16) : "--:--";
}
function mediaMarker(m) {
  if (m.media_type === "voice_message" || m.media_type === "video_message" || m.media_type === "audio_file") {
    const d = m.duration_seconds ?? 0;
    const mm = Math.floor(d / 60), ss = String(d % 60).padStart(2, "0");
    const kind = m.media_type === "voice_message" ? "voice" : m.media_type === "video_message" ? "video" : "audio";
    return `[${kind} ${mm}:${ss}]`;
  }
  if (m.media_type === "sticker") return `[sticker ${m.sticker_emoji ?? ""}]`.trim();
  if (m.media_type === "animation" || m.media_type === "video_file") return "[video]";
  if (m.photo) return "[photo]";
  if (m.file) return `[file: ${m.file_name ?? "attachment"}]`;
  return null;
}
function lineFor(m, byId) {
  const name = resolveName(m);
  const time = timeOf(m.date);
  let reply = "";
  if (m.reply_to_message_id != null) {
    const target = byId.get(String(m.reply_to_message_id));
    if (target) {
      const q = flattenText(target).slice(0, 40);
      reply = ` \u21B3 ${resolveName(target)} \xAB${q}\xBB`;
    }
  }
  const text2 = flattenText(m);
  const marker = mediaMarker(m);
  const body = [marker, text2].filter(Boolean).join(" ").trim();
  return `[${time}] ${name}${reply}: ${body}`;
}
function formatUnit(unit, maxTokens = 9e4) {
  const byId = new Map(unit.messages.map((m) => [String(m.id), m]));
  const dates = unit.messages.map((m) => m.date).filter((d) => !!d);
  const participants = [...new Set(unit.messages.map(resolveName))].join(", ");
  const period = dates.length ? `${dayOf(dates[0])} \u2014 ${dayOf(dates[dates.length - 1])}` : "n/a";
  const titleLine = unit.topicTitle ? `# \u0427\u0430\u0442: ${unit.chatName} / \u0422\u043E\u043F\u0438\u043A: ${unit.topicTitle}` : `# \u0427\u0430\u0442: ${unit.chatName}`;
  const header = `${titleLine}
# \u041F\u0435\u0440\u0438\u043E\u0434: ${period} | \u0441\u043E\u043E\u0431\u0449\u0435\u043D\u0438\u0439: ${unit.messages.length} | \u0443\u0447\u0430\u0441\u0442\u043D\u0438\u043A\u0438: ${participants}
`;
  const blocks = [];
  let curDay = "";
  for (const m of unit.messages) {
    const day = dayOf(m.date);
    if (day !== curDay) {
      blocks.push({ day, text: `
## ${day}`, isDayHeader: true });
      curDay = day;
    }
    blocks.push({ day, text: lineFor(m, byId), isDayHeader: false });
  }
  const headerCost = estTokens(header);
  const parts = [];
  let cur = [];
  let cost = headerCost;
  let partDay = "";
  for (const b of blocks) {
    const c = estTokens(b.text) + 1;
    if (cur.length && cost + c > maxTokens) {
      parts.push(header + cur.join("\n"));
      cur = [];
      cost = headerCost;
      partDay = "";
    }
    if (b.isDayHeader) {
      partDay = b.day;
    } else if (partDay !== b.day) {
      const dh = `
## ${b.day}`;
      cur.push(dh);
      cost += estTokens(dh) + 1;
      partDay = b.day;
    }
    cur.push(b.text);
    cost += c;
  }
  if (cur.length) parts.push(header + cur.join("\n"));
  const topicPart = unit.topicTitle ? `__${safeName(unit.topicTitle)}` : "";
  const filename = `${safeName(unit.chatName)}${topicPart}.md`;
  return { filename, parts };
}

// src/write-output.ts
function writeUnits(units, outDir, maxTokens = 9e4) {
  mkdirSync(outDir, { recursive: true });
  const written = [];
  const seen = /* @__PURE__ */ new Map();
  for (const unit of units) {
    const { filename, parts } = formatUnit(unit, maxTokens);
    const baseStem = filename.replace(/\.md$/, "");
    const n = (seen.get(baseStem) ?? 0) + 1;
    seen.set(baseStem, n);
    const stem = n === 1 ? baseStem : `${baseStem} (${n})`;
    if (parts.length === 1) {
      const p = join(outDir, `${stem}.md`);
      writeFileSync(p, parts[0], "utf8");
      written.push(p);
    } else {
      parts.forEach((content, i) => {
        const p = join(outDir, `${stem}.part-${i + 1}.md`);
        writeFileSync(p, content, "utf8");
        written.push(p);
      });
    }
  }
  return written;
}

// src/logo.ts
var BANNER = [
  "\u2588\u2588\u2588\u2588\u2588\u2588\u2588\u2588\u2557 \u2588\u2588\u2588\u2588\u2588\u2588\u2557 \u2588\u2588\u2588\u2588\u2588\u2588\u2588\u2557\u2588\u2588\u2557   \u2588\u2588\u2557\u2588\u2588\u2588\u2557   \u2588\u2588\u2588\u2557",
  "\u255A\u2550\u2550\u2588\u2588\u2554\u2550\u2550\u255D\u2588\u2588\u2554\u2550\u2550\u2550\u2550\u255D \u2588\u2588\u2554\u2550\u2550\u2550\u2550\u255D\u2588\u2588\u2551   \u2588\u2588\u2551\u2588\u2588\u2588\u2588\u2557 \u2588\u2588\u2588\u2588\u2551",
  "   \u2588\u2588\u2551   \u2588\u2588\u2551  \u2588\u2588\u2588\u2557\u2588\u2588\u2588\u2588\u2588\u2588\u2588\u2557\u2588\u2588\u2551   \u2588\u2588\u2551\u2588\u2588\u2554\u2588\u2588\u2588\u2588\u2554\u2588\u2588\u2551",
  "   \u2588\u2588\u2551   \u2588\u2588\u2551   \u2588\u2588\u2551\u255A\u2550\u2550\u2550\u2550\u2588\u2588\u2551\u2588\u2588\u2551   \u2588\u2588\u2551\u2588\u2588\u2551\u255A\u2588\u2588\u2554\u255D\u2588\u2588\u2551",
  "   \u2588\u2588\u2551   \u255A\u2588\u2588\u2588\u2588\u2588\u2588\u2554\u255D\u2588\u2588\u2588\u2588\u2588\u2588\u2588\u2551\u255A\u2588\u2588\u2588\u2588\u2588\u2588\u2554\u255D\u2588\u2588\u2551 \u255A\u2550\u255D \u2588\u2588\u2551",
  "   \u255A\u2550\u255D    \u255A\u2550\u2550\u2550\u2550\u2550\u255D \u255A\u2550\u2550\u2550\u2550\u2550\u2550\u255D \u255A\u2550\u2550\u2550\u2550\u2550\u255D \u255A\u2550\u255D     \u255A\u2550\u255D"
];
var lerp = (a, b, t) => Math.round(a + (b - a) * t);
function renderLogo(color = true) {
  if (!color) return BANNER.join("\n");
  const [tr, tg, tb] = [0, 255, 255];
  const [br, bg, bb] = [80, 120, 255];
  return BANNER.map((line, i) => {
    const t = BANNER.length > 1 ? i / (BANNER.length - 1) : 0;
    const r = lerp(tr, br, t), g = lerp(tg, bg, t), b = lerp(tb, bb, t);
    return `\x1B[1;38;2;${r};${g};${b}m${line}\x1B[0m`;
  }).join("\n");
}
function printLogo() {
  const color = Boolean(process.stdout.isTTY) && !process.env.NO_COLOR;
  process.stdout.write("\n" + renderLogo(color) + "\n\n");
}

// src/wizard.ts
var cleanPath = (s) => s.trim().replace(/^['"]|['"]$/g, "");
function toOptions(idx) {
  const options = [];
  const decode = /* @__PURE__ */ new Map();
  for (const c of idx) {
    if (c.topics.length) {
      for (const t of c.topics) {
        const key = `${c.chatId}::${t.topicId}`;
        options.push({ value: key, label: `${c.name} \u203A ${t.title}`, hint: `${t.count} msgs` });
        decode.set(key, { chatId: c.chatId, topicIds: [t.topicId] });
      }
    } else {
      options.push({ value: c.chatId, label: c.name, hint: `${c.count} msgs \xB7 ${c.type}` });
      decode.set(c.chatId, { chatId: c.chatId });
    }
  }
  return { options, decode };
}
function bail() {
  cancel("\u041E\u0442\u043C\u0435\u043D\u0435\u043D\u043E.");
  process.exit(0);
}
async function runWizard() {
  printLogo();
  intro("tgsum \u2014 \u0432\u044B\u0433\u0440\u0443\u0437\u043A\u0430 Telegram \u2192 \u0444\u0430\u0439\u043B\u044B \u0434\u043B\u044F \u0418\u0418");
  note(
    [
      "Telegram Desktop \u2192 \u2699 Settings \u2192 Advanced \u2192 Export Telegram data",
      "\u0424\u043E\u0440\u043C\u0430\u0442: Machine-readable JSON (\u043C\u0435\u0434\u0438\u0430 \u043C\u043E\u0436\u043D\u043E \u043D\u0435 \u0432\u043A\u043B\u044E\u0447\u0430\u0442\u044C) \u2192 Export.",
      "\u0412 \u043F\u0430\u043F\u043A\u0435 \u044D\u043A\u0441\u043F\u043E\u0440\u0442\u0430 \u043F\u043E\u044F\u0432\u0438\u0442\u0441\u044F \u0444\u0430\u0439\u043B result.json \u2014 \u0435\u0433\u043E \u043F\u0443\u0442\u044C \u0438 \u043D\u0443\u0436\u0435\u043D \u043D\u0438\u0436\u0435."
    ].join("\n"),
    "\u041A\u0430\u043A \u043F\u043E\u043B\u0443\u0447\u0438\u0442\u044C result.json"
  );
  const file = await text({
    message: "\u0428\u0430\u0433 1/3 \u2014 \u043F\u0435\u0440\u0435\u0442\u0430\u0449\u0438\u0442\u0435 result.json \u0441\u044E\u0434\u0430 \u0438\u043B\u0438 \u0432\u0441\u0442\u0430\u0432\u044C\u0442\u0435 \u043F\u0443\u0442\u044C:",
    validate: (v) => v && existsSync(cleanPath(v)) ? void 0 : "\u0424\u0430\u0439\u043B \u043D\u0435 \u043D\u0430\u0439\u0434\u0435\u043D"
  });
  if (isCancel(file)) bail();
  const path = cleanPath(file);
  const s = spinner();
  s.start("\u0427\u0438\u0442\u0430\u044E \u0441\u043F\u0438\u0441\u043E\u043A \u0447\u0430\u0442\u043E\u0432 (\u043D\u0430 \u0431\u043E\u043B\u044C\u0448\u043E\u0439 \u0432\u044B\u0433\u0440\u0443\u0437\u043A\u0435 \u044D\u0442\u043E \u0437\u0430\u043D\u0438\u043C\u0430\u0435\u0442 \u0432\u0440\u0435\u043C\u044F)");
  const idx = await streamIndex(path);
  s.stop(`\u041D\u0430\u0439\u0434\u0435\u043D\u043E: ${idx.reduce((n, c) => n + (c.topics.length || 1), 0)} \u0447\u0430\u0442\u043E\u0432/\u0442\u043E\u043F\u0438\u043A\u043E\u0432`);
  const { options, decode } = toOptions(idx);
  const picked = await autocompleteMultiselect({
    message: "\u0428\u0430\u0433 2/3 \u2014 \u043F\u0435\u0447\u0430\u0442\u0430\u0439\u0442\u0435 \u0434\u043B\u044F \u043F\u043E\u0438\u0441\u043A\u0430, \u043F\u0440\u043E\u0431\u0435\u043B \u2014 \u043E\u0442\u043C\u0435\u0442\u0438\u0442\u044C, Enter \u2014 \u043F\u043E\u0434\u0442\u0432\u0435\u0440\u0434\u0438\u0442\u044C:",
    options,
    placeholder: "\u041F\u043E\u0438\u0441\u043A \u043F\u043E \u043D\u0430\u0437\u0432\u0430\u043D\u0438\u044E\u2026"
  });
  if (isCancel(picked)) bail();
  const selection = picked.map((k) => decode.get(k)).filter(Boolean);
  if (!selection.length) {
    outro("\u041D\u0438\u0447\u0435\u0433\u043E \u043D\u0435 \u0432\u044B\u0431\u0440\u0430\u043D\u043E.");
    return;
  }
  const dir = await text({ message: "\u0428\u0430\u0433 3/3 \u2014 \u043F\u0430\u043F\u043A\u0430 \u0434\u043B\u044F \u0444\u0430\u0439\u043B\u043E\u0432:", placeholder: "tgsum-output", defaultValue: "tgsum-output" });
  if (isCancel(dir)) bail();
  const outDir = resolve(cleanPath(dir || "tgsum-output"));
  const ok = await confirm({ message: `\u0417\u0430\u043F\u0438\u0441\u0430\u0442\u044C ${selection.length} \u0432\u044B\u0431\u0440\u0430\u043D\u043D\u044B\u0445 \u0432 ${outDir}?` });
  if (isCancel(ok) || !ok) bail();
  const s2 = spinner();
  s2.start("\u0418\u0437\u0432\u043B\u0435\u043A\u0430\u044E \u0438 \u0444\u043E\u0440\u043C\u0430\u0442\u0438\u0440\u0443\u044E");
  const units = await extractSelection(path, selection);
  const written = writeUnits(units, outDir);
  s2.stop(`\u0413\u043E\u0442\u043E\u0432\u043E: ${written.length} \u0444\u0430\u0439\u043B(\u043E\u0432)`);
  outro(`\u0424\u0430\u0439\u043B\u044B \u0432 ${outDir}. \u0412\u0441\u0442\u0430\u0432\u044C\u0442\u0435 \u043D\u0443\u0436\u043D\u044B\u0439 \u0432 \u0447\u0430\u0442 \u0441 \u0418\u0418 \u0432\u043C\u0435\u0441\u0442\u0435 \u0441\u043E \u0441\u043A\u0438\u043B\u043B\u043E\u043C-\u043F\u0440\u043E\u043C\u043F\u0442\u043E\u043C.`);
}

// src/index.ts
runWizard().catch((err) => {
  console.error("  \u041E\u0448\u0438\u0431\u043A\u0430:", err?.message ?? err);
  process.exit(1);
});
