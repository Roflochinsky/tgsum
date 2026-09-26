// tgsum UI — plain ES module, no build step. All heavy lifting happens in
// Rust (`src-tauri`); this file only renders state and calls commands.

import { mountPaintings } from './paint.js'
import { mountProjects } from './projects.js'

const { invoke } = window.__TAURI__.core
const { listen } = window.__TAURI__.event

const $ = (sel) => document.querySelector(sel)

const IS_MAC = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent)
document.documentElement.classList.toggle('mac', IS_MAC)
for (const k of document.querySelectorAll('.mod-key')) {
  k.textContent = k.textContent.replace('⌘', IS_MAC ? '⌘' : 'Ctrl+')
}

// ---------- formatting ----------

const nf = new Intl.NumberFormat('ru-RU')
const nf1 = new Intl.NumberFormat('ru-RU', { maximumFractionDigits: 1 })

function plural(n, one, few, many) {
  const m10 = n % 10
  const m100 = n % 100
  if (m10 === 1 && m100 !== 11) return one
  if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return few
  return many
}
const MSGS = ['сообщение', 'сообщения', 'сообщений']
const CHATS = ['чат', 'чата', 'чатов']
const TOPICS = ['топик', 'топика', 'топиков']
const FILES = ['файл', 'файла', 'файлов']
const count = (n, forms) => `${nf.format(n)} ${plural(n, ...forms)}`

function formatBytes(b) {
  const units = ['Б', 'КБ', 'МБ', 'ГБ', 'ТБ']
  let i = 0
  while (b >= 1024 && i < units.length - 1) { b /= 1024; i++ }
  return `${i ? nf1.format(b) : Math.round(b)} ${units[i]}`
}

function formatDuration(s) {
  s = Math.ceil(s)
  return s < 60 ? `${s} с` : `${Math.floor(s / 60)} мин ${s % 60} с`
}

function day(d) {
  const [y, m, dd] = (d || '').slice(0, 10).split('-')
  return y && m && dd ? `${dd}.${m}.${y}` : (d || '').slice(0, 10)
}

function period(first, last) {
  if (!first) return ''
  const a = day(first)
  const b = day(last || first)
  return a === b ? a : `${a} — ${b}`
}

const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c])

function highlight(text, q) {
  const lower = text.toLowerCase()
  const i = q && lower.length === text.length ? lower.indexOf(q) : -1
  if (i < 0) return esc(text)
  return `${esc(text.slice(0, i))}<mark>${esc(text.slice(i, i + q.length))}</mark>${esc(text.slice(i + q.length))}`
}

const basename = (p) => p.split(/[\\/]/).filter(Boolean).pop() || p

function initials(name) {
  const words = name.replace(/[^\p{L}\p{N}\s]/gu, ' ').trim().split(/\s+/).filter(Boolean)
  if (!words.length) return '#'
  return ([...words[0]][0] + (words[1] ? [...words[1]][0] : '')).toUpperCase()
}

// Stable, well-spread avatar hue per chat (FNV-1a + golden-angle step).
function hue(id) {
  let h = 0x811c9dc5
  for (let i = 0; i < id.length; i++) h = Math.imul(h ^ id.charCodeAt(i), 0x01000193) >>> 0
  return Math.round(((h % 1000) * 137.508) % 360)
}

const TYPES = {
  personal_chat: 'Личный чат',
  bot_chat: 'Бот',
  saved_messages: 'Избранное',
  replies: 'Ответы',
  verification_codes: 'Коды подтверждения',
  private_group: 'Группа',
  private_supergroup: 'Супергруппа',
  public_supergroup: 'Публичная супергруппа',
  private_channel: 'Канал',
  public_channel: 'Публичный канал',
}
const typeLabel = (t) => TYPES[t] || t || 'Чат'

function category(c) {
  if (c.topics.length) return 'forum'
  if (/channel/.test(c.type)) return 'channel'
  if (/group/.test(c.type)) return 'group'
  return 'personal'
}
const FILTERS = [['all', 'Все'], ['personal', 'Личные'], ['group', 'Группы'], ['channel', 'Каналы'], ['forum', 'Форумы']]

