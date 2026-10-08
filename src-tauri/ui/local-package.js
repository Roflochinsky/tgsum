// Saved local-file pipeline. It never controls or signs in to Telegram.
export function mountLocalPackage({ invoke, act, project, toast, reload }) {
  let renderEpoch = 0
  async function render(container, { preserveDraft = false } = {}) {
    const previous = preserveDraft ? container.querySelector('[data-local-package][data-dirty="true"]') : null
    const draft = previous ? {
      open: previous.open,
      input: previous.querySelector('[data-package-input]').value,
      output: previous.querySelector('[data-package-output]').value,
      repository: previous.querySelector('[data-package-repo]').value,
      automatic: previous.querySelector('[data-package-auto]').checked,
      images: previous.querySelector('[data-package-images]').checked,
      office: previous.querySelector('[data-package-office]').checked,
    } : null
    const epoch = ++renderEpoch
    container.replaceChildren()
    const id = project().project_id
    const selected = project().sources.filter(s => s.selection.enabled)
    let state = await invoke('local_package_status', { projectId: id })
    if (!container.isConnected || project()?.project_id !== id || epoch !== renderEpoch) return
    if (!state && !selected.some(s => s.connector_id === 'telegram_json')) return
    const observed = selected.filter(s => project().telegram_continuous?.[s.source_id])
    const liveOnly = observed.length > 0 && observed.length === selected.length
    const fromLogs = observed.length > 0
    const section = document.createElement('details')
    section.className = 'card assisted-export'
    section.dataset.localPackage = 'project'
    section.open = draft ? draft.open : !!state
    section.innerHTML = `<summary>Собрать выбранные чаты в папку</summary>
      <p class="hint">Сохраните выбор чатов и тем выше. ${fromLogs ? 'Сообщения чатов, подключённых к сборщику, берутся из сохранённой истории проекта; остальные — из готового экспорта.' : 'Затем укажите папку готового экспорта Telegram.'} TGSUM соберёт сообщения и выбранные вложения в один пакет файлов. Исходники остаются на месте; предыдущая созданная копия заменяется после успешного обновления.</p>
      <p data-package-selection></p>
      <label>${liveOnly ? 'Папка начальной выгрузки' : 'Откуда читать экспорт'}<input data-package-input readonly></label>
      <p class="hint">${liveOnly ? 'Новый result.json не требуется: сообщения уже сохранены в проекте. Укажите существующую локальную папку для настройки пакета. Начальные вложения берутся из ранее выбранной папки медиа.' : 'Выберите папку с result.json и папками вложений. Подойдёт и папка, содержащая ChatExport_* или DataExport_*: TGSUM возьмёт самый свежий result.json. Для нескольких чатов нужен общий экспорт, в котором есть все выбранные архивные чаты.'}</p>
      <button type="button" class="btn btn-ghost" data-package-pick-input>Выбрать папку экспорта…</button>
      <label>Куда сохранять результат<input data-package-output readonly></label>
      <button type="button" class="btn btn-ghost" data-package-pick-output>Выбрать папку результата…</button>
      <p class="hint">В этой папке TGSUM создаст отдельную подпапку проекта. Точный путь к готовым файлам появится ниже после сборки.</p>
      <label class="project-check"><input type="checkbox" data-package-images checked> Включать картинки без изменений, включая метаданные</label>
      <label class="project-check"><input type="checkbox" data-package-office checked> Включать DOCX/XLSX: Фамилия Имя Отчество → Фамилия И. О.</label>
      <p class="hint">Картинки не обезличиваются. Сокращение ФИО в документах не является полной анонимизацией. PDF, DOC, XLS, аудио и архивы пока отмечаются как не включённые. Текстовые вложения обрабатываются по правилам приватности проекта.</p>
      <label class="project-check"><input type="checkbox" data-package-auto> Автоматически обновлять пакет при изменении ${fromLogs ? 'истории проекта' : 'выгрузки'}, пока TGSUM запущен</label>
      <p class="hint">${fromLogs ? 'Пакет обновляется по сохранённым наблюдениям сборщика. Для остальных чатов новый экспорт сохраняется вручную. Логи не являются полной историей.' : 'Новый экспорт нужно сохранить из Telegram самостоятельно. TGSUM проверяет локальную папку, пока приложение запущено. Сбор сообщений можно отдельно настроить в карточке чата.'}</p>
      <label>Приватный репозиторий GitHub, необязательно<input data-package-repo placeholder="owner/TGSUM-IMPORT"></label>
      <p class="hint">Оставьте поле пустым, чтобы сохранять только на компьютере. Указанный репозиторий включает публикацию через установленную программу gh и выполненный в ней вход. Удалённые файлы сохраняются в истории Git.</p>
      <div class="project-actions"><button type="button" class="btn btn-ghost" data-package-save>Сохранить настройки</button>
        <button type="button" class="btn btn-primary" data-package-refresh>Собрать пакет</button>
        <button type="button" class="btn btn-ghost" data-package-stop>Остановить обновление</button>
        <button type="button" class="btn btn-ghost" data-package-open>Открыть готовую папку</button></div>
      <p role="status" data-package-status></p><p data-package-ready-directory hidden></p><p class="hint" data-package-counts></p><p class="hint" data-package-github></p>`
    container.append(section)
    const $ = s => section.querySelector(s)
    let running = false
    let dirty = !!draft
    if (draft) section.dataset.dirty = 'true'
    let polling = false
    const settings = state?.settings
    $('[data-package-input]').value = draft?.input ?? (settings?.input_directory || selected[0]?.archive_path?.split('/').slice(0, -1).join('/') || '')
    $('[data-package-output]').value = draft?.output ?? (settings?.output_directory || '')
    $('[data-package-auto]').checked = draft?.automatic ?? (settings?.automatic || false)
    $('[data-package-images]').checked = draft?.images ?? (settings?.include_images ?? true)
    $('[data-package-office]').checked = draft?.office ?? (settings?.include_office ?? true)
    $('[data-package-repo]').value = draft?.repository ?? (settings?.github_repository || '')
    $('[data-package-repo]').disabled = fromLogs
    $('[data-package-selection]').textContent = `Выбрано чатов: ${selected.length}. ` + selected.map(s => {
      const title = document.querySelector(`[data-source="${CSS.escape(s.source_id)}"] h3`)?.textContent || s.scope.conversation_id
      return `${title}: ${s.selection.filter.topic_ids === null ? 'все темы' : 'тем — ' + s.selection.filter.topic_ids.length}`
    }).join('; ')
    function renderStatus() {
      section.dataset.phase = running ? 'building' : state?.phase || 'unconfigured'
      $('[data-package-status]').textContent = state?.message || 'Выберите папки, сохраните настройки и нажмите «Собрать пакет».'
      const ready = state?.ready
      $('[data-package-ready-directory]').hidden = !ready
      $('[data-package-ready-directory]').textContent = ready ? `Готовые файлы: ${ready.directory}` : ''
      $('[data-package-counts]').textContent = ready ? `Готово ${new Date(ready.prepared_at * 1000).toLocaleString()}: ${ready.conversations ?? 1} чатов · ${ready.messages} сообщений · ${ready.files} файлов · ${(ready.bytes / 1024 / 1024).toFixed(1)} МиБ · ${ready.skipped_attachments} вложений не включено · ${ready.initials_replacements} ФИО сокращено. Полнота истории неизвестна.` : ''
      $('[data-package-github]').textContent = fromLogs || ready?.local_only ? 'Пакет с диагностическими наблюдениями сохраняется только локально. Автоматическая публикация отключена; прежняя настройка репозитория сохранена.' :
        state?.github_error || (state?.github_commit && ready?.content_sha256 === state.github_content_sha256 ? `GitHub: опубликовано, коммит ${state.github_commit.slice(0, 8)}` : state?.settings.github_repository ? 'GitHub: ожидается публикация готового пакета.' : 'Локальный пакет; публикация GitHub отключена.')
      if (!dirty) $('[data-package-auto]').checked = state?.settings.automatic || false
      $('[data-package-refresh]').disabled = !state || running || dirty
      $('[data-package-save]').disabled = running
      $('[data-package-stop]').disabled = !running && state?.phase !== 'building' && !state?.settings.automatic
      $('[data-package-open]').disabled = !ready
    }
    section.addEventListener('input', e => { e.stopPropagation(); dirty = true; section.dataset.dirty = 'true'; renderStatus() })
    for (const [button, field, command, args] of [
      ['input', 'input', 'pick_assisted_path', { kind: 'directory' }],
      ['output', 'output', 'pick_out_dir', {}],
    ]) {
      $(`[data-package-pick-${button}]`).onclick = () => act(async () => {
        const path = await invoke(command, args)
        if (path) { $(`[data-package-${field}]`).value = path; dirty = true; section.dataset.dirty = 'true'; renderStatus() }
      })
    }
    $('[data-package-save]').onclick = () => act(async () => {
      if (!document.querySelector('#project-unsaved').hidden) throw new Error('Сначала сохраните выбор чата и периода.')
      state = await invoke('configure_local_package', { projectId: id, expectedRevision: project().revision, settings: {
        source_ids: selected.map(s => s.source_id), input_directory: $('[data-package-input]').value,
        output_directory: $('[data-package-output]').value, automatic: $('[data-package-auto]').checked,
        include_images: $('[data-package-images]').checked, include_office: $('[data-package-office]').checked,
        github_repository: $('[data-package-repo]').value.trim() || null,
      } })
      dirty = false; delete section.dataset.dirty; renderStatus(); toast('Настройки пакета сохранены'); await reload()
    })
    $('[data-package-refresh]').onclick = async () => {
      if (running || dirty) return
      if (!document.querySelector('#project-unsaved').hidden) { toast('Сначала сохраните выбор чата и периода.', 'error'); return }
      running = true; renderStatus()
      try {
        state = await invoke('refresh_local_package', { projectId: id })
        toast(state.phase === 'ready' ? 'Локальный пакет готов' : state.message, state.phase === 'ready' ? undefined : 'error')
        await reload()
      } catch (e) { toast(e?.message || String(e), 'error') }
      finally { running = false; if (section.isConnected) renderStatus() }
    }
    $('[data-package-stop]').onclick = async () => {
      try { await invoke('cancel_local_package', { projectId: id }); state = await invoke('local_package_status', { projectId: id }); renderStatus() }
      catch (e) { toast(e?.message || String(e), 'error') }
    }
    $('[data-package-open]').onclick = () => act(() => invoke('open_folder', { path: state.ready.directory }))
    const timer = setInterval(async () => {
      if (!section.isConnected) { clearInterval(timer); return }
      if (polling || document.hidden || !section.open) return
      polling = true
      try { state = await invoke('local_package_status', { projectId: id }); renderStatus() }
      catch (e) { $('[data-package-status]').textContent = e?.message || String(e) }
      finally { polling = false }
    }, 1500)
    renderStatus()
  }
  return { render }
}
