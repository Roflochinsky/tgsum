// Assisted Desktop export: each launch/import is a separate explicit action.
// No process exit, timer or file appearance is used as a completion signal.
export function mountAssisted({ invoke, act, project, update, invalidate, imported, startJob, endJob, show, toast }) {
  function source(card, source, title) {
    if (source.connector_id !== 'telegram_json' || source.scope.platform !== 'telegram') return
    const section = document.createElement('details')
    section.className = 'assisted-export'
    section.dataset.assistedSource = source.source_id
    section.dataset.state = 'needs_user_action'
    section.innerHTML = `<summary>Обновить через Telegram Desktop</summary>
      <p class="hint">Экспорт выполняется в Telegram Desktop. TGSUM получает выбранный JSON; сессиями и входом управляет сам клиент.</p>
      <p data-assisted-scope></p>
      <ol><li>Откройте нужный аккаунт и этот чат в Telegram Desktop.</li>
        <li>В меню чата ⋮ выберите «Экспорт истории чата», формат JSON и папку ниже.</li>
        <li>Дождитесь завершения в Telegram. Если он просит подтверждение с другого устройства или ожидание, выполните его инструкцию.</li>
        <li>Выберите созданный result.json и подтвердите источник. TGSUM покажет файлы из назначенной папки, но не считает появление файла окончанием экспорта.</li></ol>
      <label>Назначенная папка<input data-assisted-directory readonly></label>
      <div class="project-actions"><button type="button" class="btn btn-ghost" data-assisted-folder>Выбрать папку…</button></div>
      <div data-assisted-candidates hidden><p class="hint">Замечены возможные JSON экспорты. Выберите файл только после завершения в Telegram Desktop.</p><div class="project-actions" data-assisted-candidate-list></div></div>
      <label>Выбранный клиент<input data-assisted-client readonly placeholder="Можно открыть Desktop самостоятельно"></label>
      <p class="hint">Выберите установленный исполняемый файл Desktop. TGSUM не проверяет его издателя. Пакет .app, Flatpak или ярлык можно открыть самостоятельно.</p>
      <div class="project-actions"><button type="button" class="btn btn-ghost" data-assisted-pick-client>Выбрать клиент…</button>
        <button type="button" class="btn btn-ghost" data-assisted-clear-client>Забыть клиент</button>
        <button type="button" class="btn btn-ghost" data-assisted-launch>Открыть выбранный клиент</button></div>
      <label>JSON завершённого экспорта<input data-assisted-archive readonly placeholder="Выберите файл после завершения экспорта"></label>
      <div class="project-actions"><button type="button" class="btn btn-ghost" data-assisted-pick-archive>Выбрать готовый JSON…</button></div>
      <label class="project-check"><input type="checkbox" data-assisted-confirm> Я выбрал указанные аккаунт и чат; Telegram завершил этот экспорт.</label>
      <p class="hint">Локальная метка аккаунта задаётся вами; JSON не подтверждает владельца аккаунта. При импорте TGSUM проверит ID чата.</p>
      <p role="status" data-assisted-status></p>
      <button type="button" class="btn btn-primary" data-assisted-import>Импортировать завершённый экспорт</button>`
    card.append(section)
    const $ = (selector) => section.querySelector(selector)
    $('[data-assisted-scope]').textContent = `Источник: ${title} · аккаунт ${source.scope.account_local_id} · ID чата ${source.scope.conversation_id}`
    let archive = ''
    let watching = false
    const settings = () => project().assisted_exports?.[source.source_id]
    function availability() {
      const config = settings()
      $('[data-assisted-directory]').value = config?.directory || ''
      $('[data-assisted-client]').value = config?.client || ''
      $('[data-assisted-archive]').value = archive
      $('[data-assisted-launch]').disabled = !config?.client
      $('[data-assisted-clear-client]').disabled = !config?.client
      $('[data-assisted-pick-client]').disabled = !config
      $('[data-assisted-pick-archive]').disabled = !config
      $('[data-assisted-import]').disabled = !config || !archive || !$('[data-assisted-confirm]').checked
    }
    function status(text, state = 'needs_user_action') {
      section.dataset.state = state
      $('[data-assisted-status]').textContent = text
    }
    function reset() { $('[data-assisted-confirm]').checked = false; availability() }
    async function pollCandidates() {
      if (watching || !section.isConnected || !section.open || document.hidden || !settings() || section.dataset.state === 'importing') return
      watching = true
      const revision = project().revision
      try {
        const candidates = await invoke('poll_assisted_exports', { projectId: project().project_id,
          sourceId: source.source_id, expectedRevision: revision })
        if (!section.isConnected || project().revision !== revision) return
        const list = $('[data-assisted-candidate-list]')
        list.replaceChildren(...candidates.slice(0, 8).map((candidate) => {
          const button = document.createElement('button')
          button.type = 'button'
          button.className = 'btn btn-ghost'
          const name = candidate.path.split(/[/\\]/).slice(-2).join('/')
          button.textContent = `${name} · ${(candidate.bytes / 1024).toFixed(0)} КБ`
          button.title = candidate.path
          button.onclick = () => { archive = candidate.path; reset(); status('Проверьте аккаунт и чат в Desktop, затем подтвердите завершение экспорта.') }
          return button
        }))
        $('[data-assisted-candidates]').hidden = candidates.length === 0
      } catch (error) {
        if (error?.kind === 'conflict' && project().revision !== revision) return
        if (error?.kind === 'conflict') {
          clearInterval(timer)
          status('Проект изменён. Откройте его заново перед обновлением.')
        } else {
          status(`Папка экспорта недоступна: ${error?.message || String(error)}. Можно выбрать JSON вручную.`)
        }
      } finally { watching = false }
    }
    async function save(config) {
      await update({ kind: 'assisted_export', value: { source_id: source.source_id, settings: config } })
      invalidate()
      reset()
      status('Настройки сохранены. Ожидается экспорт и ваше подтверждение.')
    }
    section.addEventListener('input', (event) => { event.stopPropagation(); availability() })
    $('[data-assisted-folder]').onclick = () => act(async () => {
      const directory = await invoke('pick_assisted_path', { kind: 'directory' })
      if (directory) await save({ directory, client: settings()?.client || null })
    })
    $('[data-assisted-pick-client]').onclick = () => act(async () => {
      const client = await invoke('pick_assisted_path', { kind: 'client' })
      if (client) await save({ ...settings(), client })
    })
    $('[data-assisted-clear-client]').onclick = () => act(() => save({ ...settings(), client: null }))
    $('[data-assisted-launch]').onclick = () => act(async () => {
      reset()
      try {
        await invoke('launch_assisted_client', { projectId: project().project_id, sourceId: source.source_id, expectedRevision: project().revision })
        status('Запуск запрошен. Выберите нужный аккаунт и чат в Desktop, выполните экспорт и дождитесь его завершения.')
      } catch (error) {
        status('Клиент не запущен. Откройте Desktop самостоятельно и выберите готовый JSON.')
        throw error
      }
    })
    $('[data-assisted-pick-archive]').onclick = () => act(async () => {
      const path = await invoke('pick_assisted_path', { kind: 'archive' })
      if (path) { archive = path; reset(); status('Проверьте указанные аккаунт и чат, затем подтвердите завершение экспорта.') }
    })
    $('[data-assisted-import]').onclick = () => act(async () => {
      if (!$('[data-assisted-confirm]').checked || !archive || !settings()) return
      if (!document.querySelector('#project-unsaved').hidden) throw new Error('Сначала сохраните выбор сообщений и вложений источника.')
      invalidate()
      status('Проверка и импорт JSON…', 'importing')
      startJob('import', 'Импорт завершённого экспорта Telegram')
      try {
        const completed = await invoke('import_assisted_export', { request: { project_id: project().project_id,
          source_id: source.source_id, expected_revision: project().revision,
          archive_path: archive, scope_and_completion_confirmed: true } })
        await imported(completed.project)
        const delta = completed.delta
        toast(`Источник обновлён: +${delta.created} новых · ${delta.edited} изменённых · ${delta.missing} отсутствуют в новом архиве`)
      } catch (error) {
        reset()
        status(error?.kind === 'cancelled' ? 'Импорт отменён. Предыдущий snapshot сохранён.' : 'Импорт не завершён. Предыдущий snapshot сохранён; проверьте файл и повторите подтверждение.')
        if (error?.kind !== 'cancelled') throw error
      } finally { endJob(); show('projects') }
    })
    availability()
    status('Ожидается ручной экспорт. Открыть клиент и предоставить JSON можно самостоятельно.')
    section.addEventListener('toggle', () => { if (section.open) pollCandidates() })
    const timer = setInterval(() => {
      if (!section.isConnected) clearInterval(timer)
      else pollCandidates()
    }, 3000)
  }
  return { source }
}