// 0 = never split.
const BUDGETS = [[30000, '30k'], [60000, '60k'], [90000, '90k', 'рекомендуется'], [150000, '150k'], [0, 'Без разбивки']]
const NO_SPLIT = Number.MAX_SAFE_INTEGER

// ---------- state ----------

const state = {
  screen: 'start',
  index: null, // { path, fileName, size, outDir, chats }
  items: [], // prepared chats
  keys: new Map(), // row key -> { item, topic? }
  selected: new Map(), // row key -> { chatId, topicIds?, label, count }
  expanded: new Set(), // forum chatIds
  visible: [],
  query: '',
  filter: 'all',
  sort: 'export',
  outDir: '',
  budget: 90000,
  job: null, // { phase, started }
  result: null,
}

const STEPS = { start: 1, select: 2, save: 3, done: 3 }
const projects = mountProjects({ invoke, show, pickFile, startJob,
  endJob: () => { state.job = null }, busy: () => Boolean(state.job),
  selection: () => [...state.selected.values()], index: () => state.index, toast })

function show(name) {
  state.screen = name
  document.body.dataset.screen = name
  for (const s of document.querySelectorAll('.screen')) s.hidden = s.id !== `screen-${name}`
  const step = name === 'progress' ? (state.job?.phase === 'extract' ? 3 : 1) : STEPS[name]
  for (const li of document.querySelectorAll('.steps li')) {
    const n = Number(li.dataset.step)
    li.classList.toggle('is-active', n === step && name !== 'done')
    li.classList.toggle('is-done', n < step || name === 'done')
  }
  if (name === 'start') requestAnimationFrame(placeStarTags)
}

let toastTimer
function toast(msg, kind = '') {
  const t = $('#toast')
  t.textContent = msg
  t.className = `toast ${kind}`
  t.hidden = false
  clearTimeout(toastTimer)
  toastTimer = setTimeout(() => { t.hidden = true }, kind === 'error' ? 7000 : 3000)
}

const isCancel = (e) => e && e.kind === 'cancelled'
const errText = (e) => (e && e.message) || String(e)

// ---------- progress ----------

function startJob(phase, fileName) {
  state.job = { phase, started: performance.now() }
  $('#progress-title').textContent = phase === 'index' ? 'Читаю выгрузку…' : 'Извлекаю и сохраняю…'
  $('#progress-file').textContent = fileName
  $('#btn-cancel').disabled = false
  setProgress(0, 0)
  show('progress')
}

function setProgress(read, total) {
  const pct = total ? Math.min(100, (read / total) * 100) : 0
  $('#progress-fill').style.width = `${pct.toFixed(1)}%`
  $('#progress-percent').textContent = `${Math.floor(pct)}%`
  let detail = total ? `${formatBytes(read)} из ${formatBytes(total)}` : ''
  const secs = (performance.now() - state.job.started) / 1000
  if (total && read > 0 && secs > 0.7) {
    const speed = read / secs
    detail += ` · ${formatBytes(speed)}/с`
    const left = (total - read) / speed
    if (left >= 1) detail += ` · осталось ~${formatDuration(left)}`
  }
  $('#progress-detail').textContent = detail
}

listen('progress', ({ payload }) => {
  if (state.screen === 'progress' && state.job?.phase === payload.phase) setProgress(payload.read, payload.total)
})

$('#btn-cancel').addEventListener('click', () => {
  $('#btn-cancel').disabled = true
  invoke('cancel_job')
})

// ---------- step 1: file ----------

function showStartError(msg) {
  const el = $('#start-error')
  el.textContent = msg
  el.hidden = !msg
  show('start')
}

async function pickFile() {
  if (state.job) return
  const path = await invoke('pick_export')
  if (path) await openExport(path)
  return Boolean(path)
}

async function openExport(path) {
  if (state.job) return
  $('#start-error').hidden = true
  startJob('index', basename(path))
  try {
    const idx = await invoke('index_export', { path })
    state.job = null
    if (!idx.chats.length) {
      showStartError('В этом файле нет чатов. Нужен result.json из «Экспорта данных из Telegram» в формате «Машиночитаемый JSON».')
      return
    }
    loadIndex(idx)
  } catch (e) {
    state.job = null
    if (isCancel(e)) show(state.index ? 'select' : 'start')
    else showStartError(errText(e))
  }
}

