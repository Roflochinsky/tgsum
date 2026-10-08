// Scheduling controls use the same IPC/coordinator as the application timer.
export function mountTelegramRefresh({ invoke, act, project, changed, toast }) {
  const notified = new Set()
  function source(card, source) {
    if (source.connector_id !== 'telegram_json' || source.scope.platform !== 'telegram') return
    const panel = document.createElement('details')
    panel.className = 'assisted-export'
    panel.dataset.telegramRefresh = source.source_id
    panel.innerHTML = `<summary>Расписание обновления</summary>
      <p data-refresh-availability></p>
      <label>Когда обновлять<select data-refresh-cadence>
        <option value="manual">Только вручную</option>
        <option value="on_start">При запуске TGSUM</option>
        <option value="every_six_hours">Каждые 6 часов</option>
        <option value="daily">Раз в сутки</option>
      </select></label>
      <p class="hint">Расписание работает, пока TGSUM открыт и компьютер разблокирован. Обновление переписки не запускает анализ с помощью ИИ.</p>
      <button type="button" class="btn btn-ghost" data-refresh-save>Сохранить расписание</button>
      <p role="status" data-refresh-status>Загрузка состояния…</p>
      <div class="project-actions">
        <button type="button" class="btn btn-primary" data-refresh-now>Обновить автоматически</button>
        <button type="button" class="btn btn-ghost" data-refresh-cancel hidden>Остановить</button>
        <button type="button" class="btn btn-ghost" data-refresh-reload hidden>Перезагрузить проект</button>
      </div>
      <div data-refresh-recovery hidden>
        <p>Предыдущая попытка не завершена в TGSUM. Проверьте состояние экспорта в Telegram Desktop.</p>
        <label class="project-check"><input type="checkbox" data-refresh-confirm> Экспорт в Telegram завершён или остановлен.</label>
        <button type="button" class="btn btn-ghost" data-refresh-resolve disabled>Подтвердить проверку</button>
        <p class="hint">Готовый файл можно импортировать через раздел выше. Подтверждение снимает блокировку попытки и оставляет паузу перед повтором.</p>
      </div>`
    card.append(panel)
    const $ = selector => panel.querySelector(selector)
    const target = { projectId: project().project_id, sourceId: source.source_id }
    let view = null
    let loading = false
    let draft = false
    function render() {
      if (!view) {
        for (const el of panel.querySelectorAll('button,select')) el.disabled = true
        return
      }
      const cadence = view.project.telegram_refresh?.[source.source_id]?.cadence || 'manual'
      if (!draft) $('[data-refresh-cadence]').value = cadence
      const state = view.state.state
      const active = !!view.run_id
      const stale = view.project.revision !== project().revision
      const recovery = state === 'needs_user_action'
      const configured = !!view.project.assisted_exports?.[source.source_id]
      $('[data-refresh-availability]').textContent = view.available
        ? (configured ? 'Обновляется только этот подключённый чат.' : 'Сначала назначьте папку экспорта в разделе выше.')
        : 'Автоматическая выгрузка для этого клиента пока недоступна. Используйте экспорт через Telegram Desktop.'
      const continuous = !!view.project.telegram_continuous?.[source.source_id]
      if (continuous) $('[data-refresh-availability]').textContent = 'Этот чат связан с журналом сообщений. Управление сбором — в разделе «Собирать сообщения из Telegram».'
      $('[data-refresh-cadence]').disabled = active || stale || continuous
      for (const option of $('[data-refresh-cadence]').options) option.disabled = option.value !== 'manual' && (!view.available || !configured)
      $('[data-refresh-save]').disabled = active || stale || continuous || (!view.available && $('[data-refresh-cadence]').value !== 'manual')
      $('[data-refresh-now]').disabled = !view.available || !configured || active || stale || recovery || continuous
      $('[data-refresh-cancel]').hidden = !active
      $('[data-refresh-cancel]').disabled = state === 'cancelling'
      $('[data-refresh-reload]').hidden = !stale
      $('[data-refresh-reload]').disabled = false
      $('[data-refresh-recovery]').hidden = !recovery
      $('[data-refresh-resolve]').disabled = stale || !$('[data-refresh-confirm]').checked
      const delta = view.state.delta
      let text = ({ idle: 'Ожидается обновление.', unavailable: 'Доступен ручной экспорт.',
        waiting_for_client: 'Ожидание клиента…', exporting: 'Telegram выполняет экспорт…', cancelling: 'Ожидание подтверждения остановки…',
        needs_user_action: 'Нужна проверка предыдущей попытки.', cancelled: 'Обновление отменено.', failed: 'Обновление не завершено. Предыдущие данные сохранены.',
        ready: delta ? `+${delta.created} новых · ${delta.edited} изменённых · ${delta.missing} отсутствуют в новом архиве` : 'Чат обновлён.',
      })[state] || 'Обновление отложено.'
      const decision = state === 'deferred' ? view.state.decision : view.decision
      if (!active && decision?.wait_until) text += ` Повтор доступен после ${new Date(decision.wait_until * 1000).toLocaleString()}.`
      if (!active && decision === 'needs_active_session') text += ' Ожидается разблокированная сессия компьютера.'
      if (state === 'deferred' && decision === 'busy') text = 'Сейчас выполняется другая операция.'
      if (state === 'deferred' && decision === 'needs_user_action') text = 'Проверьте незавершённый экспорт в подключённых проектах.'
      $('[data-refresh-status]').textContent = text
      panel.dataset.state = state
    }
    async function load() {
      if (loading || !panel.isConnected) return
      loading = true
      try {
        const result = await invoke('telegram_refresh_status', target)
        if (!panel.isConnected || project().project_id !== target.projectId) return
        if (result.project.revision < project().revision) return
        view = result
        render()
        if (view.state.state === 'ready' && view.project.revision > project().revision) {
          const id = `${target.projectId}:${source.source_id}:${view.state.project.sources.find(s => s.source_id === source.source_id)?.latest_snapshot_id}`
          if (!notified.has(id)) { notified.add(id); toast($('[data-refresh-status]').textContent) }
          await changed(view.project, source.source_id)
        }
      } catch (error) { $('[data-refresh-status]').textContent = error?.message || String(error) }
      finally { loading = false }
    }
    panel.addEventListener('input', event => { event.stopPropagation(); if (event.target.matches('[data-refresh-cadence]')) draft = true; render() })
    $('[data-refresh-save]').onclick = () => act(async () => {
      const updated = await invoke('set_telegram_refresh_cadence', { ...target, expectedRevision: project().revision, cadence: $('[data-refresh-cadence]').value })
      await changed(updated, source.source_id)
    })
    $('[data-refresh-now]').onclick = () => act(async () => {
      await invoke('start_telegram_refresh', { ...target, expectedRevision: project().revision })
      view = await invoke('telegram_refresh_status', target)
      if (view.project.revision !== project().revision) await changed(view.project, source.source_id)
      else render()
    })
    $('[data-refresh-cancel]').onclick = () => act(async () => {
      await invoke('cancel_telegram_refresh', { ...target, runId: view.run_id })
      $('[data-refresh-cancel]').disabled = true
      $('[data-refresh-status]').textContent = 'Запрошена остановка…'
    })
    $('[data-refresh-reload]').onclick = () => act(async () => changed(await invoke('open_project', { projectId: target.projectId }), source.source_id))
    $('[data-refresh-resolve]').onclick = () => act(async () => {
      const updated = await invoke('resolve_telegram_refresh', { ...target, expectedRevision: project().revision, clientStoppedConfirmed: $('[data-refresh-confirm]').checked })
      await changed(updated, source.source_id)
    })
    render()
    panel.addEventListener('toggle', () => { if (panel.open) load() })
    const timer = setInterval(() => {
      if (!panel.isConnected) clearInterval(timer)
      else if (panel.open && !document.hidden) load()
    }, 1000)
  }
  return { source }
}
