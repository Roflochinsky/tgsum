// Private local policy and explicit before/after review. Original data never
// joins the public bundle or an analysis request in this controller.
const pii = { participants: 'Имена участников', emails: 'Email', phones: 'Телефоны', usernames: 'Упоминания @username' }
const infrastructure = { ip: 'IP-адреса', host: 'Серверы', domain: 'Внутренние домены', url: 'Внутренние URL', username: 'Имена пользователей серверов', path: 'Пути к файлам', cloud_resource: 'Облачные ресурсы' }
const key = (choice) => JSON.stringify([choice.message_id, choice.position])
const refKey = (ref) => `${ref.id}@${ref.revision}`

export function mountPrivacy({ invoke, act, project, update, invalidate, navigate, startJob, endJob, show }) {
  const $ = (s) => document.querySelector(s)
  let presets = null
  let policyDirty = false
  let loadedProject = null
  let bundle = null
  let page = null
  const fileForms = new WeakMap()
  const references = new Map()
  const form = $('#privacy-settings')
  for (const [selector, values, group] of [['#privacy-pii', pii, 'pii'], ['#privacy-infrastructure', infrastructure, 'infrastructure']]) {
    for (const [value, title] of Object.entries(values)) {
      const label = document.createElement('label'); label.className = 'project-check'
      const input = document.createElement('input'); input.type = 'checkbox'; input.value = value; input.dataset.privacyGroup = group
      label.append(input, ` ${title}`); $(selector).append(label)
    }
  }
  const lines = (selector) => $(selector).value.split(/\r?\n/).map((v) => v.trim()).filter(Boolean)
  const values = (group) => [...form.querySelectorAll(`[data-privacy-group="${group}"]:checked`)].map((e) => e.value)
  function categories(options) {
    for (const input of form.querySelectorAll('[data-privacy-group]')) input.checked = (options[input.dataset.privacyGroup]?.categories || []).includes(input.value)
  }
  function dirty() {
    policyDirty = true
    $('#privacy-saved-state').textContent = 'Настройки сохранятся при подготовке файлов. После изменений нужно заново проверить результат.'
    invalidate()
  }
  form.addEventListener('input', (event) => {
    if (event.target.dataset.privacyGroup) $('#privacy-preset').value = 'custom'
    dirty()
  })
  $('#privacy-preset').addEventListener('change', () => {
    const preset = presets?.find((p) => p.id === $('#privacy-preset').value)
    if (preset && preset.id !== 'custom') categories(preset.options)
    dirty()
  })
  form.addEventListener('submit', (event) => { event.preventDefault(); act(persist) })
  $('#btn-privacy-settings').addEventListener('click', () => {
    navigate('source')
    $('#project-privacy-settings').open = true
    form.scrollIntoView({ block: 'start' })
  })

  async function load({ discard = false } = {}) {
    presets ||= await invoke('privacy_presets')
    const current = project()
    // A source save/refresh rerenders its cards. Keep a policy draft until an
    // explicit reload or Project change, including after a validation error.
    if (!discard && policyDirty && loadedProject === current.project_id) return
    loadedProject = current.project_id
    const options = current.privacy_options || presets.find((p) => p.id === 'secrets').options
    $('#privacy-preset').value = presets.some((p) => p.id === current.settings.privacy_preset) ? current.settings.privacy_preset : 'custom'
    categories(options)
    $('#project-redact-candidates').checked = !!options.redact_candidates
    $('#privacy-domains').value = (options.infrastructure?.internal_domains || []).join('\n')
    $('#privacy-hosts').value = (options.infrastructure?.hostnames || []).join('\n')
    $('#privacy-exceptions').value = (options.keep_values || []).join('\n')
    const terms = current.custom_terms?.entries || []
    $('#privacy-terms').value = terms.filter((t) => t.boundary !== 'substring').map((t) => t.value).join('\n')
    $('#privacy-substrings').value = terms.filter((t) => t.boundary === 'substring').map((t) => t.value).join('\n')
    policyDirty = false
    $('#privacy-saved-state').textContent = 'Настройки сохранены в проекте.'
    clearReview()
  }
  async function persist() {
    if (!policyDirty) return
    const options = { redact_candidates: $('#project-redact-candidates').checked,
      pii: { categories: values('pii') }, infrastructure: { categories: values('infrastructure'), internal_domains: lines('#privacy-domains'), hostnames: lines('#privacy-hosts') },
      keep_values: lines('#privacy-exceptions') }
    try {
      await update({ kind: 'privacy', value: { preset: $('#privacy-preset').value, options, custom_terms: { entries: [
        ...lines('#privacy-terms').map((value) => ({ value, boundary: 'word' })), ...lines('#privacy-substrings').map((value) => ({ value, boundary: 'substring' }))] } } })
    } catch (error) {
      if (String(error).includes('invalid privacy profile')) throw new Error('Проверьте словарь и исключения: значения должны быть уникальными, без управляющих символов и в пределах допустимого размера.')
      throw error
    }
    policyDirty = false
    $('#privacy-saved-state').textContent = 'Настройки сохранены в проекте.'
  }

  async function sourceFiles(card, source) {
    const section = document.createElement('details'); section.className = 'attachment-selector'
    section.innerHTML = '<summary>Текстовые вложения</summary><p class="hint">Только файлы выбранных сообщений. Выберите папку экспорта и отметьте нужные вложения, затем нажмите «Сохранить выбор» в карточке чата.</p><label class="attachment-root">Папка экспорта<input data-attachment-root readonly></label><div class="project-actions"><button type="button" class="btn btn-ghost" data-attachment-folder>Выбрать папку…</button><button type="button" class="btn btn-ghost" data-attachment-clear>Исключить все файлы</button></div><p class="hint" data-attachment-selected></p><div class="attachment-items"></div><div class="project-actions"><button type="button" class="btn btn-ghost" data-attachment-prev>Предыдущие</button><span class="hint" data-attachment-page></span><button type="button" class="btn btn-ghost" data-attachment-next>Следующие</button></div>'
    card.querySelector('.project-actions').before(section)
    const state = { root: source.selection.attachments?.root || '', selected: new Map((source.selection.attachments?.files || []).map((c) => [key(c), c])), page: null }
    fileForms.set(card, state)
    const s = (selector) => section.querySelector(selector)
    const changed = () => { count(); card.dispatchEvent(new Event('input', { bubbles: true })) }
    function count() {
      s('[data-attachment-root]').value = state.root
      s('[data-attachment-selected]').textContent = `Выбрано файлов: ${state.selected.size} из не более 100 на проект.${state.selected.size && !state.root ? ' Укажите папку экспорта.' : ''}`
    }
    async function renderPage(offset) {
      const current = project()
      state.page = await invoke('attachment_catalog', { projectId: current.project_id, expectedRevision: current.revision, sourceId: source.source_id, offset })
      const rows = state.page.items.map((candidate) => {
        const label = document.createElement('label'); label.className = 'project-check'
        const input = document.createElement('input'); input.type = 'checkbox'; input.dataset.attachmentChoice = key(candidate.choice)
        input.checked = JSON.stringify(state.selected.get(key(candidate.choice))) === JSON.stringify(candidate.choice)
        input.disabled = !candidate.eligible && !input.checked
        const metadata = candidate.choice.expected
        const name = metadata.original_name || metadata.relative_path || 'Файл не включён в экспорт'
        const stale = state.selected.has(key(candidate.choice)) && !input.checked
        label.append(input, ` ${name} · сообщение ${candidate.choice.message_id}${metadata.size == null ? '' : ` · размер из архива ${metadata.size} Б`}${!candidate.eligible ? ' · не поддерживается как локальный текст' : ''}${stale ? ' · сохранённая ссылка изменилась, выберите заново' : ''}`)
        input.addEventListener('change', () => { if (input.checked) state.selected.set(key(candidate.choice), candidate.choice); else state.selected.delete(key(candidate.choice)); changed() })
        return label
      })
      s('.attachment-items').replaceChildren(...rows)
      s('[data-attachment-page]').textContent = state.page.total ? `${offset + 1}–${offset + rows.length} из ${state.page.total}` : 'Нет вложений в сохранённом выборе сообщений'
      s('[data-attachment-prev]').disabled = offset === 0
      s('[data-attachment-next]').disabled = state.page.next_offset === null
      count()
    }
    s('[data-attachment-folder]').addEventListener('click', () => act(async () => {
      const root = await invoke('pick_attachment_root', { current: state.root || source.archive_path || null })
      if (root) { state.root = root; changed() }
    }))
    s('[data-attachment-clear]').addEventListener('click', () => { state.selected.clear(); changed(); for (const input of s('.attachment-items').querySelectorAll('input')) input.checked = false })
    s('[data-attachment-prev]').addEventListener('click', () => act(() => renderPage(Math.max(0, state.page.offset - 50))))
    s('[data-attachment-next]').addEventListener('click', () => act(() => renderPage(state.page.next_offset)))
    try { await renderPage(0) } catch (e) { s('[data-attachment-page]').textContent = e?.message || String(e); s('[data-attachment-prev]').disabled = true; s('[data-attachment-next]').disabled = true; count() }
  }
  function selection(card) {
    const state = fileForms.get(card)
    if (!state || !state.selected.size) return null
    if (!state.root) throw new Error('Укажите папку экспорта для выбранных вложений')
    return { root: state.root, files: [...state.selected.values()] }
  }

  function clearReview() {
    bundle = null; page = null; references.clear()
    $('#privacy-comparison').hidden = true
    $('#privacy-comparison-panes').hidden = true
    $('#privacy-before').textContent = ''; $('#privacy-after').textContent = ''
  }
  async function reviewPage(offset) {
    if (!bundle) return
    page = await invoke('review_items', { projectId: project().project_id, bundleId: bundle.bundle_id, expectedRevision: bundle.project_revision, offset })
    references.clear()
    const items = [...page.items]
    for (const attachment of bundle.manifest.attachments || []) {
      if (attachment.evidence && !items.some((i) => refKey(i.reference) === refKey(attachment.evidence))) items.push({ reference: attachment.evidence, kind: 'attachment', label: attachment.file })
    }
    const options = items.map((item) => {
      const option = document.createElement('option'); option.value = refKey(item.reference); references.set(option.value, item.reference)
      const source = bundle.manifest.sources.find((s) => s.id === item.source_id)?.title
      option.textContent = `${item.kind === 'attachment' ? 'Файл' : 'Сообщение'}: ${item.label}${source ? ` · ${source}` : ''}`
      return option
    })
    $('#privacy-review-item').replaceChildren(...options)
    $('#privacy-review-page').textContent = page.total ? `${offset + 1}–${offset + page.items.length} из ${page.total}` : 'Нет сообщений для сравнения'
    $('#privacy-page-prev').disabled = offset === 0
    $('#privacy-page-next').disabled = page.next_offset === null
    $('#privacy-compare').disabled = !options.length
    $('#privacy-comparison-panes').hidden = true
    $('#privacy-comparison-status').textContent = ''
  }
  async function showReview(review) {
    bundle = review
    const manifest = document.createElement('li'); manifest.textContent = 'manifest.json · список файлов и сведения об исходных чатах'
    $('#privacy-reviewed-files').replaceChildren(manifest, ...review.manifest.files.map((file) => { const li = document.createElement('li'); li.textContent = `${file.name} · ${file.bytes.toLocaleString('ru-RU')} Б`; return li }))
    const missing = (review.manifest.attachments || []).filter((a) => a.status === 'missing').length
    $('#privacy-file-gaps').textContent = `Отсутствующих выбранных файлов: ${missing}. Выбрано файлов из сообщений вне заданного периода или тем: ${review.manifest.attachment_choices_outside_scope || 0}.`
    $('#privacy-comparison').hidden = false
    await reviewPage(0)
  }
  function summary(manifest) {
    const m = manifest
    const counts = (report, labels) => report ? [...report.categories].map((category) => `${labels[category]}: ${report.by_category[category] || 0}`).join(', ') : 'выключено'
    $('#project-review-privacy').textContent = `Секреты: скрыто ${m.privacy.redacted}. Требуют решения: ${m.privacy.needs_review}. Личные данные: ${m.pii?.replacements || 0} (${counts(m.pii, pii)}). Служебные адреса и пути: ${m.infrastructure?.replacements || 0} (${counts(m.infrastructure, infrastructure)}). Словарь: ${m.custom_terms?.replacements || 0}. Это число замен, включая повторы.${m.privacy.needs_review ? ' В настройках включите скрытие подозрительных значений и обновите проверку.' : ''}`
  }
  $('#privacy-page-prev').addEventListener('click', () => act(() => reviewPage(Math.max(0, page.offset - 50))))
  $('#privacy-page-next').addEventListener('click', () => act(() => reviewPage(page.next_offset)))
  $('#privacy-review-item').addEventListener('change', () => { $('#privacy-comparison-panes').hidden = true; $('#privacy-before').textContent = ''; $('#privacy-after').textContent = '' })
  $('#privacy-compare').addEventListener('click', () => act(async () => {
    const reference = references.get($('#privacy-review-item').value)
    if (!bundle || !reference) return
    startJob('privacy_preview', 'Чтение выбранного фрагмента для сравнения')
    let result
    try { result = await invoke('preview_evidence', { projectId: project().project_id, bundleId: bundle.bundle_id, expectedRevision: bundle.project_revision, reference }) }
    finally { endJob(); show('projects') }
    $('#privacy-before').textContent = result.before ?? (result.before_state === 'file_changed' ? 'Исходный файл изменился после подготовки. Оригинал не показывается.' : 'Исходный файл недоступен. Сохранённый результат очистки доступен справа.')
    $('#privacy-after').textContent = result.after
    $('#privacy-comparison-status').textContent = `${result.before_truncated || result.after_truncated ? 'Показано начало: не более 24 КиБ в каждой панели. ' : ''}${result.before_state === 'verified_file' ? 'Оригинал файла совпадает с версией, использованной при подготовке.' : result.before_state === 'snapshot' ? 'Оригинал сообщения взят из сохранённой копии чата.' : ''}`
    $('#privacy-comparison-panes').hidden = false
  }))
  return { load, persist, sourceFiles, selection, summary, showReview, clearReview }
}