$('#dropzone').addEventListener('click', pickFile)

// ---------- step 2: chats ----------

function loadIndex(idx) {
  state.index = idx
  state.keys.clear()
  state.items = idx.chats.map((chat, order) => {
    const item = {
      chat,
      order,
      key: `c:${chat.chatId}`,
      lower: chat.name.toLowerCase(),
      cat: category(chat),
      topics: [],
    }
    item.topics = chat.topics.map((t) => ({ t, key: `t:${chat.chatId}:${t.topicId}`, lower: t.title.toLowerCase() }))
    state.keys.set(item.key, { item })
    for (const topic of item.topics) state.keys.set(topic.key, { item, topic })
    return item
  })
  state.selected.clear()
  state.expanded.clear()
  state.query = ''
  state.filter = 'all'
  $('#search').value = ''
  state.outDir = idx.outDir

  const topics = idx.chats.reduce((n, c) => n + c.topics.length, 0)
  $('#file-name').textContent = idx.fileName
  $('#file-name').closest('.file-chip').title = idx.path
  $('#file-meta').textContent = [formatBytes(idx.size), count(idx.chats.length, CHATS), topics ? count(topics, TOPICS) : '']
    .filter(Boolean).join(' · ')

  renderFilters()
  renderList()
  renderSummary()
  show('select')
  $('#list').scrollTop = 0
  $('#search').focus()
}

function sortedItems() {
  const items = state.items.slice()
  switch (state.sort) {
    case 'count': return items.sort((a, b) => b.chat.count - a.chat.count)
    case 'recent': return items.sort((a, b) => (b.chat.lastDate || '').localeCompare(a.chat.lastDate || ''))
    case 'name': return items.sort((a, b) => a.chat.name.localeCompare(b.chat.name, 'ru'))
    default: return items
  }
}

// Chats (and their topics) matching the filter and search, in display order.
function visibleItems() {
  const q = state.query
  const out = []
  for (const item of sortedItems()) {
    if (state.filter !== 'all' && item.cat !== state.filter) continue
    const id = item.chat.chatId
    if (!q) {
      out.push({ item, topics: item.topics, open: state.expanded.has(id) })
      continue
    }
    const self = item.lower.includes(q)
    const topics = self ? item.topics : item.topics.filter((t) => t.lower.includes(q))
    if (self || topics.length) out.push({ item, topics, open: topics.length > 0 && (!self || state.expanded.has(id)) })
  }
  return out
}

const CHECK = '<span class="check"><svg viewBox="0 0 24 24"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg></span>'
const CHEVRON = '<svg viewBox="0 0 24 24"><path d="m9 6 6 6-6 6"/></svg>'

function forumState(item) {
  const n = item.topics.reduce((s, t) => s + (state.selected.has(t.key) ? 1 : 0), 0)
  return n === 0 ? '' : n === item.topics.length ? 'selected' : 'partial'
}

function rowClass(key) {
  const { item, topic } = state.keys.get(key)
  if (topic) return `row topic${state.selected.has(key) ? ' selected' : ''}`
  if (item.topics.length) return `row ${forumState(item)}`
  return `row${state.selected.has(key) ? ' selected' : ''}`
}

function chatRow({ item, open }, q) {
  const c = item.chat
  const forum = c.topics.length > 0
  const badge = forum ? `<span class="badge">форум · ${count(c.topics.length, TOPICS)}</span>` : ''
  const dates = period(c.firstDate, c.lastDate)
  return `<div class="${rowClass(item.key)}${forum && open ? ' open' : ''}" data-key="${esc(item.key)}" role="option" tabindex="-1">
    ${forum ? `<button class="chev" data-chev="${esc(c.chatId)}" tabindex="-1" aria-label="Топики">${CHEVRON}</button>` : '<span></span>'}
    ${CHECK}
    <span class="avatar" style="--h:${hue(c.chatId)}">${esc(initials(c.name))}</span>
    <span class="main"><span class="name">${highlight(c.name, q)}${badge}</span><span class="sub">${esc(typeLabel(c.type))}${dates ? ` · ${dates}` : ''}</span></span>
    <span class="count"><b>${nf.format(c.count)}</b>${plural(c.count, ...MSGS)}</span>
  </div>`
}

