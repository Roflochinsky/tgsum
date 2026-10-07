// Persistent Project scope editor. Archive parsing and filtering stay in Rust.
import { mountAnalysis } from './analysis.js'
import { mountPrivacy } from './privacy.js'
import { mountAssisted } from './assisted.js'
import { mountTelegramRefresh } from './telegram-refresh.js'
import { mountLocalPackage } from './local-package.js'
import { recentProjects, rememberProject } from './onboarding.js'
import { renderSourceAccess } from './source-access.js'

export function mountProjects({ invoke, show, pickFile, startJob, endJob, busy, selection, index, toast }) {
  const $ = (s) => document.querySelector(s)
  const esc = (v) => String(v ?? '').replace(/[&<>"']/g, (c) => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'})[c])
  let current = null
  let connecting = false
  let working = false
  let renderEpoch = 0
  let review = null
  let exportedDirectory = null
  let step = 'source'
  let privacyVisible = false
  const error = (e) => toast(e?.message || String(e), 'error')
  const analysis = mountAnalysis({ invoke, act, project: () => current, bundle: () => review,
    reload: async () => { current = await invoke('open_project', { projectId: current.project_id }); await render() },
    startJob, endJob, show, toast, navigate, changed: syncSteps })
  const privacy = mountPrivacy({ invoke, act, project: () => current, update,
    invalidate: invalidateReview, navigate, startJob, endJob, show })
  const assisted = mountAssisted({ invoke, act, project: () => current, update,
    invalidate: invalidateReview, imported: async (updated) => { current = updated; await render() },
    startJob, endJob, show, toast })
  const telegramRefresh = mountTelegramRefresh({ invoke, act, project: () => current, toast,
    changed: async (updated, sourceId) => {
      current = updated
      invalidateReview()
      await render()
      const panel = document.querySelector(`[data-telegram-refresh="${CSS.escape(sourceId)}"]`)
      if (panel) panel.open = true
    } })
  const localPackage = mountLocalPackage({ invoke, act, project: () => current, toast,
    reload: async () => { current = await invoke('open_project', { projectId: current.project_id }); await render() } })

  function invalidateReview() {
    review = null
    privacyVisible = false
    privacy.clearReview()
    analysis.invalidate()
    analysis.availability()
    syncSteps()
  }

  function syncSteps() {
    const enabled = { source: !!current, privacy: privacyVisible,
      analyze: !!review && review.manifest.privacy.needs_review === 0,
      review: analysis.hasReview(), result: !!current }
    if (!enabled[step]) step = enabled.analyze ? 'analyze' : 'source'
    for (const button of $('#project-steps').querySelectorAll('[data-go]')) {
      button.disabled = !enabled[button.dataset.go]
      if (button.dataset.go === step) button.setAttribute('aria-current', 'step')
      else button.removeAttribute('aria-current')
    }
    for (const panel of document.querySelectorAll('[data-project-panel]')) panel.hidden = panel.dataset.projectPanel !== step
    $('#btn-project-destination').disabled = !enabled.analyze
    $('#btn-project-export').disabled = !enabled.analyze
  }
  function navigate(next) {
    step = next
    syncSteps()
    $('#project-steps').scrollIntoView({ block: 'start' })
  }

  async function act(job) {
    if (working || busy()) return
    working = true
    $('#screen-projects').setAttribute('aria-busy', 'true')
    $('#screen-projects').inert = true
    try { await job() } catch (e) {
      error(e)
      // Keep draft scope/model/options on failure. A conflict requires an
      // explicit reload, which is the only error path that discards form edits.
      if (e?.kind === 'conflict' && current) {
        invalidateReview()
        $('#project-conflict').hidden = false
      }
    } finally {
      working = false
      $('#screen-projects').setAttribute('aria-busy', 'false')
      $('#screen-projects').inert = false
      syncSteps()
    }
  }

  async function update(change) {
    current = await invoke('update_project', { projectId: current.project_id, expectedRevision: current.revision, change })
  }

  function cancelConnecting() {
    connecting = false
    $('#project-attach-options').hidden = true
    $('#btn-next').textContent = 'Далее'
  }

  async function open() {
    if (busy()) return
    cancelConnecting()
    show('projects')
    const entries = await invoke('list_projects')
    const recent = recentProjects()
    const rank = (entry) => { const i = recent.indexOf(entry.project?.project_id); return i < 0 ? Infinity : i }
    entries.sort((a, b) => rank(a) - rank(b))
    if (current) current = await invoke('open_project', { projectId: current.project_id })
    $('#project-list').innerHTML = entries.map((entry) => entry.state === 'ready'
      ? `<button type="button" class="btn btn-ghost" data-project="${esc(entry.project.project_id)}">${esc(entry.project.name)}</button>`
      : `<div class="alert">Проект недоступен: ${esc(entry.project_id)} — ${esc(entry.message)}</div>`).join('')
    $('#project-list-empty').hidden = entries.length > 0
    await render({ discardPrivacy: true })
  }

  async function render({ discardPrivacy = false, preserveSourceDrafts = false } = {}) {
    const epoch = ++renderEpoch
    // Keep unsaved cards alive, including their attachment choices and timers,
    // while a different chat is saved. Rebuilding them discards the user's draft.
    const drafts = new Map(preserveSourceDrafts
      ? [...$('#project-sources').querySelectorAll('[data-source][data-dirty="true"]')]
        .filter(card => current?.sources.some(source => source.source_id === card.dataset.source))
        .map(card => [card.dataset.source, card])
      : [])
    $('#project-detail').hidden = !current
    $('#project-home').hidden = !!current
    $('#projects-title').textContent = current?.name || 'Проекты'
    $('#project-conflict').hidden = true
    step = 'source'
    privacyVisible = false
    review = null
    privacy.clearReview()
    exportedDirectory = null
    $('#project-export-result').hidden = true
    $('#project-export-result').textContent = ''
    $('#btn-project-open-export').hidden = true
    await analysis.invalidate()
    if (!current) return
    await privacy.load({ discard: discardPrivacy })
    rememberProject(current.project_id)
    $('#project-review').hidden = true
    $('#project-unsaved').hidden = drafts.size === 0
    $('#btn-project-review').disabled = drafts.size > 0 || !current.sources.some((s) => s.selection.enabled)
    $('#project-name').value = current.name
    $('#project-empty').hidden = current.sources.length > 0
    const access = await invoke('project_source_accesses', { projectId: current.project_id, expectedRevision: current.revision })
    if (epoch !== renderEpoch) return
    $('#project-sources').replaceChildren(...drafts.values())
    const project = current
    for (const source of project.sources) {
      if (drafts.has(source.source_id)) {
        $('#project-sources').append(drafts.get(source.source_id))
        continue
      }
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
        <p class="muted">${esc(source.scope.platform)} · ${esc(source.scope.account_local_id)}</p>
        <div class="source-access"></div>
        <label class="project-check"><input name="enabled" type="checkbox" ${source.selection.enabled ? 'checked' : ''}> Включать в результат</label>
        <label class="project-check"><input name="only_changes" type="checkbox" ${source.selection.only_changes ? 'checked' : ''}> Только новые и изменённые после последнего анализа</label>
        <p class="hint">${preview?.baseline_analysis_id ? 'Режим «Только новые и изменённые» сравнивает сообщения с последним успешным анализом.' : 'Пока анализов нет, выбираются все подходящие сообщения. Простое обновление файла не отмечает их как проанализированные.'}</p>
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
        ${stats ? `<p class="project-stats" role="status">Выбрано сообщений: <b>${stats.selected}</b> · новых ${stats.created} · изменённых ${stats.edited} · отсутствуют в новом архиве ${stats.missing}</p>
          <p class="hint">Без определённой даты: исключено ${stats.excluded_unknown_dates}, включено ${stats.included_unknown_dates}. Полнота архива: ${esc(preview.coverage.level === 'unknown' ? 'не подтверждена' : preview.coverage.level)}.</p>` : `<p class="alert">${esc(problem)}</p>`}
        <div class="project-actions"><button class="btn btn-primary" type="submit">Сохранить выбор</button>
          <button class="btn btn-ghost" type="button" data-refresh>Перечитать архив</button>
          <button class="btn btn-ghost" type="button" data-relink>Изменить файл…</button>
          <button class="btn btn-ghost" type="button" data-remove>Отключить</button></div>`
      const facts = access.sources.find((s) => s.source_id === source.source_id)
      renderSourceAccess(card.querySelector('.source-access'), facts, preview?.coverage)
      card.querySelector('[data-refresh]').disabled = !facts?.method
      card.querySelector('[data-relink]').disabled = !facts?.method
      const all = card.elements.all_topics
      const syncTopics = () => { for (const input of card.querySelectorAll('[name="topic"]')) input.disabled = all.checked }
      all.addEventListener('change', syncTopics)
      syncTopics()
      await privacy.sourceFiles(card, source)
      assisted.source(card, source, preview?.title || source.scope.conversation_id)
      telegramRefresh.source(card, source)
      if (epoch !== renderEpoch) return
    }
    await localPackage.render($('#project-local-package'))
    if (epoch !== renderEpoch) return
    await analysis.reset()
    syncSteps()
  }

  async function refresh(sourceId) {
    startJob('import', 'Чтение обновлённого файла чата')
    try {
      current = await invoke('refresh_project_source', { projectId: current.project_id, sourceId, expectedRevision: current.revision })
    } finally { endJob(); show('projects') }
  }

  async function prepareReview() {
    if (!$('#project-unsaved').hidden) throw new Error('Сначала сохраните выбор сообщений во всех изменённых чатах.')
    invalidateReview()
    await privacy.persist()
    $('#btn-project-export').disabled = true
    $('#project-export-result').hidden = true
    $('#btn-project-open-export').hidden = true
    exportedDirectory = null
    startJob('bundle', 'Подготовка выбранных сообщений и вложений')
    let prepared
    try {
      prepared = await invoke('prepare_project_bundle', { projectId: current.project_id, expectedRevision: current.revision })
      // Publishing pseudonym mappings may advance the Project revision.
      current = await invoke('open_project', { projectId: current.project_id })
    } finally { endJob(); show('projects') }
    const m = prepared.manifest
    $('#project-review-summary').textContent = `Чатов: ${m.sources.length} · сообщений: ${m.messages} · вложений включено: ${m.included_attachments} · ссылок на невключённые вложения: ${m.attachment_references - m.included_attachments}`
    const coverage = { complete: 'полнота подтверждена для архивного диапазона', partial: 'неполная история', own_messages_only: 'только собственные сообщения', future_only: 'только новые события', unknown: 'полнота не подтверждена' }
    $('#project-review-sources').replaceChildren(...m.sources.map((source) => {
      const li = document.createElement('li')
      const dates = source.dates ? ` · ${source.dates.from || 'начало'} — ${source.dates.through || 'конец'} (${source.dates.basis === 'utc' ? 'UTC' : 'даты архива'})` : ''
      const topics = source.selected_topics === null ? 'все темы' : `тем выбрано: ${source.selected_topics}`
      const unknown = source.stats.included_unknown_dates ? ` · без определённой даты включено: ${source.stats.included_unknown_dates}` : ''
      li.textContent = `${source.title}: ${source.stats.selected} сообщений · ${topics}${source.only_changes ? ' · новые и изменённые' : ''}${dates}${unknown} · ${coverage[source.coverage]}${source.known_gaps ? ` · пропусков: ${source.known_gaps}` : ''}`
      return li
    }))
    privacy.summary(m)
    const fields = { project_title: 'Название проекта', source_title: 'Название чата', platform: 'Платформа', sender: 'Отправитель', timestamp: 'Дата', edited_at: 'Дата изменения', service_action: 'Событие', service_title: 'Название события', text: 'Сообщение', attachment_text: 'Текст вложения' }
    $('#project-review-findings').replaceChildren(...prepared.findings.map((finding) => {
      const li = document.createElement('li')
      const label = document.createElement('strong')
      label.textContent = `${fields[finding.field] || finding.field} · ${finding.rule === 'jwt_candidate' ? 'похожее на JWT значение' : 'возможный ключ'}: `
      const excerpt = document.createElement('code')
      excerpt.textContent = finding.excerpt
      li.append(label, excerpt)
      return li
    }))
    $('#project-review-omitted').hidden = prepared.omitted_findings === 0
    $('#project-review-omitted').textContent = `Ещё находок: ${prepared.omitted_findings}. Скрытие подозрительных значений применяется ко всем выбранным сообщениям и вложениям.`
    $('#project-review-preview').textContent = prepared.preview
    $('#project-review-truncated').hidden = !prepared.preview_truncated
    await privacy.showReview(prepared)
    // Enable recipient selection only after the complete local Review rendered.
    review = prepared
    $('#project-review').hidden = false
    privacyVisible = true
    $('#btn-project-export').disabled = m.privacy.needs_review > 0
    analysis.availability()
    navigate('privacy')
    $('#project-review').scrollIntoView({ block: 'start' })
  }

  $('#btn-project-review').addEventListener('click', () => act(prepareReview))
  $('#btn-project-review-again').addEventListener('click', () => act(prepareReview))
  $('#project-sources').addEventListener('input', (event) => {
    const card = event.target.closest('[data-source]')
    if (card) card.dataset.dirty = 'true'
    invalidateReview()
    $('#btn-project-review').disabled = true
    $('#project-unsaved').hidden = false
  })
  $('#btn-project-export').addEventListener('click', () => act(async () => {
    if (!review) return
    const sourcePath = current.sources.find((s) => s.selection.enabled && s.archive_path)?.archive_path
    const destination = await invoke('pick_out_dir', { current: exportedDirectory || sourcePath || index()?.outDir || null })
    if (!destination) return
    startJob('bundle', 'Сохранение подготовленных файлов')
    try {
      const result = await invoke('export_project_bundle', { projectId: current.project_id, bundleId: review.bundle_id,
        expectedRevision: review.project_revision, outDir: destination })
      exportedDirectory = result.directory
      $('#project-export-result').textContent = `Файлы сохранены: ${result.directory}`
      $('#project-export-result').hidden = false
      $('#btn-project-open-export').hidden = false
      navigate('result')
    } finally { endJob(); show('projects') }
  }))
  $('#btn-project-open-export').addEventListener('click', () => {
    if (exportedDirectory) invoke('open_folder', { path: exportedDirectory }).catch(error)
  })

  $('#btn-projects').addEventListener('click', () => act(async () => { current = null; await open() }))
  $('#btn-project-reload').addEventListener('click', () => act(open))
  $('#project-steps').addEventListener('click', (e) => {
    const button = e.target.closest('[data-go]')
    if (button && !button.disabled && !working) navigate(button.dataset.go)
  })
  $('#btn-project-destination').addEventListener('click', () => navigate('analyze'))
  $('#btn-projects-back').addEventListener('click', () => {
    if (working) return
    cancelConnecting()
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
    if (button) act(async () => { current = await invoke('open_project', { projectId: button.dataset.project }); await render({ discardPrivacy: true }) })
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
    if (!await pickFile({ fromProject: true })) cancelConnecting()
  }))
  $('#project-sources').addEventListener('submit', (e) => {
    e.preventDefault()
    const form = e.target
    const f = form.elements
    act(async () => {
      await update({ kind: 'selection', value: { source_id: form.dataset.source, selection: {
        ...current.sources.find((s) => s.source_id === form.dataset.source).selection,
        enabled: f.enabled.checked, only_changes: f.only_changes.checked,
        attachments: privacy.selection(form),
        filter: { topic_ids: f.all_topics.checked ? null : [...form.querySelectorAll('[name="topic"]:checked')].map((input) => input.value),
          dates: f.from.value || f.through.value ? { from: f.from.value || null, through: f.through.value || null, basis: f.basis.value } : null,
          include_unknown_dates: f.include_unknown_dates.checked, include_service: f.include_service.checked }
      } } })
      delete form.dataset.dirty
      await render({ preserveSourceDrafts: true })
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
    open: () => act(open),
    busy: () => working || busy(),
    isConnecting: () => connecting,
    cancelConnecting,
    async attachSelected() {
      await act(async () => {
        const account = $('#project-account-label').value.trim()
        if (!account) throw new Error('Укажите название аккаунта, чтобы различать экспорты.')
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
            selection: { ...existing?.selection, enabled: true, only_changes: existing?.selection.only_changes || false,
              filter: { ...(existing?.selection.filter || {}), topic_ids: topicIds } } }
          await update({ kind: 'source', value: source })
          await refresh(sourceId)
        }
        await open()
      })
    },
  }
}
