// The worker keeps running independently of this panel and its polling timer.
export function mountTelegramContinuous({ invoke, act, project, changed, busy }) {
  function source(card, source) {
    if (source.connector_id !== 'telegram_json' || source.scope.platform !== 'telegram') return
    const panel = document.createElement('details')
    panel.className = 'assisted-export'
    panel.dataset.telegramContinuous = source.source_id
    panel.innerHTML = `<summary>Собирать сообщения из Telegram</summary>
      <p>Экспериментальный сбор текста из диагностических логов Telegram Desktop 7.2.5 на Linux. Начальная история и вложения остаются из подключённого архива.</p>
      <p class="hint">Telegram должен работать с отладкой и записывать папку DebugLogs. При включённом сборе окно TGSUM можно закрыть: процесс продолжит работу. Автозапуск настраивается в «Работа в фоне». Полная история и получение каждого нового сообщения не гарантированы.</p>
      <label>Папка DebugLogs<input type="text" data-continuous-directory spellcheck="false" placeholder="Абсолютный путь к DebugLogs"></label>
      <button type="button" class="btn btn-ghost" data-continuous-pick>Выбрать папку…</button>
      <label class="project-check"><input type="checkbox" data-continuous-account> В этом Telegram Desktop подключён один аккаунт.</label>
      <p class="hint">Telegram может писать логи других чатов в эту папку. TGSUM сохраняет в своём журнале только подключённый чат, связывая сообщения и авторов по их ID. Логи не входят в результат.</p>
      <p role="status" data-continuous-status>Загрузка состояния…</p>
      <p class="hint" data-continuous-counts></p>
      <ul class="hint" data-continuous-gaps hidden></ul>
      <p class="hint" data-continuous-path></p>
      <div class="project-actions">
        <button type="button" class="btn btn-primary" data-continuous-start disabled>Запустить сбор</button>
        <button type="button" class="btn btn-ghost" data-continuous-stop hidden>Остановить сбор</button>
      </div>
      <p class="hint">Сообщения сохраняются в истории проекта. Для Markdown и вложений настройте «Локальный пакет» ниже. Остановка сохраняет уже собранную историю. Автоматическая отправка этого пакета отключена.</p>`
    card.append(panel)
    const $ = selector => panel.querySelector(selector)
    const target = { projectId: project().project_id, sourceId: source.source_id }
    let view = null
    let loading = false
    let draft = false
    function render() {
      if (!view) return
      const plan = view.project.telegram_continuous?.[source.source_id]
      const enabled = !!plan?.settings.enabled
      if (plan) {
        $('[data-continuous-directory]').value = plan.settings.input_directory
        $('[data-continuous-account]').checked = plan.settings.confirmed_single_account
      }
      $('[data-continuous-directory]').disabled = !!plan || !view.supported
      $('[data-continuous-pick]').disabled = !!plan || !view.supported
      $('[data-continuous-account]').disabled = !!plan || !view.supported
      const state = view.observation
      panel.dataset.state = state.phase
      const labels = {
        stopped: plan ? 'Сбор остановлен. История сохранена.' : 'Сбор ещё не настроен.',
        starting: 'Настройка сохранена. Ожидание запуска сборщика…',
        watching: 'Сборщик проверяет логи. Ожидание следующих данных…',
        waiting_for_operation: 'Наблюдения сохраняются в журнале. Обновление проекта ожидает завершения текущей операции.',
        failed: state.message || 'Сбор остановлен из-за ошибки. Повторите запуск.',
        unavailable: 'На этой ОС сбор диагностических логов пока недоступен.'
      }
      $('[data-continuous-status]').textContent = labels[state.phase] || 'Проверка состояния…'
      $('[data-continuous-counts]').textContent = (state.counts_known
        ? `Сохранено наблюдений: ${state.events}. Применено к проекту: ${state.applied_events}.`
        : `Применено к проекту: ${state.applied_events}. Число оставшихся наблюдений и причины пропусков будут прочитаны из журнала после запуска.`) +
        (state.last_poll ? ` Последняя проверка: ${new Date(state.last_poll).toLocaleTimeString()}.` : '')
      const gaps = {
        started_without_history: 'Полная история не запрашивалась', file_rotated: 'Файл логов сменился', file_truncated: 'Файл логов сократился',
        source_rewritten_during_poll: 'Файл логов изменился во время чтения', malformed_packet: 'Запись не удалось разобрать',
        logging_restarted: 'Telegram начал новый сеанс записи логов; возможны пропуски',
        collector_restarted: 'Перерыв между запусками сборщика; возможны пропуски',
        unsupported_packet: 'Неизвестный формат записи', unsupported_update: 'Неизвестный вид обновления',
        unsupported_message: 'Сообщение не удалось разобрать', incomplete_history: 'История неполная',
        unresolved_delete: 'Не удалось связать удаление с чатом', unresolved_outgoing: 'Не удалось связать исходящее сообщение',
        missing_self_user: 'Неизвестный ID владельца аккаунта'
      }
      $('[data-continuous-gaps]').replaceChildren(...state.gaps.map(item => {
        const li = document.createElement('li')
        const reason = item.gap.reason === 'parser' ? item.gap.detail : item.gap.reason
        li.textContent = `${gaps[reason] || 'Ограничение наблюдения'}: ${item.count}`
        return li
      }))
      $('[data-continuous-gaps]').hidden = state.gaps.length === 0
      $('[data-continuous-path]').textContent = view.journal_path ? `Локальный журнал: ${view.journal_path}` : ''
      const stale = view.project.revision !== project().revision
      $('[data-continuous-start]').textContent = state.phase === 'failed' ? 'Повторить запуск' : 'Запустить сбор'
      $('[data-continuous-start]').disabled = !view.supported || stale || (enabled && state.phase !== 'failed') ||
        !($('[data-continuous-directory]').value.trim() && $('[data-continuous-account]').checked)
      $('[data-continuous-stop]').hidden = !enabled
      $('[data-continuous-stop]').disabled = stale
    }
    async function load() {
      if (loading || !panel.isConnected) return
      loading = true
      try {
        const result = await invoke('telegram_continuous_status', target)
        if (!panel.isConnected || project()?.project_id !== target.projectId) return
        if (result.project.revision < project().revision) return
        view = result
        if (!busy() && view.project.revision > project().revision) {
          await changed(view.project, source.source_id)
        }
        if (panel.isConnected && project()?.project_id === target.projectId && view.project.revision >= project().revision) render()
      } catch (error) { $('[data-continuous-status]').textContent = error?.message || String(error) }
      finally { loading = false }
    }
    panel.addEventListener('input', event => { event.stopPropagation(); draft = !view?.project.telegram_continuous?.[source.source_id]; if (draft) card.dataset.continuousDirty = 'true'; render() })
    $('[data-continuous-pick]').onclick = () => act(async () => {
      const path = await invoke('pick_telegram_log_directory')
      if (path) { $('[data-continuous-directory]').value = path; draft = true; card.dataset.continuousDirty = 'true'; render() }
    })
    async function setEnabled(enabled) {
      const updated = await invoke('set_telegram_continuous', { request: {
        project_id: target.projectId, source_id: target.sourceId, expected_revision: project().revision, enabled,
        input_directory: $('[data-continuous-directory]').value.trim() || null,
        confirmed_single_account: $('[data-continuous-account]').checked } })
      draft = false
      delete card.dataset.continuousDirty
      await changed(updated, source.source_id)
    }
    $('[data-continuous-start]').onclick = () => act(() => setEnabled(true))
    $('[data-continuous-stop]').onclick = () => act(() => setEnabled(false))
    panel.addEventListener('toggle', () => { if (panel.open) load() })
    const timer = setInterval(() => {
      if (!panel.isConnected) clearInterval(timer)
      else if (panel.open && !document.hidden && document.body.dataset.screen === 'projects') load()
    }, 1000)
  }
  return { source }
}
