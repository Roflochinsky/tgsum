#!/usr/bin/env node
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __commonJS = (cb, mod) => function __require() {
  return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));

// node_modules/sisteransi/src/index.js
var require_src = __commonJS({
  "node_modules/sisteransi/src/index.js"(exports, module) {
    "use strict";
    var ESC2 = "\x1B";
    var CSI2 = `${ESC2}[`;
    var beep = "\x07";
    var cursor2 = {
      to(x, y) {
        if (!y) return `${CSI2}${x + 1}G`;
        return `${CSI2}${y + 1};${x + 1}H`;
      },
      move(x, y) {
        let ret = "";
        if (x < 0) ret += `${CSI2}${-x}D`;
        else if (x > 0) ret += `${CSI2}${x}C`;
        if (y < 0) ret += `${CSI2}${-y}A`;
        else if (y > 0) ret += `${CSI2}${y}B`;
        return ret;
      },
      up: (count = 1) => `${CSI2}${count}A`,
      down: (count = 1) => `${CSI2}${count}B`,
      forward: (count = 1) => `${CSI2}${count}C`,
      backward: (count = 1) => `${CSI2}${count}D`,
      nextLine: (count = 1) => `${CSI2}E`.repeat(count),
      prevLine: (count = 1) => `${CSI2}F`.repeat(count),
      left: `${CSI2}G`,
      hide: `${CSI2}?25l`,
      show: `${CSI2}?25h`,
      save: `${ESC2}7`,
      restore: `${ESC2}8`
    };
    var scroll = {
      up: (count = 1) => `${CSI2}S`.repeat(count),
      down: (count = 1) => `${CSI2}T`.repeat(count)
    };
    var erase2 = {
      screen: `${CSI2}2J`,
      up: (count = 1) => `${CSI2}1J`.repeat(count),
      down: (count = 1) => `${CSI2}J`.repeat(count),
      line: `${CSI2}2K`,
      lineEnd: `${CSI2}K`,
      lineStart: `${CSI2}1K`,
      lines(count) {
        let clear = "";
        for (let i = 0; i < count; i++)
          clear += this.line + (i < count - 1 ? cursor2.up() : "");
        if (count)
          clear += cursor2.left;
        return clear;
      }
    };
    module.exports = { cursor: cursor2, scroll, erase: erase2, beep };
  }
});

// src/wizard.ts
import { existsSync } from "fs";
import { resolve } from "path";
import { intro, outro, text, note, confirm, isCancel, cancel, spinner } from "@clack/prompts";

// src/select-prompt.ts
import { styleText as styleText2 } from "util";

// node_modules/@clack/core/dist/index.mjs
import { styleText } from "util";
import { stdout, stdin } from "process";
import * as l from "readline";
import l__default from "readline";

// node_modules/fast-string-truncated-width/dist/utils.js
var getCodePointsLength = /* @__PURE__ */ (() => {
  const SURROGATE_PAIR_RE = /[\uD800-\uDBFF][\uDC00-\uDFFF]/g;
  return (input) => {
    let surrogatePairsNr = 0;
    SURROGATE_PAIR_RE.lastIndex = 0;
    while (SURROGATE_PAIR_RE.test(input)) {
      surrogatePairsNr += 1;
    }
    return input.length - surrogatePairsNr;
  };
})();
var isFullWidth = (x) => {
  return x === 12288 || x >= 65281 && x <= 65376 || x >= 65504 && x <= 65510;
};
var isWideNotCJKTNotEmoji = (x) => {
  return x === 8987 || x === 9001 || x >= 12272 && x <= 12287 || x >= 12289 && x <= 12350 || x >= 12441 && x <= 12543 || x >= 12549 && x <= 12591 || x >= 12593 && x <= 12686 || x >= 12688 && x <= 12771 || x >= 12783 && x <= 12830 || x >= 12832 && x <= 12871 || x >= 12880 && x <= 19903 || x >= 65040 && x <= 65049 || x >= 65072 && x <= 65106 || x >= 65108 && x <= 65126 || x >= 65128 && x <= 65131 || x >= 127488 && x <= 127490 || x >= 127504 && x <= 127547 || x >= 127552 && x <= 127560 || x >= 131072 && x <= 196605 || x >= 196608 && x <= 262141;
};