function topicRow(topic, q) {
  const t = topic.t
  return `<div class="${rowClass(topic.key)}" data-key="${esc(topic.key)}" role="option" tabindex="-1">
    <span></span>
    ${CHECK}
    <span class="avatar">#</span>
    <span class="main"><span class="name">${highlight(t.title, q)}</span><span class="sub">${esc(period(t.firstDate, t.lastDate) || 'без дат')}</span></span>
    <span class="count"><b>${nf.format(t.count)}</b>${plural(t.count, ...MSGS)}</span>
  </div>`
}

function renderList() {
  const q = state.query
  state.visible = visibleItems()
  const html = []
  for (const v of state.visible) {
    html.push(chatRow(v, q))
    if (v.open) for (const t of v.topics) html.push(topicRow(t, q))
  }
  $('#list').innerHTML = html.join('')
  $('#list').hidden = state.visible.length === 0
  $('#list-empty').hidden = state.visible.length > 0
}

// Re-applies selection classes without rebuilding the list.
function syncRows() {
  for (const row of $('#list').children) {
    const open = row.classList.contains('open')
    row.className = rowClass(row.dataset.key) + (open ? ' open' : '')
  }
  renderSummary()
}

function renderFilters() {
  const counts = { all: state.items.length }
  for (const it of state.items) counts[it.cat] = (counts[it.cat] || 0) + 1
  $('#filters').innerHTML = FILTERS.filter(([k]) => k === 'all' || counts[k])
    .map(([k, label]) => `<button class="chip${state.filter === k ? ' active' : ''}" data-filter="${k}" type="button">${label} <span class="n">${nf.format(counts[k])}</span></button>`)
    .join('')
}

function selectKey(key, on) {
  const { item, topic } = state.keys.get(key)
  if (!on) { state.selected.delete(key); return }
  if (state.selected.has(key)) return
  state.selected.set(key, topic
    ? { chatId: item.chat.chatId, topicIds: [topic.t.topicId], label: `${item.chat.name} › ${topic.t.title}`, count: topic.t.count }
    : { chatId: item.chat.chatId, label: item.chat.name, count: item.chat.count })
}

function toggleKey(key) {
  const { item, topic } = state.keys.get(key)
  if (!topic && item.topics.length) {
    // A forum row toggles its visible topics and unfolds them.
    const v = state.visible.find((x) => x.item === item)
    const topics = v ? v.topics : item.topics
    const on = !topics.every((t) => state.selected.has(t.key))
    for (const t of topics) selectKey(t.key, on)
    if (!v?.open) { state.expanded.add(item.chat.chatId); renderList(); renderSummary(); return }
  } else {
    selectKey(key, !state.selected.has(key))
  }
  syncRows()
}

function toggleExpand(chatId) {
  if (state.expanded.has(chatId)) state.expanded.delete(chatId)
  else state.expanded.add(chatId)
  const focused = document.activeElement?.dataset?.key
  renderList()
  if (focused) focusRow(focused)
}

function renderSummary() {
  const n = state.selected.size
  const msgs = [...state.selected.values()].reduce((s, x) => s + x.count, 0)
  $('#sel-count').textContent = n ? `Выбрано: ${nf.format(n)}` : 'Ничего не выбрано'
  $('#sel-detail').textContent = n
    ? `${count(msgs, MSGS)} · по файлу на каждый чат и топик`
    : 'Отметьте чаты или топики, которые нужно сохранить'
  $('#btn-next').disabled = !n
  $('#btn-clear-selection').disabled = !n
}

function focusRow(key) {
  const row = [...$('#list').children].find((r) => r.dataset.key === key)
  row?.focus()
}

$('#list').addEventListener('click', (e) => {
  const chev = e.target.closest('[data-chev]')
  if (chev) { toggleExpand(chev.dataset.chev); return }
  const row = e.target.closest('.row')
  if (row) { toggleKey(row.dataset.key); row.focus() }
})

