import { existsSync } from 'node:fs'
import { resolve } from 'node:path'
import { intro, outro, text, note, autocompleteMultiselect, confirm, isCancel, cancel, spinner } from '@clack/prompts'
import { streamIndex } from './parse-index.js'
import { extractSelection } from './parse-extract.js'
import { writeUnits } from './write-output.js'
import { printLogo } from './logo.js'
import type { ChatIndex, Selection } from './types.js'

// Drag-and-drop in terminals often wraps the path in quotes — strip them.
const cleanPath = (s: string) => s.trim().replace(/^['"]|['"]$/g, '')

// One selectable option per whole non-forum chat, or per forum topic.
// Encode the Selection in the option value as a string key, decode after selection
// (clack compares option values, so a stable primitive key is safer than an object).
function toOptions(idx: ChatIndex[]) {
  const options: { value: string; label: string; hint?: string }[] = []
  const decode = new Map<string, Selection>()
  for (const c of idx) {
    if (c.topics.length) {
      for (const t of c.topics) {
        const key = `${c.chatId}::${t.topicId}`
        options.push({ value: key, label: `${c.name} › ${t.title}`, hint: `${t.count} msgs` })
        decode.set(key, { chatId: c.chatId, topicIds: [t.topicId] })
      }
    } else {
      options.push({ value: c.chatId, label: c.name, hint: `${c.count} msgs · ${c.type}` })
      decode.set(c.chatId, { chatId: c.chatId })
    }
  }
  return { options, decode }
}

function bail(): never { cancel('Отменено.'); process.exit(0) }

export async function runWizard(): Promise<void> {
  printLogo()
  intro('tgsum — выгрузка Telegram → файлы для ИИ')

  note(
    [
      'Telegram Desktop → ⚙ Settings → Advanced → Export Telegram data',
      'Формат: Machine-readable JSON (медиа можно не включать) → Export.',
      'В папке экспорта появится файл result.json — его путь и нужен ниже.',
    ].join('\n'),
    'Как получить result.json',
  )

  // Step 1/3: file
  const file = await text({
    message: 'Шаг 1/3 — перетащите result.json сюда или вставьте путь:',
    validate: (v) => (v && existsSync(cleanPath(v)) ? undefined : 'Файл не найден'),
  })
  if (isCancel(file)) bail()
  const path = cleanPath(file)

  const s = spinner()
  s.start('Читаю список чатов (на большой выгрузке это занимает время)')
  const idx: ChatIndex[] = await streamIndex(path)
  s.stop(`Найдено: ${idx.reduce((n, c) => n + (c.topics.length || 1), 0)} чатов/топиков`)

  // Step 2/3: searchable multi-select (filter-as-you-type is built into clack)
  const { options, decode } = toOptions(idx)
  const picked = await autocompleteMultiselect({
    message: 'Шаг 2/3 — печатайте для поиска, пробел — отметить, Enter — подтвердить:',
    options,
    placeholder: 'Поиск по названию…',
  })
  if (isCancel(picked)) bail()
  const selection = (picked as string[]).map((k) => decode.get(k)!).filter(Boolean)
  if (!selection.length) { outro('Ничего не выбрано.'); return }

  // Step 3/3: output dir + confirm
  const dir = await text({ message: 'Шаг 3/3 — папка для файлов:', placeholder: 'tgsum-output', defaultValue: 'tgsum-output' })
  if (isCancel(dir)) bail()
  const outDir = resolve(cleanPath(dir || 'tgsum-output'))
  const ok = await confirm({ message: `Записать ${selection.length} выбранных в ${outDir}?` })
  if (isCancel(ok) || !ok) bail()

  const s2 = spinner()
  s2.start('Извлекаю и форматирую')
  const units = await extractSelection(path, selection)
  const written = writeUnits(units, outDir)
  s2.stop(`Готово: ${written.length} файл(ов)`)
  outro(`Файлы в ${outDir}. Вставьте нужный в чат с ИИ вместе со скиллом-промптом.`)
}