// node_modules/fast-string-truncated-width/dist/index.js
var ANSI_RE = /[\u001b\u009b][[()#;?]*(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-ORZcf-nqry=><]|\u001b\]8;[^;]*;.*?(?:\u0007|\u001b\u005c)/y;
var CONTROL_RE = /[\x00-\x08\x0A-\x1F\x7F-\x9F]{1,1000}/y;
var CJKT_WIDE_RE = /(?:(?![\uFF61-\uFF9F\uFF00-\uFFEF])[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}\p{Script=Tangut}]){1,1000}/yu;
var TAB_RE = /\t{1,1000}/y;
var EMOJI_RE = new RegExp("[\\u{1F1E6}-\\u{1F1FF}]{2}|\\u{1F3F4}[\\u{E0061}-\\u{E007A}]{2}[\\u{E0030}-\\u{E0039}\\u{E0061}-\\u{E007A}]{1,3}\\u{E007F}|(?:\\p{Emoji}\\uFE0F\\u20E3?|\\p{Emoji_Modifier_Base}\\p{Emoji_Modifier}?|\\p{Emoji_Presentation})(?:\\u200D(?:\\p{Emoji_Modifier_Base}\\p{Emoji_Modifier}?|\\p{Emoji_Presentation}|\\p{Emoji}\\uFE0F\\u20E3?))*", "yu");
var LATIN_RE = /(?:[\x20-\x7E\xA0-\xFF](?!\uFE0F)){1,1000}/y;
var MODIFIER_RE = new RegExp("\\p{M}+", "gu");
var NO_TRUNCATION = { limit: Infinity, ellipsis: "" };
var getStringTruncatedWidth = (input, truncationOptions = {}, widthOptions = {}) => {
  const LIMIT = truncationOptions.limit ?? Infinity;
  const ELLIPSIS = truncationOptions.ellipsis ?? "";
  const ELLIPSIS_WIDTH = truncationOptions?.ellipsisWidth ?? (ELLIPSIS ? getStringTruncatedWidth(ELLIPSIS, NO_TRUNCATION, widthOptions).width : 0);
  const ANSI_WIDTH = 0;
  const CONTROL_WIDTH = widthOptions.controlWidth ?? 0;
  const TAB_WIDTH = widthOptions.tabWidth ?? 8;
  const EMOJI_WIDTH = widthOptions.emojiWidth ?? 2;
  const FULL_WIDTH_WIDTH = 2;
  const REGULAR_WIDTH = widthOptions.regularWidth ?? 1;
  const WIDE_WIDTH = widthOptions.wideWidth ?? FULL_WIDTH_WIDTH;
  const PARSE_BLOCKS = [
    [LATIN_RE, REGULAR_WIDTH],
    [ANSI_RE, ANSI_WIDTH],
    [CONTROL_RE, CONTROL_WIDTH],
    [TAB_RE, TAB_WIDTH],
    [EMOJI_RE, EMOJI_WIDTH],
    [CJKT_WIDE_RE, WIDE_WIDTH]
  ];
  let indexPrev = 0;
  let index = 0;
  let length = input.length;
  let lengthExtra = 0;
  let truncationEnabled = false;
  let truncationIndex = length;
  let truncationLimit = Math.max(0, LIMIT - ELLIPSIS_WIDTH);
  let unmatchedStart = 0;
  let unmatchedEnd = 0;
  let width = 0;
  let widthExtra = 0;
  outer: while (true) {
    if (unmatchedEnd > unmatchedStart || index >= length && index > indexPrev) {
      const unmatched = input.slice(unmatchedStart, unmatchedEnd) || input.slice(indexPrev, index);
      lengthExtra = 0;
      for (const char of unmatched.replaceAll(MODIFIER_RE, "")) {
        const codePoint = char.codePointAt(0) || 0;
        if (isFullWidth(codePoint)) {
          widthExtra = FULL_WIDTH_WIDTH;
        } else if (isWideNotCJKTNotEmoji(codePoint)) {
          widthExtra = WIDE_WIDTH;
        } else {
          widthExtra = REGULAR_WIDTH;
        }
        if (width + widthExtra > truncationLimit) {
          truncationIndex = Math.min(truncationIndex, Math.max(unmatchedStart, indexPrev) + lengthExtra);
        }
        if (width + widthExtra > LIMIT) {
          truncationEnabled = true;
          break outer;
        }
        lengthExtra += char.length;
        width += widthExtra;
      }
      unmatchedStart = unmatchedEnd = 0;
    }
    if (index >= length) {
      break outer;
    }
    for (let i = 0, l2 = PARSE_BLOCKS.length; i < l2; i++) {
      const [BLOCK_RE, BLOCK_WIDTH] = PARSE_BLOCKS[i];
      BLOCK_RE.lastIndex = index;
      if (BLOCK_RE.test(input)) {
        lengthExtra = BLOCK_RE === CJKT_WIDE_RE ? getCodePointsLength(input.slice(index, BLOCK_RE.lastIndex)) : BLOCK_RE === EMOJI_RE ? 1 : BLOCK_RE.lastIndex - index;
        widthExtra = lengthExtra * BLOCK_WIDTH;
        if (width + widthExtra > truncationLimit) {
          truncationIndex = Math.min(truncationIndex, index + Math.floor((truncationLimit - width) / BLOCK_WIDTH));
        }
        if (width + widthExtra > LIMIT) {
          truncationEnabled = true;
          break outer;
        }
        width += widthExtra;
        unmatchedStart = indexPrev;
        unmatchedEnd = index;
        index = indexPrev = BLOCK_RE.lastIndex;
        continue outer;
      }
    }
    index += 1;
  }
  return {
    width: truncationEnabled ? truncationLimit : width,
    index: truncationEnabled ? truncationIndex : length,
    truncated: truncationEnabled,
    ellipsed: truncationEnabled && LIMIT >= ELLIPSIS_WIDTH
  };
};
var dist_default = getStringTruncatedWidth;

// node_modules/fast-string-width/dist/index.js
var NO_TRUNCATION2 = {
  limit: Infinity,
  ellipsis: "",
  ellipsisWidth: 0
};
var fastStringWidth = (input, options = {}) => {
  return dist_default(input, NO_TRUNCATION2, options).width;
};
var dist_default2 = fastStringWidth;

// node_modules/fast-wrap-ansi/lib/main.js
var ESC = "\x1B";
var CSI = "\x9B";
var END_CODE = 39;
var ANSI_ESCAPE_BELL = "\x07";
var ANSI_CSI = "[";
var ANSI_OSC = "]";
var ANSI_SGR_TERMINATOR = "m";
var ANSI_ESCAPE_LINK = `${ANSI_OSC}8;;`;
var GROUP_REGEX = new RegExp(`(?:\\${ANSI_CSI}(?<code>\\d+)m|\\${ANSI_ESCAPE_LINK}(?<uri>.*)${ANSI_ESCAPE_BELL})`, "y");
var getClosingCode = (openingCode) => {
  if (openingCode >= 30 && openingCode <= 37)
    return 39;
  if (openingCode >= 90 && openingCode <= 97)
    return 39;
  if (openingCode >= 40 && openingCode <= 47)
    return 49;
  if (openingCode >= 100 && openingCode <= 107)
    return 49;
  if (openingCode === 1 || openingCode === 2)
    return 22;
  if (openingCode === 3)
    return 23;
  if (openingCode === 4)
    return 24;
  if (openingCode === 7)
    return 27;
  if (openingCode === 8)
    return 28;
  if (openingCode === 9)
    return 29;
  if (openingCode === 0)
    return 0;
  return void 0;
};
var wrapAnsiCode = (code) => `${ESC}${ANSI_CSI}${code}${ANSI_SGR_TERMINATOR}`;
var wrapAnsiHyperlink = (url) => `${ESC}${ANSI_ESCAPE_LINK}${url}${ANSI_ESCAPE_BELL}`;
var wrapWord = (rows, word, columns) => {
  const characters = word[Symbol.iterator]();
  let isInsideEscape = false;
  let isInsideLinkEscape = false;
  let lastRow = rows.at(-1);
  let visible = lastRow === void 0 ? 0 : dist_default2(lastRow);
  let currentCharacter = characters.next();
  let nextCharacter = characters.next();
  let rawCharacterIndex = 0;
  while (!currentCharacter.done) {
    const character = currentCharacter.value;
    const characterLength = dist_default2(character);
    if (visible + characterLength <= columns) {
      rows[rows.length - 1] += character;
    } else {
      rows.push(character);
      visible = 0;
    }
    if (character === ESC || character === CSI) {
      isInsideEscape = true;
      isInsideLinkEscape = word.startsWith(ANSI_ESCAPE_LINK, rawCharacterIndex + 1);
    }
    if (isInsideEscape) {
      if (isInsideLinkEscape) {
        if (character === ANSI_ESCAPE_BELL) {
          isInsideEscape = false;
          isInsideLinkEscape = false;
        }
      } else if (character === ANSI_SGR_TERMINATOR) {
        isInsideEscape = false;
      }
    } else {
      visible += characterLength;
      if (visible === columns && !nextCharacter.done) {
        rows.push("");
        visible = 0;
      }
    }
    currentCharacter = nextCharacter;
    nextCharacter = characters.next();
    rawCharacterIndex += character.length;
  }
  lastRow = rows.at(-1);
  if (!visible && lastRow !== void 0 && lastRow.length && rows.length > 1) {
    rows[rows.length - 2] += rows.pop();
  }
};
var stringVisibleTrimSpacesRight = (string) => {
  const words = string.split(" ");
  let last = words.length;
  while (last) {
    if (dist_default2(words[last - 1])) {
      break;
    }
    last--;
  }
  if (last === words.length) {
    return string;
  }
  return words.slice(0, last).join(" ") + words.slice(last).join("");
};
var exec = (string, columns, options = {}) => {
  if (options.trim !== false && string.trim() === "") {
    return "";
  }
  let returnValue = "";
  let escapeCode;
  let escapeUrl;
  const words = string.split(" ");
  let rows = [""];
  let rowLength = 0;
  for (let index = 0; index < words.length; index++) {
    const word = words[index];
    if (options.trim !== false) {
      const row = rows.at(-1) ?? "";
      const trimmed = row.trimStart();
      if (row.length !== trimmed.length) {
        rows[rows.length - 1] = trimmed;
        rowLength = dist_default2(trimmed);
      }
    }
    if (index !== 0) {
      if (rowLength >= columns && (options.wordWrap === false || options.trim === false)) {
        rows.push("");
        rowLength = 0;
      }
      if (rowLength || options.trim === false) {
        rows[rows.length - 1] += " ";
        rowLength++;
      }
    }
    const wordLength = dist_default2(word);
    if (options.hard && wordLength > columns) {
      const remainingColumns = columns - rowLength;
      const breaksStartingThisLine = 1 + Math.floor((wordLength - remainingColumns - 1) / columns);
      const breaksStartingNextLine = Math.floor((wordLength - 1) / columns);
      if (breaksStartingNextLine < breaksStartingThisLine) {
        rows.push("");
      }
      wrapWord(rows, word, columns);
      rowLength = dist_default2(rows.at(-1) ?? "");
      continue;
    }
    if (rowLength + wordLength > columns && rowLength && wordLength) {
      if (options.wordWrap === false && rowLength < columns) {
        wrapWord(rows, word, columns);
        rowLength = dist_default2(rows.at(-1) ?? "");
        continue;
      }
      rows.push("");
      rowLength = 0;
    }
    if (rowLength + wordLength > columns && options.wordWrap === false) {
      wrapWord(rows, word, columns);
      rowLength = dist_default2(rows.at(-1) ?? "");
      continue;
    }
    rows[rows.length - 1] += word;
    rowLength += wordLength;
  }
  if (options.trim !== false) {
    rows = rows.map((row) => stringVisibleTrimSpacesRight(row));
  }
  const preString = rows.join("\n");
  let inSurrogate = false;
  for (let i = 0; i < preString.length; i++) {
    const character = preString[i];
    returnValue += character;
    if (!inSurrogate) {
      inSurrogate = character >= "\uD800" && character <= "\uDBFF";
      if (inSurrogate) {
        continue;
      }
    } else {
      inSurrogate = false;
    }
    if (character === ESC || character === CSI) {
      GROUP_REGEX.lastIndex = i + 1;
      const groupsResult = GROUP_REGEX.exec(preString);
      const groups = groupsResult?.groups;
      if (groups?.code !== void 0) {
        const code = Number.parseFloat(groups.code);
        escapeCode = code === END_CODE ? void 0 : code;
      } else if (groups?.uri !== void 0) {
        escapeUrl = groups.uri.length === 0 ? void 0 : groups.uri;
      }
    }
    if (preString[i + 1] === "\n") {
      if (escapeUrl) {
        returnValue += wrapAnsiHyperlink("");
      }
      const closingCode = escapeCode ? getClosingCode(escapeCode) : void 0;
      if (escapeCode && closingCode) {
        returnValue += wrapAnsiCode(closingCode);
      }
    } else if (character === "\n") {
      if (escapeCode && getClosingCode(escapeCode)) {
        returnValue += wrapAnsiCode(escapeCode);
      }
      if (escapeUrl) {
        returnValue += wrapAnsiHyperlink(escapeUrl);
      }
    }
  }
  return returnValue;
};
var CRLF_OR_LF = /\r?\n/;
function wrapAnsi(string, columns, options) {
  return String(string).normalize().split(CRLF_OR_LF).map((line) => exec(line, columns, options)).join("\n");
}

// node_modules/@clack/core/dist/index.mjs
var import_sisteransi = __toESM(require_src(), 1);
import { ReadStream } from "tty";
function findCursor(s, o, l2) {
  if (!l2.some((r) => !r.disabled))
    return s;
  const t2 = s + o, n = Math.max(l2.length - 1, 0), e = t2 < 0 ? n : t2 > n ? 0 : t2;
  return l2[e].disabled ? findCursor(e, o < 0 ? -1 : 1, l2) : e;
}
var a$2 = ["up", "down", "left", "right", "space", "enter", "cancel"];
var t = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December"
];
var settings = {
  actions: new Set(a$2),
  aliases: /* @__PURE__ */ new Map([
    // vim support
    ["k", "up"],
    ["j", "down"],
    ["h", "left"],
    ["l", "right"],
    ["", "cancel"],
    // opinionated defaults!
    ["escape", "cancel"]
  ]),
  messages: {
    cancel: "Canceled",
    error: "Something went wrong"
  },
  withGuide: true,
  date: {
    monthNames: [...t],
    messages: {
      required: "Please enter a valid date",
      invalidMonth: "There are only 12 months in a year",
      invalidDay: (n, e) => `There are only ${n} days in ${e}`,
      afterMin: (n) => `Date must be on or after ${n.toISOString().slice(0, 10)}`,
      beforeMax: (n) => `Date must be on or before ${n.toISOString().slice(0, 10)}`
    }
  }
};
function isActionKey(n, e) {
  if (typeof n == "string")
    return settings.aliases.get(n) === e;
  for (const s of n)
    if (s !== void 0 && isActionKey(s, e))
      return true;
  return false;
}
function diffLines(i, s) {
  if (i === s) return;
  const e = i.split(`
`), t2 = s.split(`
`), r = Math.max(e.length, t2.length), f = [];
  for (let n = 0; n < r; n++)
    e[n] !== t2[n] && f.push(n);
  return {
    lines: f,
    numLinesBefore: e.length,
    numLinesAfter: t2.length,
    numLines: r
  };
}
var R = globalThis.process.platform.startsWith("win");
var CANCEL_SYMBOL = /* @__PURE__ */ Symbol("clack:cancel");
function setRawMode(e, r) {
  const o = e;
  o.isTTY && o.setRawMode(r);
}
var getRows = (e) => "rows" in e && typeof e.rows == "number" ? e.rows : 20;
function runValidation(e, n) {
  if ("~standard" in e) {
    const a = e["~standard"].validate(n);
    if (a instanceof Promise)
      throw new TypeError(
        "Schema validation must be synchronous. Update `validate()` and remove any asynchronous logic."
      );
    return a.issues?.at(0)?.message;
  }
  return e(n);
}
var V = class {
  input;
  output;
  _abortSignal;
  rl;
  opts;
  _render;
  _track = false;
  _prevFrame = "";
  _subscribers = /* @__PURE__ */ new Map();
  _cursor = 0;
  state = "initial";
  error = "";
  value;
  userInput = "";
  constructor(t2, e = true) {
    const { input: i = stdin, output: n = stdout, render: s, signal: r, ...o } = t2;
    this.opts = o, this.onKeypress = this.onKeypress.bind(this), this.close = this.close.bind(this), this.render = this.render.bind(this), this._render = s.bind(this), this._track = e, this._abortSignal = r, this.input = i, this.output = n;
  }
  /**
   * Unsubscribe all listeners
   */
  unsubscribe() {
    this._subscribers.clear();
  }
  /**
   * Set a subscriber with opts
   * @param event - The event name
   */
  setSubscriber(t2, e) {
    const i = this._subscribers.get(t2) ?? [];
    i.push(e), this._subscribers.set(t2, i);
  }
  /**
   * Subscribe to an event
   * @param event - The event name
   * @param cb - The callback
   */
  on(t2, e) {
    this.setSubscriber(t2, { cb: e });
  }
  /**
   * Subscribe to an event once
   * @param event - The event name
   * @param cb - The callback
   */
  once(t2, e) {
    this.setSubscriber(t2, { cb: e, once: true });
  }
  /**
   * Emit an event with data
   * @param event - The event name
   * @param data - The data to pass to the callback
   */
  emit(t2, ...e) {
    const i = this._subscribers.get(t2) ?? [], n = [];
    for (const s of i)
      s.cb(...e), s.once && n.push(() => i.splice(i.indexOf(s), 1));
    for (const s of n)
      s();
  }
  prompt() {
    return new Promise((t2) => {
      if (this._abortSignal) {
        if (this._abortSignal.aborted)
          return this.state = "cancel", this.close(), t2(CANCEL_SYMBOL);
        this._abortSignal.addEventListener(
          "abort",
          () => {
            this.state = "cancel", this.close();
          },
          { once: true }
        );
      }
      this.rl = l__default.createInterface({
        input: this.input,
        tabSize: 2,
        prompt: "",
        escapeCodeTimeout: 50,
        terminal: true
      }), this.rl.prompt(), this.opts.initialUserInput !== void 0 && this._setUserInput(this.opts.initialUserInput, true), this.input.on("keypress", this.onKeypress), setRawMode(this.input, true), this.output.on("resize", this.render), this.render(), this.once("submit", () => {
        this.output.write(import_sisteransi.cursor.show), this.output.off("resize", this.render), setRawMode(this.input, false), t2(this.value);
      }), this.once("cancel", () => {
        this.output.write(import_sisteransi.cursor.show), this.output.off("resize", this.render), setRawMode(this.input, false), t2(CANCEL_SYMBOL);
      });
    });
  }
  _isActionKey(t2, e) {
    return t2 === "	";
  }
  _shouldSubmit(t2, e) {
    return true;
  }
  _setValue(t2) {
    this.value = t2, this.emit("value", this.value);
  }
  _setUserInput(t2, e) {
    this.userInput = t2 ?? "", this.emit("userInput", this.userInput), e && this._track && this.rl && (this.rl.write(this.userInput), this._cursor = this.rl.cursor);
  }
  _clearUserInput() {
    this.rl?.write(null, { ctrl: true, name: "u" }), this._setUserInput("");
  }
  onKeypress(t2, e) {
    if (this._track && e.name !== "return" && (e.name && this._isActionKey(t2, e) && this.rl?.write(null, { ctrl: true, name: "h" }), this._cursor = this.rl?.cursor ?? 0, this._setUserInput(this.rl?.line)), this.state === "error" && (this.state = "active"), e?.name && (!this._track && settings.aliases.has(e.name) && this.emit("cursor", settings.aliases.get(e.name)), settings.actions.has(e.name) && this.emit("cursor", e.name)), t2 && (t2.toLowerCase() === "y" || t2.toLowerCase() === "n") && this.emit("confirm", t2.toLowerCase() === "y"), this.emit("key", t2, e), e?.name === "return" && this._shouldSubmit(t2, e)) {
      if (this.opts.validate) {
        const i = runValidation(this.opts.validate, this.value);
        i && (this.error = i instanceof Error ? i.message : i, this.state = "error", this.rl?.write(this.userInput));
      }
      this.state !== "error" && (this.state = "submit");
    }
    isActionKey([t2, e?.name, e?.sequence], "cancel") && (this.state = "cancel"), (this.state === "submit" || this.state === "cancel") && this.emit("finalize"), this.render(), (this.state === "submit" || this.state === "cancel") && this.close();
  }
  close() {
    this.input.unpipe(), this.input.removeListener("keypress", this.onKeypress), this.output.write(`
`), setRawMode(this.input, false), this.rl?.close(), this.rl = void 0, this.emit(`${this.state}`, this.value), this.unsubscribe();
  }
  restoreCursor() {
    const t2 = wrapAnsi(this._prevFrame, process.stdout.columns, { hard: true, trim: false }).split(`
`).length - 1;
    this.output.write(import_sisteransi.cursor.move(-999, t2 * -1));
  }
  render() {
    const t2 = wrapAnsi(this._render(this) ?? "", process.stdout.columns, {
      hard: true,
      trim: false
    });
    if (t2 !== this._prevFrame) {
      if (this.state === "initial")
        this.output.write(import_sisteransi.cursor.hide);
      else {
        const e = diffLines(this._prevFrame, t2), i = getRows(this.output);
        if (this.restoreCursor(), e) {
          const n = Math.max(0, e.numLinesAfter - i), s = Math.max(0, e.numLinesBefore - i);
          let r = e.lines.find((o) => o >= n);
          if (r === void 0) {
            this._prevFrame = t2;
            return;
          }
          if (e.lines.length === 1) {
            this.output.write(import_sisteransi.cursor.move(0, r - s)), this.output.write(import_sisteransi.erase.lines(1));
            const o = t2.split(`
`);
            this.output.write(o[r]), this._prevFrame = t2, this.output.write(import_sisteransi.cursor.move(0, o.length - r - 1));
            return;
          } else if (e.lines.length > 1) {
            if (n < s)
              r = n;
            else {
              const h = r - s;
              h > 0 && this.output.write(import_sisteransi.cursor.move(0, h));
            }
            this.output.write(import_sisteransi.erase.down());
            const f = t2.split(`
`).slice(r);
            this.output.write(f.join(`
`)), this._prevFrame = t2;
            return;
          }
        }
        this.output.write(import_sisteransi.erase.down());
      }
      this.output.write(t2), this.state === "initial" && (this.state = "active"), this._prevFrame = t2;
    }
  }
};
function p$1(l2, e) {
  if (l2 === void 0 || e.length === 0)
    return 0;
  const i = e.findIndex((s) => s.value === l2);
  return i !== -1 ? i : 0;
}
function g(l2, e) {
  return (e.label ?? String(e.value)).toLowerCase().includes(l2.toLowerCase());
}
function m(l2, e) {
  if (e)
    return l2 ? e : e[0];
}
var T$1 = class T extends V {
  filteredOptions;
  multiple;
  isNavigating = false;
  selectedValues = [];
  focusedValue;
  #e = 0;
  #s = "";
  #t;
  #i;
  #n;
  get cursor() {
    return this.#e;
  }
  get userInputWithCursor() {
    if (!this.userInput)
      return styleText(["inverse", "hidden"], "_");
    if (this._cursor >= this.userInput.length)
      return `${this.userInput}\u2588`;
    const e = this.userInput.slice(0, this._cursor), [t2, ...i] = this.userInput.slice(this._cursor);
    return `${e}${styleText("inverse", t2)}${i.join("")}`;
  }
  get options() {
    return typeof this.#i == "function" ? this.#i() : this.#i;
  }
  constructor(e) {
    super(e), this.#i = e.options, this.#n = e.placeholder;
    const t2 = this.options;
    this.filteredOptions = [...t2], this.multiple = e.multiple === true, this.#t = typeof e.options == "function" ? e.filter : e.filter ?? g;
    let i;
    if (e.initialValue && Array.isArray(e.initialValue) ? this.multiple ? i = e.initialValue : i = e.initialValue.slice(0, 1) : !this.multiple && this.options.length > 0 && (i = [this.options[0].value]), i)
      for (const s of i) {
        const n = t2.findIndex((o) => o.value === s);
        n !== -1 && (this.toggleSelected(s), this.#e = n);
      }
    this.focusedValue = this.options[this.#e]?.value, this.on("key", (s, n) => this.#l(s, n)), this.on("userInput", (s) => this.#u(s));
  }
  _isActionKey(e, t2) {
    return e === "	" || this.multiple && this.isNavigating && t2.name === "space" && e !== void 0 && e !== "";
  }
  #l(e, t2) {
    const i = t2.name === "up", s = t2.name === "down", n = t2.name === "return", o = this.userInput === "" || this.userInput === "	", u = this.#n, h = this.options, f = u !== void 0 && u !== "" && h.some(
      (r) => !r.disabled && (this.#t ? this.#t(u, r) : true)
    );
    if (t2.name === "tab" && o && f) {
      this.userInput === "	" && this._clearUserInput(), this._setUserInput(u, true), this.isNavigating = false;
      return;
    }
    i || s ? (this.#e = findCursor(this.#e, i ? -1 : 1, this.filteredOptions), this.focusedValue = this.filteredOptions[this.#e]?.value, this.multiple || (this.selectedValues = [this.focusedValue]), this.isNavigating = true) : n ? this.value = m(this.multiple, this.selectedValues) : this.multiple ? this.focusedValue !== void 0 && (t2.name === "tab" || this.isNavigating && t2.name === "space") ? this.toggleSelected(this.focusedValue) : this.isNavigating = false : (this.focusedValue && (this.selectedValues = [this.focusedValue]), this.isNavigating = false);
  }
  deselectAll() {
    this.selectedValues = [];
  }
  toggleSelected(e) {
    this.filteredOptions.length !== 0 && (this.multiple ? this.selectedValues.includes(e) ? this.selectedValues = this.selectedValues.filter((t2) => t2 !== e) : this.selectedValues = [...this.selectedValues, e] : this.selectedValues = [e]);
  }
  #u(e) {
    if (e !== this.#s) {
      this.#s = e;
      const t2 = this.options;
      e && this.#t ? this.filteredOptions = t2.filter((n) => this.#t?.(e, n)) : this.filteredOptions = [...t2];
      const i = p$1(this.focusedValue, this.filteredOptions);
      this.#e = findCursor(i, 0, this.filteredOptions);
      const s = this.filteredOptions[this.#e];
      s && !s.disabled ? this.focusedValue = s.value : this.focusedValue = void 0, this.multiple || (this.focusedValue !== void 0 ? this.toggleSelected(this.focusedValue) : this.deselectAll());
    }
  }
};

// src/select-prompt.ts
import {
  limitOptions,
  symbol,
  S_BAR,
  S_BAR_END,
  S_CHECKBOX_SELECTED,
  S_CHECKBOX_INACTIVE
} from "@clack/prompts";
function styleItem(opt, active, selected, focused) {
  const isSel = selected.includes(opt.value);
  const label = opt.label ?? String(opt.value ?? "");
  const hint = opt.hint && focused !== void 0 && opt.value === focused ? styleText2("dim", ` (${opt.hint})`) : "";
  const box = isSel ? styleText2("green", S_CHECKBOX_SELECTED) : styleText2("dim", S_CHECKBOX_INACTIVE);
  if (opt.disabled) {
    return `  ${styleText2("gray", S_CHECKBOX_INACTIVE)} ${styleText2(["strikethrough", "gray"], label)}`;
  }
  if (active) return `${styleText2("green", "\u203A")} ${box} ${styleText2("green", label)}${hint}`;
  if (isSel) return `  ${box} ${styleText2("green", label)}${hint}`;
  return `  ${box} ${styleText2("dim", label)}`;
}
function colorAutocompleteMultiselect(opts) {
  const filter = (input, option) => (option.label ?? String(option.value ?? "")).toLowerCase().includes(input.toLowerCase());
  const prompt = new T$1({
    options: opts.options,
    multiple: true,
    placeholder: opts.placeholder,
    filter,
    render() {
      const withGuide = settings.withGuide;
      const title = `${withGuide ? styleText2("gray", S_BAR) + "\n" : ""}${symbol(this.state)}  ${opts.message}
`;
      const ph = opts.placeholder;
      const showPh = this.userInput === "" && ph !== void 0;
      const value = this.isNavigating || showPh ? styleText2("dim", showPh ? ph : this.userInput) : this.userInputWithCursor;
      const matchInfo = this.filteredOptions.length !== this.options.length ? styleText2("dim", ` (${this.filteredOptions.length} match${this.filteredOptions.length === 1 ? "" : "es"})`) : "";
      if (this.state === "submit") {
        return `${title}${withGuide ? styleText2("gray", S_BAR) + "  " : ""}${styleText2("dim", `${this.selectedValues.length} items selected`)}`;
      }
      if (this.state === "cancel") {
        return `${title}${withGuide ? styleText2("gray", S_BAR) + "  " : ""}${styleText2(["strikethrough", "dim"], this.userInput)}`;
      }
      const color = this.state === "error" ? "yellow" : "cyan";
      const bar = withGuide ? `${styleText2(color, S_BAR)}  ` : "";
      const barEnd = withGuide ? styleText2(color, S_BAR_END) : "";
      const footer = [
        `${styleText2("dim", "\u2191/\u2193")} to navigate`,
        `${styleText2("dim", this.isNavigating ? "Space/Tab:" : "Tab:")} select`,
        `${styleText2("dim", "Enter:")} confirm`,
        `${styleText2("dim", "Type:")} to search`
      ];
      const noMatch = this.filteredOptions.length === 0 && this.userInput ? [`${bar}${styleText2("yellow", "No matches found")}`] : [];
      const errLines = this.state === "error" ? [`${bar}${styleText2("yellow", this.error)}`] : [];
      const head = [
        ...`${title}${withGuide ? styleText2(color, S_BAR) : ""}`.split("\n"),
        `${bar}${styleText2("dim", "Search:")} ${value}${matchInfo}`,
        ...noMatch,
        ...errLines
      ];
      const foot = [`${bar}${footer.join(" \u2022 ")}`, barEnd];
      const body = limitOptions({
        cursor: this.cursor,
        options: this.filteredOptions,
        style: (o, active) => styleItem(o, active, this.selectedValues, this.focusedValue),
        maxItems: opts.maxItems,
        rowPadding: head.length + foot.length
      });
      return [...head, ...body.map((f) => `${bar}${f}`), ...foot].join("\n");
    }
  });
  return prompt.prompt();
}

// src/model.ts
function runsToText(runs) {
  return runs.map((r) => typeof r === "string" ? r : r.text).join("");
}
function flattenText(m2) {
  if (Array.isArray(m2.text_entities) && m2.text_entities.length) return runsToText(m2.text_entities);
  if (typeof m2.text === "string") return m2.text;
  if (Array.isArray(m2.text)) return runsToText(m2.text);
  return "";
}
function resolveName(m2) {
  if (m2.from != null && m2.from !== "") return m2.from;
  if (m2.from_id != null) return String(m2.from_id);
  if (m2.actor != null && m2.actor !== "") return m2.actor;
  if (m2.actor_id != null) return String(m2.actor_id);
  return "unknown";
}
function stripService(msgs) {
  return msgs.filter((m2) => m2.type !== "service");
}
function groupByTopic(msgs) {
  const byId = /* @__PURE__ */ new Map();
  const topicTitles = /* @__PURE__ */ new Map([["1", "General"]]);
  for (const m2 of msgs) {
    byId.set(String(m2.id), m2);
    if (m2.type === "service" && m2.action === "topic_created") {
      topicTitles.set(String(m2.id), m2.title ?? "Untitled");
    }
  }
  const resolveTopic = (m2) => {
    let cur = m2;
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
  for (const m2 of msgs) {
    if (m2.type === "service") continue;
    const topicId = resolveTopic(m2);
    let g2 = groups.get(topicId);
    if (!g2) {
      g2 = { topicId, title: topicTitles.get(topicId) ?? "General", messages: [] };
      groups.set(topicId, g2);
    }
    g2.messages.push(m2);
  }
  return [...groups.values()];
}
function isForum(msgs) {
  return msgs.some((m2) => m2.type === "service" && m2.action === "topic_created");
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
  const dates = msgs.map((m2) => m2.date).filter((d) => !!d);
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
      topics = groupByTopic(chat.messages).map((g2) => ({
        topicId: g2.topicId,
        title: g2.title,
        count: g2.messages.length,
        ...dateRange(g2.messages)
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
  const byChat = /* @__PURE__ */ new Map();
  for (const s of selection) {
    const e = byChat.get(s.chatId) ?? { whole: false, topicIds: /* @__PURE__ */ new Set() };
    if (!s.topicIds || !s.topicIds.length) e.whole = true;
    else for (const t2 of s.topicIds) e.topicIds.add(t2);
    byChat.set(s.chatId, e);
  }
  const units = [];
  for await (const chat of streamChats(path)) {
    const e = byChat.get(String(chat.id));
    if (!e) continue;
    const name = chat.name ?? `(no name ${chat.id})`;
    if (e.whole) {
      units.push({ chatName: name, messages: stripService(chat.messages) });
    }
    if (e.topicIds.size) {
      const groups = groupByTopic(chat.messages);
      for (const topicId of e.topicIds) {
        const g2 = groups.find((x) => x.topicId === topicId);
        if (g2) units.push({ chatName: name, topicTitle: g2.title, messages: g2.messages });
      }
    }
  }
  return units;
}

// src/write-output.ts
import { mkdirSync, writeFileSync, readdirSync, rmSync } from "fs";
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
function mediaMarker(m2) {
  if (m2.media_type === "voice_message" || m2.media_type === "video_message" || m2.media_type === "audio_file") {
    const d = m2.duration_seconds ?? 0;
    const mm = Math.floor(d / 60), ss = String(d % 60).padStart(2, "0");
    const kind = m2.media_type === "voice_message" ? "voice" : m2.media_type === "video_message" ? "video" : "audio";
    return `[${kind} ${mm}:${ss}]`;
  }
  if (m2.media_type === "sticker") return `[sticker ${m2.sticker_emoji ?? ""}]`.trim();
  if (m2.media_type === "animation" || m2.media_type === "video_file") return "[video]";
  if (m2.photo) return "[photo]";
  if (m2.file) return `[file: ${m2.file_name ?? "attachment"}]`;
  return null;
}
function lineFor(m2, byId) {
  const name = resolveName(m2);
  const time = timeOf(m2.date);
  let reply = "";
  if (m2.reply_to_message_id != null) {
    const target = byId.get(String(m2.reply_to_message_id));
    if (target) {
      const q = flattenText(target).slice(0, 40);
      reply = ` \u21B3 ${resolveName(target)} \xAB${q}\xBB`;
    }
  }
  const text2 = flattenText(m2);
  const marker = mediaMarker(m2);
  const body = [marker, text2].filter(Boolean).join(" ").trim();
  return `[${time}] ${name}${reply}: ${body}`;
}
function formatUnit(unit, maxTokens = 9e4) {
  const byId = new Map(unit.messages.map((m2) => [String(m2.id), m2]));
  const dates = unit.messages.map((m2) => m2.date).filter((d) => !!d);
  const participants = [...new Set(unit.messages.map(resolveName))].join(", ");
  const period = dates.length ? `${dayOf(dates[0])} \u2014 ${dayOf(dates[dates.length - 1])}` : "n/a";
  const titleLine = unit.topicTitle ? `# \u0427\u0430\u0442: ${unit.chatName} / \u0422\u043E\u043F\u0438\u043A: ${unit.topicTitle}` : `# \u0427\u0430\u0442: ${unit.chatName}`;
  const header = `${titleLine}
# \u041F\u0435\u0440\u0438\u043E\u0434: ${period} | \u0441\u043E\u043E\u0431\u0449\u0435\u043D\u0438\u0439: ${unit.messages.length} | \u0443\u0447\u0430\u0441\u0442\u043D\u0438\u043A\u0438: ${participants}
`;
  const blocks = [];
  let curDay = "";
  for (const m2 of unit.messages) {
    const day = dayOf(m2.date);
    if (day !== curDay) {
      blocks.push({ day, text: `
## ${day}`, isDayHeader: true });
      curDay = day;
    }
    blocks.push({ day, text: lineFor(m2, byId), isDayHeader: false });
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
function clearStem(outDir, stem) {
  let entries;
  try {
    entries = readdirSync(outDir);
  } catch {
    return;
  }
  const esc = stem.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(`^${esc}(\\.part-\\d+)?\\.md$`);
  for (const f of entries) if (re.test(f)) rmSync(join(outDir, f), { force: true });
}
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
    clearStem(outDir, stem);
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
var lerp = (a, b, t2) => Math.round(a + (b - a) * t2);
function renderLogo(color = true) {
  if (!color) return BANNER.join("\n");
  const [tr, tg, tb] = [0, 255, 255];
  const [br, bg, bb] = [80, 120, 255];
  return BANNER.map((line, i) => {
    const t2 = BANNER.length > 1 ? i / (BANNER.length - 1) : 0;
    const r = lerp(tr, br, t2), g2 = lerp(tg, bg, t2), b = lerp(tb, bb, t2);
    return `\x1B[1;38;2;${r};${g2};${b}m${line}\x1B[0m`;
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
      for (const t2 of c.topics) {
        const key = `${c.chatId}::${t2.topicId}`;
        options.push({ value: key, label: `${c.name} \u203A ${t2.title}`, hint: `${t2.count} msgs` });
        decode.set(key, { chatId: c.chatId, topicIds: [t2.topicId] });
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
  const picked = await colorAutocompleteMultiselect({
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