$('#list').addEventListener('keydown', (e) => {
  const row = e.target.closest('.row')
  if (!row) return
  const { item, topic } = state.keys.get(row.dataset.key)
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    const next = e.key === 'ArrowDown' ? row.nextElementSibling : row.previousElementSibling
    if (next) next.focus()
    else if (e.key === 'ArrowUp') $('#search').focus()
  } else if (e.key === ' ' || e.key === 'Enter') {
    e.preventDefault()
    toggleKey(row.dataset.key)
    focusRow(row.dataset.key)
  } else if (!topic && item.topics.length && (e.key === 'ArrowRight' || e.key === 'ArrowLeft')) {
    const open = row.classList.contains('open')
    if ((e.key === 'ArrowRight') !== open) toggleExpand(item.chat.chatId)
  }
})

let searchTimer
$('#search').addEventListener('input', (e) => {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(() => {
    state.query = e.target.value.trim().toLowerCase()
    renderList()
    $('#list').scrollTop = 0
  }, 60)
})

$('#search').addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && e.target.value) {
    e.target.value = ''
    state.query = ''
    renderList()
  } else if (e.key === 'ArrowDown') {
    e.preventDefault()
    $('#list').firstElementChild?.focus()
  }
})

$('#btn-clear-search').addEventListener('click', () => {
  $('#search').value = ''
  state.query = ''
  state.filter = 'all'
  renderFilters()
  renderList()
  $('#search').focus()
})

$('#filters').addEventListener('click', (e) => {
  const chip = e.target.closest('[data-filter]')
  if (!chip) return
  state.filter = chip.dataset.filter
  renderFilters()
  renderList()
  $('#list').scrollTop = 0
})

$('#sort').addEventListener('change', (e) => {
  state.sort = e.target.value
  renderList()
  $('#list').scrollTop = 0
})

$('#btn-select-visible').addEventListener('click', () => {
  for (const v of state.visible) {
    if (v.item.topics.length) for (const t of v.topics) selectKey(t.key, true)
    else selectKey(v.item.key, true)
  }
  syncRows()
})

$('#btn-clear-selection').addEventListener('click', () => {
  state.selected.clear()
  syncRows()
})

$('#btn-other-file').addEventListener('click', pickFile)
$('#btn-next').addEventListener('click', openSave)

// ---------- step 3: save ----------

function openSave() {
  if (!state.selected.size) return
  if (projects.isConnecting()) { projects.attachSelected(); return }
  renderSave()
  show('save')
}

function renderSave() {
  const sel = [...state.selected.entries()]
  const topics = sel.filter(([, s]) => s.topicIds).length
  const chats = sel.length - topics
  const msgs = sel.reduce((n, [, s]) => n + s.count, 0)
  $('#save-summary').textContent = [chats ? count(chats, CHATS) : '', topics ? count(topics, TOPICS) : '']
    .filter(Boolean).join(' и ') + ` · ${count(msgs, MSGS)}`
  $('#save-items').innerHTML = sel.map(([key, s]) =>
    `<li title="${esc(s.label)}"><span>${esc(s.label)}</span><button type="button" data-remove="${esc(key)}" aria-label="Убрать"><svg viewBox="0 0 24 24"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg></button></li>`).join('')
  // LRM marks keep the leading "/" in place in the left-truncating (rtl) box.
  $('#out-dir').textContent = `\u200e${state.outDir}\u200e`
  $('#out-dir').title = state.outDir
  $('#budgets').innerHTML = BUDGETS.map(([v, label, note]) =>
    `<button type="button" role="radio" data-budget="${v}" class="${v === state.budget ? 'active' : ''}" aria-checked="${v === state.budget}">${label}${note ? ` <small>· ${note}</small>` : ''}</button>`).join('')
}

$('#save-items').addEventListener('click', (e) => {
  const btn = e.target.closest('[data-remove]')
  if (!btn) return
  state.selected.delete(btn.dataset.remove)
  syncRows()
  if (state.selected.size) renderSave()
  else show('select')
})

$('#budgets').addEventListener('click', (e) => {
  const btn = e.target.closest('[data-budget]')
  if (!btn) return
  state.budget = Number(btn.dataset.budget)
  renderSave()
})

