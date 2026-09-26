// Persistent Project scope editor. Archive parsing and filtering stay in Rust.
export function mountProjects({ invoke, show, pickFile, startJob, endJob, busy, selection, index, toast }) {
  const $ = (s) => document.querySelector(s)
  const esc = (v) => String(v ?? '').replace(/[&<>"']/g, (c) => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'})[c])
  let current = null
  let connecting = false
  let working = false
  let renderEpoch = 0
  let review = null
  let exportedDirectory = null
  const error = (e) => toast(e?.message || String(e), 'error')

  async function act(job) {
    if (working || busy()) return
    working = true
    $('#screen-projects').setAttribute('aria-busy', 'true')
    try { await job() } catch (e) {
      error(e)
      if (e?.kind === 'conflict' && current) current = await invoke('open_project', { projectId: current.project_id }).catch(() => current)
      if (!$('#screen-projects').hidden) await render().catch(error)
    } finally {
      working = false
      $('#screen-projects').setAttribute('aria-busy', 'false')
    }
  }

  async function update(change) {
    current = await invoke('update_project', { projectId: current.project_id, expectedRevision: current.revision, change })
  }

  async function open() {
    if (busy()) return
    connecting = false
    $('#project-attach-options').hidden = true
    $('#btn-next').textContent = 'Далее'
    show('projects')
    const entries = await invoke('list_projects')
    if (current) current = await invoke('open_project', { projectId: current.project_id })
    $('#project-list').innerHTML = entries.map((entry) => entry.state === 'ready'
      ? `<button type="button" class="btn btn-ghost" data-project="${esc(entry.project.project_id)}">${esc(entry.project.name)}</button>`
      : `<div class="alert">Проект недоступен: ${esc(entry.project_id)} — ${esc(entry.message)}</div>`).join('')
    $('#project-list-empty').hidden = entries.length > 0
    await render()
  }

  async function render() {
    const epoch = ++renderEpoch
    $('#project-detail').hidden = !current
    if (!current) return
    review = null
    exportedDirectory = null
    $('#project-review').hidden = true
    $('#project-unsaved').hidden = true
    $('#btn-project-review').disabled = !current.sources.some((s) => s.selection.enabled)
    $('#project-name').value = current.name
    $('#project-empty').hidden = current.sources.length > 0
    $('#project-sources').replaceChildren()
    const project = current
    for (const source of project.sources) {
      const card = document.createElement('form')
      card.className = 'card project-source'
      card.dataset.source = source.source_id
      $('#project-sources').append(card)
      let preview, problem
      try { preview = await invoke('preview_project_source', { projectId: project.project_id, sourceId: source.source_id }) }
      catch (e) { problem = e?.message || String(e) }
      if (epoch !== renderEpoch) return
      const filter = source.selection.filter
      const stats = preview?.stats
      const topics = preview?.topics || []
      card.innerHTML = `<h3>${esc(preview?.title || source.scope.conversation_id)}</h3>
        <p class="muted">Telegram · ${esc(source.scope.account_local_id)}</p>
        <label class="project-check"><input name="enabled" type="checkbox" ${source.selection.enabled ? 'checked' : ''}> Включать в анализ проекта</label>
        <label class="project-check"><input name="only_changes" type="checkbox" ${source.selection.only_changes ? 'checked' : ''}> Новые и изменённые после успешного анализа</label>
        <p class="hint">${preview?.baseline_analysis_id ? 'Есть сохранённая точка последнего успешного анализа.' : 'Первый анализ включает все выбранные сообщения. Обновление архива не отмечает сообщения как проанализированные.'}</p>
        <div class="project-fields">
          <label>С даты<input name="from" type="date" value="${esc(filter.dates?.from || '')}"></label>
          <label>По дату включительно<input name="through" type="date" value="${esc(filter.dates?.through || '')}"></label>
          <label>Как считать даты<select name="basis" class="select"><option value="source_date" ${filter.dates?.basis !== 'utc' ? 'selected' : ''}>Как указано в архиве</option><option value="utc" ${filter.dates?.basis === 'utc' ? 'selected' : ''}>По UTC</option></select></label>
        </div>
        <label class="project-check"><input name="include_unknown_dates" type="checkbox" ${filter.include_unknown_dates ? 'checked' : ''}> Включать сообщения с неопределённой датой</label>
        <label class="project-check"><input name="include_service" type="checkbox" ${filter.include_service ? 'checked' : ''}> Включать служебные события</label>
        <fieldset class="project-topics" ${topics.length ? '' : 'hidden'}><legend>Темы</legend>
          <label class="project-check"><input name="all_topics" type="checkbox" ${filter.topic_ids === null ? 'checked' : ''}> Все темы, включая новые</label>
          ${topics.map((topic) => `<label class="project-check"><input name="topic" type="checkbox" value="${esc(topic.id)}" ${filter.topic_ids?.includes(topic.id) ? 'checked' : ''}> ${esc(topic.title)} · ${topic.count}</label>`).join('')}
        </fieldset>
        ${stats ? `<p class="project-stats" role="status">Сообщений в контексте: <b>${stats.selected}</b> · новых ${stats.created} · изменённых ${stats.edited} · отсутствуют в новом архиве ${stats.missing}</p>
          <p class="hint">Без определённой даты: исключено ${stats.excluded_unknown_dates}, включено ${stats.included_unknown_dates}. Полнота архива: ${esc(preview.coverage.level === 'unknown' ? 'не подтверждена' : preview.coverage.level)}.</p>` : `<p class="alert">${esc(problem)}</p>`}
        <div class="project-actions"><button class="btn btn-primary" type="submit">Сохранить выбор</button>
          <button class="btn btn-ghost" type="button" data-refresh>Обновить из архива</button>
          <button class="btn btn-ghost" type="button" data-relink>Изменить файл…</button>
          <button class="btn btn-ghost" type="button" data-remove>Отключить</button></div>`
      const all = card.elements.all_topics
      const syncTopics = () => { for (const input of card.querySelectorAll('[name="topic"]')) input.disabled = all.checked }
      all.addEventListener('change', syncTopics)
      syncTopics()
    }
  }

  async function refresh(sourceId) {
    startJob('import', 'Обновление выбранного источника')
    try {
      current = await invoke('refresh_project_source', { projectId: current.project_id, sourceId, expectedRevision: current.revision })
    } finally { endJob(); show('projects') }
  }

  async function prepareReview() {
    review = null
    $('#btn-project-export').disabled = true
    $('#project-export-result').hidden = true
    $('#btn-project-open-export').hidden = true
    exportedDirectory = null
    startJob('bundle', 'Проверка и подготовка выбранного контекста')
    try {
      review = await invoke('prepare_project_bundle', { projectId: current.project_id, expectedRevision: current.revision,
        options: { redact_candidates: $('#project-redact-candidates').checked } })
    } finally { endJob(); show('projects') }
    const m = review.manifest
    $('#project-review-summary').textContent = `Источников: ${m.sources.length} · сообщений: ${m.messages} · вложений включено: ${m.included_attachments} · ссылок на невключённые вложения: ${m.attachment_references}`
    const coverage = { complete: 'полнота подтверждена для архивного диапазона', partial: 'неполная история', own_messages_only: 'только собственные сообщения', future_only: 'только новые события', unknown: 'полнота не подтверждена' }
    $('#project-review-sources').replaceChildren(...m.sources.map((source) => {
      const li = document.createElement('li')
      const dates = source.dates ? ` · ${source.dates.from || 'начало'} — ${source.dates.through || 'конец'} (${source.dates.basis === 'utc' ? 'UTC' : 'даты архива'})` : ''
      const topics = source.selected_topics === null ? 'все темы' : `тем выбрано: ${source.selected_topics}`
      const unknown = source.stats.included_unknown_dates ? ` · без определённой даты включено: ${source.stats.included_unknown_dates}` : ''
      li.textContent = `${source.title}: ${source.stats.selected} сообщений · ${topics}${source.only_changes ? ' · новые и изменённые' : ''}${dates}${unknown} · ${coverage[source.coverage]}${source.known_gaps ? ` · пропусков: ${source.known_gaps}` : ''}`
      return li
    }))
    $('#project-review-privacy').textContent = `Скрыто значений: ${m.privacy.redacted}. Требуют решения: ${m.privacy.needs_review}.${m.privacy.needs_review ? ' Включите скрытие подозрительных значений и обновите проверку.' : ''}`
    const fields = { project_title: 'Название проекта', source_title: 'Название источника', platform: 'Платформа', sender: 'Отправитель', timestamp: 'Дата', edited_at: 'Дата изменения', service_action: 'Событие', service_title: 'Название события', text: 'Сообщение' }
    $('#project-review-findings').replaceChildren(...review.findings.map((finding) => {
      const li = document.createElement('li')
      const label = document.createElement('strong')
      label.textContent = `${fields[finding.field] || finding.field} · ${finding.rule === 'jwt_candidate' ? 'похожее на JWT значение' : 'возможный ключ'}: `
      const excerpt = document.createElement('code')
      excerpt.textContent = finding.excerpt
      li.append(label, excerpt)
      return li
    }))
    $('#project-review-omitted').hidden = review.omitted_findings === 0
    $('#project-review-omitted').textContent = `Ещё находок: ${review.omitted_findings}. Скрытие подозрительных значений применяется ко всему выбранному контексту.`
    $('#project-review-preview').textContent = review.preview
    $('#project-review-truncated').hidden = !review.preview_truncated
    $('#project-review').hidden = false
    $('#btn-project-export').disabled = m.privacy.needs_review > 0
    $('#project-review').scrollIntoView({ block: 'start' })
  }

  $('#btn-project-review').addEventListener('click', () => act(prepareReview))
  $('#btn-project-review-again').addEventListener('click', () => act(prepareReview))
  $('#project-redact-candidates').addEventListener('change', () => {
    review = null
    $('#btn-project-export').disabled = true
    $('#project-review-privacy').textContent = 'Настройка изменена. Обновите проверку перед сохранением.'
  })
  $('#project-sources').addEventListener('input', () => {
    review = null
    $('#project-review').hidden = true
    $('#btn-project-review').disabled = true
    $('#project-unsaved').hidden = false
  })
  $('#btn-project-export').addEventListener('click', () => act(async () => {
    if (!review) return
    const sourcePath = current.sources.find((s) => s.selection.enabled && s.archive_path)?.archive_path
    const destination = await invoke('pick_out_dir', { current: exportedDirectory || sourcePath || index()?.outDir || null })
    if (!destination) return
    startJob('bundle', 'Сохранение подготовленного контекста')
    try {
      const result = await invoke('export_project_bundle', { projectId: current.project_id, bundleId: review.bundle_id,
        expectedRevision: review.project_revision, outDir: destination })
      exportedDirectory = result.directory
      $('#project-export-result').textContent = `Контекст сохранён: ${result.directory}`
      $('#project-export-result').hidden = false
      $('#btn-project-open-export').hidden = false
    } finally { endJob(); show('projects') }
  }))
  $('#btn-project-open-export').addEventListener('click', () => {
    if (exportedDirectory) invoke('open_folder', { path: exportedDirectory }).catch(error)
  })

  $('#btn-projects').addEventListener('click', () => act(open))
  $('#btn-projects-back').addEventListener('click', () => {
    if (working) return
    connecting = false
    $('#project-attach-options').hidden = true
    $('#btn-next').textContent = 'Далее'
    show('start')
  })
  $('#new-project-form').addEventListener('submit', (e) => {
    e.preventDefault()
    act(async () => {
      current = await invoke('create_project', { name: $('#new-project-name').value })
      $('#new-project-name').value = ''
      await open()
    })
  })
  $('#project-list').addEventListener('click', (e) => {
    const button = e.target.closest('[data-project]')
    if (button) act(async () => { current = await invoke('open_project', { projectId: button.dataset.project }); await render() })
  })
  $('#rename-project-form').addEventListener('submit', (e) => {
    e.preventDefault()
    act(async () => { await update({ kind: 'rename', value: $('#project-name').value }); await open() })
  })
  $('#btn-project-add-source').addEventListener('click', () => act(async () => {
    connecting = true
    $('#project-attach-options').hidden = false
    $('#project-attach-title').textContent = current.name
    $('#btn-next').textContent = 'Подключить к проекту'
    if (!await pickFile()) {
      connecting = false
      $('#project-attach-options').hidden = true
      $('#btn-next').textContent = 'Далее'
    }
  }))
  $('#project-sources').addEventListener('submit', (e) => {
    e.preventDefault()
    const form = e.target
    const f = form.elements
    act(async () => {
      await update({ kind: 'selection', value: { source_id: form.dataset.source, selection: {
        enabled: f.enabled.checked, only_changes: f.only_changes.checked,
        filter: { topic_ids: f.all_topics.checked ? null : [...form.querySelectorAll('[name="topic"]:checked')].map((input) => input.value),
          dates: f.from.value || f.through.value ? { from: f.from.value || null, through: f.through.value || null, basis: f.basis.value } : null,
          include_unknown_dates: f.include_unknown_dates.checked, include_service: f.include_service.checked }
      } } })
      await render()
      toast('Выбор сохранён')
    })
  })
  $('#project-sources').addEventListener('click', (e) => {
    const form = e.target.closest('[data-source]')
    if (!form) return
    const id = form.dataset.source
    if (e.target.closest('[data-refresh]')) act(async () => { await refresh(id); await render() })
    if (e.target.closest('[data-remove]')) act(async () => { await update({ kind: 'remove_source', value: id }); await render() })
    if (e.target.closest('[data-relink]')) act(async () => {
      const path = await invoke('pick_export')
      if (!path) return
      const source = current.sources.find((s) => s.source_id === id)
      await update({ kind: 'source', value: { ...source, archive_path: path } })
      await refresh(id)
      await render()
    })
  })

  return {
    isConnecting: () => connecting,
    async attachSelected() {
      await act(async () => {
        const account = $('#project-account-label').value.trim()
        if (!account) throw new Error('Укажите локальную метку аккаунта')
        const chats = new Map()
        for (const item of selection()) {
          if (!chats.has(item.chatId)) chats.set(item.chatId, item.topicIds ? [...item.topicIds] : null)
          else if (!item.topicIds) chats.set(item.chatId, null)
          else if (chats.get(item.chatId)) chats.get(item.chatId).push(...item.topicIds)
        }
        for (const [chatId, topicIds] of chats) {
          const existing = current.sources.find((s) => s.scope.platform === 'telegram' && s.scope.account_local_id === account && s.scope.conversation_id === chatId)
          const sourceId = existing?.source_id || `source-${crypto.randomUUID()}`
          const source = { source_id: sourceId, connector_id: 'telegram_json',
            scope: { platform: 'telegram', account_local_id: account, conversation_id: chatId },
            archive_path: index().path, latest_snapshot_id: existing?.latest_snapshot_id || null,
            selection: { enabled: true, only_changes: existing?.selection.only_changes || false,
              filter: { ...(existing?.selection.filter || {}), topic_ids: topicIds } } }
          await update({ kind: 'source', value: source })
          await refresh(sourceId)
        }
        await open()
      })
    },
  }
}