$('#btn-pick-dir').addEventListener('click', async () => {
  const dir = await invoke('pick_out_dir', { current: state.outDir })
  if (dir) {
    state.outDir = dir
    renderSave()
  }
})

$('#btn-back').addEventListener('click', () => show('select'))

$('#btn-export').addEventListener('click', async () => {
  if (state.job || !state.selected.size) return
  const selection = [...state.selected.values()].map(({ chatId, topicIds }) => (topicIds ? { chatId, topicIds } : { chatId }))
  startJob('extract', state.index.fileName)
  try {
    const res = await invoke('export_selection', {
      path: state.index.path,
      selection,
      outDir: state.outDir,
      maxTokens: state.budget || NO_SPLIT,
    })
    state.job = null
    showDone(res)
  } catch (e) {
    state.job = null
    show('save')
    if (isCancel(e)) toast('Сохранение отменено')
    else toast(errText(e), 'error')
  }
})

// ---------- done ----------

const FILE_ICON = '<svg viewBox="0 0 24 24"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/><path d="M9 13h6"/><path d="M9 17h4"/></svg>'

function showDone(res) {
  state.result = res
  const n = res.files.length
  $('#done-title').textContent = n ? 'Готово!' : 'Нечего сохранять'
  $('#done-sub').textContent = n
    ? `${count(n, FILES)} · ${formatBytes(res.files.reduce((s, f) => s + f.bytes, 0))} · ${res.outDir}`
    : 'В выбранных чатах нет обычных сообщений — только служебные.'
  $('#done-files').innerHTML = res.files.map((f, i) =>
    `<li data-file="${i}" title="Показать в папке">${FILE_ICON}<span class="fname">${esc(f.name)}</span><span class="fsize">${formatBytes(f.bytes)}</span></li>`).join('')
  $('#done-files').hidden = !n
  $('#btn-open-folder').hidden = !n
  show('done')
}

$('#done-files').addEventListener('click', (e) => {
  const li = e.target.closest('[data-file]')
  if (li) invoke('reveal_file', { path: state.result.files[Number(li.dataset.file)].path }).catch((err) => toast(errText(err), 'error'))
})
$('#btn-open-folder').addEventListener('click', () => {
  invoke('open_folder', { path: state.result.outDir }).catch((err) => toast(errText(err), 'error'))
})
$('#btn-more').addEventListener('click', () => show('select'))
$('#btn-new-file').addEventListener('click', pickFile)

// ---------- drag & drop, shortcuts ----------

const overlay = $('#drop-overlay')
listen('tauri://drag-enter', () => { if (!state.job) overlay.hidden = false })
listen('tauri://drag-leave', () => { overlay.hidden = true })
listen('tauri://drag-drop', ({ payload }) => {
  overlay.hidden = true
  const path = payload?.paths?.[0]
  if (path && !state.job) openExport(path)
})
// Never let the webview navigate to a dropped file.
window.addEventListener('dragover', (e) => e.preventDefault())
window.addEventListener('drop', (e) => e.preventDefault())

document.addEventListener('keydown', (e) => {
  const mod = IS_MAC ? e.metaKey : e.ctrlKey
  if (mod && e.code === 'KeyO') {
    e.preventDefault()
    pickFile()
  } else if (mod && e.code === 'KeyF' && state.screen === 'select') {
    e.preventDefault()
    $('#search').focus()
    $('#search').select()
  } else if (e.key === 'Escape' && state.screen === 'progress') {
    invoke('cancel_job')
  }
})

document.addEventListener('contextmenu', (e) => {
  if (!e.target.closest('input, .folder, .alert, .toast')) e.preventDefault()
})

// ---------- desktop theme (Omarchy) ----------

// theme.js applied the theme the app started with; follow later switches
// (Omarchy's theme menu) when the window regains focus and, on a themed
// desktop, every few seconds while visible.
let themeKey = JSON.stringify(window.__TGSUM_THEME__ || null)

async function syncTheme() {
  const theme = await invoke('desktop_theme').catch(() => null)
  const key = JSON.stringify(theme)
  if (key === themeKey) return
  themeKey = key
  window.tgsumApplyTheme(theme)
}

window.addEventListener('focus', syncTheme)
document.addEventListener('visibilitychange', () => { if (!document.hidden) syncTheme() })
if (window.__TGSUM_THEME__) setInterval(() => { if (!document.hidden) syncTheme() }, 3000)

// ---------- the night painting ----------

// The neon sign lights up letter by letter, like the tubes of a sign.
const REDUCED = matchMedia('(prefers-reduced-motion: reduce)').matches
for (const sign of document.querySelectorAll('svg.signature')) {
  if (REDUCED) sign.classList.add('is-lit')
  else requestAnimationFrame(() => sign.classList.add('is-lit'))
}

// Facts under the painting's stars on the first screen: three stars near the
// edges get a ring, their facts sit underneath (as on the author's site).
// With no room beside the text the facts stay in a row at the bottom.
const RING_COLORS = ['#f5c451', '#9fc3e4', '#e2663a']
let paintModel = null

function placeStarTags() {
  const list = $('.star-tags')
  const rings = $('.constellation')
  const tags = [...list.querySelectorAll('.star-tag')]
  list.classList.remove('is-placed')
  rings.classList.remove('is-placed')
  rings.replaceChildren()
  const painted = document.querySelector('.paint.is-done')
  document.body.style.setProperty('--wait', painted ? '0ms' : '2400ms')
  const m = paintModel
  if (!m || state.screen !== 'start' || m.W < 900) return

  const screen = $('#screen-start').getBoundingClientRect()
  const rects = [...document.querySelectorAll('.hero > :not([hidden])')].map((el) => el.getBoundingClientRect())
  const leftMax = Math.min(...rects.map((r) => r.left)) - 16
  const rightMin = Math.max(...rects.map((r) => r.right)) + 16
  const tw = Math.max(...tags.map((t) => t.offsetWidth))
  if (leftMax - 12 < tw || m.W - 12 - rightMin < tw) return
  const edge = m.orbs
    .filter((o) => o.kind === 'star' && o.y > screen.top + 28 && o.y < m.H * 0.7 && (o.x < leftMax || o.x > rightMin))
    .sort((a, b) => a.x - b.x)
  const n = tags.length
  if (edge.length < n) return
  const chosen = Array.from({ length: n }, (_, k) => edge[Math.round((k * (edge.length - 1)) / Math.max(1, n - 1))])

  rings.setAttribute('viewBox', `0 0 ${m.W} ${m.H}`)
  chosen.forEach((o, i) => {
    const ring = document.createElementNS('http://www.w3.org/2000/svg', 'circle')
    ring.setAttribute('cx', o.x)
    ring.setAttribute('cy', o.y)
    ring.setAttribute('r', o.halo * 0.62)
    ring.setAttribute('style', `--c:${RING_COLORS[i % RING_COLORS.length]};--k:${i}`)
    rings.append(ring)
  })
  list.classList.add('is-placed')
  rings.classList.add('is-placed')
  // Under its star, inside its side band; a fact that would cover another
  // one moves below it (in this window two stars can be close together).
  const boxes = chosen
    .map((o, i) => {
      const w = tags[i].offsetWidth
      const h = tags[i].offsetHeight
      const band = o.x < m.W / 2 ? [12, leftMax] : [rightMin, m.W - 12]
      const left = Math.min(Math.max(o.x - w / 2, band[0]), Math.max(band[0], band[1] - w))
      return { i, left, top: Math.max(o.y + o.halo * 0.8 + 10, screen.top + 14), w, h }
    })
    .sort((a, b) => a.top - b.top)
  boxes.forEach((b, k) => {
    for (const a of boxes.slice(0, k)) {
      const across = b.left < a.left + a.w + 8 && a.left < b.left + b.w + 8
      if (across && b.top < a.top + a.h + 10) b.top = a.top + a.h + 10
    }
    b.top = Math.min(b.top, m.H - b.h - 16)
    const tag = tags[b.i]
    tag.style.setProperty('--k', b.i)
    tag.style.left = `${b.left - screen.left}px`
    tag.style.top = `${b.top - screen.top}px`
  })
}

document.addEventListener('paint:model', (e) => {
  paintModel = e.detail
  placeStarTags()
})

mountPaintings()

// ---------- boot ----------

show('start')
invoke('initial_path').then((path) => { if (path) openExport(path) })
