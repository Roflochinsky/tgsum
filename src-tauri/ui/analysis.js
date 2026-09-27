// Reviewed run IDs are capabilities owned by the desktop controller. Changing
// options discards that capability before another preparation can begin.
export function mountAnalysis({ invoke, act, project, bundle, reload, startJob, endJob, show, toast }) {
  const $ = (s) => document.querySelector(s)
  const states = { interrupted: 'Запуск прерван — повторный запуск только вручную', cancelled: 'Отменён', failed: 'Не завершён', uncommitted: 'Результат сохранён; точка анализа не обновлена', succeeded: 'Готово · точка анализа обновлена', unavailable: 'Запись недоступна' }
  const sections = { overview: 'Обзор', topics: 'Темы', open_questions: 'Открытые вопросы', worked: 'Что получилось', failed: 'Что не получилось', lessons: 'Выводы', next_steps: 'Следующие шаги', decisions: 'Решения', reversals: 'Изменённые решения', unresolved: 'Незавершённые действия', timeline: 'Хронология', symptoms: 'Симптомы', hypotheses: 'Гипотезы', actions_taken: 'Принятые меры', resolution: 'Решение проблемы', context: 'Контекст', people: 'Участники', systems: 'Системы' }
  const node = (tag, text, className = '') => {
    const element = document.createElement(tag)
    element.textContent = text
    element.className = className
    return element
  }
  let catalog = null
  let reviewed = null
  let selected = null
  let invalidation = Promise.resolve()

  function invalidate() {
    reviewed = null
    $('#analysis-review').hidden = true
    $('#btn-analysis-run').disabled = true
    invalidation = invalidation.then(() => invoke('discard_analysis_review')).catch((e) => toast(e.message || String(e), 'error'))
    return invalidation
  }

  function availability() {
    $('#btn-analysis-prepare').disabled = !catalog?.available || !bundle() || bundle().manifest.privacy.needs_review > 0
  }

  async function reset() {
    await invalidate()
    selected = null
    $('#analysis-result').hidden = true
    if (!catalog) {
      catalog = await invoke('analysis_catalog')
      $('#analysis-recipe').replaceChildren(...catalog.recipes.map((r) => {
        const option = node('option', r.title)
        option.value = r.id
        return option
      }))
      $('#analysis-executable').value = catalog.executables[0] || ''
      $('#analysis-status').textContent = catalog.fixtures
        ? 'Локальный тестовый стенд. Данные не передаются агенту или в сеть.'
        : catalog.available ? `Codex ${catalog.version} · Linux x86_64. Подготовка проверит совместимость без обращения к аккаунту.`
          : 'Запуск Codex недоступен: требуется Linux x86_64 и установленный компонент запуска. Контекст можно сохранить в файл.'
      $('#analysis-runtime-fields').hidden = catalog.fixtures || !catalog.available
      $('#analysis-model').value = catalog.fixtures ? 'fixture-success' : ''
      $('#analysis-destination').disabled = catalog.fixtures
    }
    availability()
    await history()
  }

  async function history() {
    const current = project()
    const list = $('#analysis-history')
    list.replaceChildren()
    if (!current) return
    const entries = await invoke('list_project_analyses', { projectId: current.project_id })
    for (const entry of entries) {
      const button = node('button', `${entry.recipe || 'Анализ'} · ${states[entry.state]} · ${entry.run_id}`, 'btn btn-ghost')
      button.type = 'button'
      button.dataset.run = entry.run_id
      button.disabled = entry.state === 'unavailable'
      list.append(button)
    }
    $('#analysis-history-empty').hidden = entries.length > 0
  }

  function result(view) {
    selected = view
    $('#analysis-result').hidden = false
    $('#analysis-result-status').textContent = states[view.state]
    $('#analysis-result-origin').textContent = `${view.spec.agent} · ${view.spec.model} · ${view.spec.recipe} v${view.spec.recipe_version} · ${view.spec.destination} · ${view.run_id}`
    const coverage = { complete: 'полнота подтверждена', partial: 'неполная история', own_messages_only: 'только собственные сообщения', future_only: 'только новые события', unknown: 'полнота не подтверждена' }
    $('#analysis-result-coverage').textContent = view.coverage.map((c, i) => `Источник ${i + 1}: ${coverage[c.level]} · ${c.messages} сообщений`).join(' · ')
    const output = $('#analysis-result-body')
    output.replaceChildren()
    const claim = (item) => {
      const block = node('div', '', 'analysis-claim')
      block.append(node('p', item.text), node('p', item.evidence.map((e) => `${e.id}@${e.revision}`).join(' · '), 'hint analysis-evidence'))
      return block
    }
    if (view.result) {
      for (const section of view.result.sections) {
        output.append(node('h4', sections[section.id] || section.id))
        if (!section.claims.length) output.append(node('p', 'Нет подтверждённых выводов.', 'hint'))
        for (const item of section.claims) output.append(claim(item))
      }
      if (view.result.actions.length) output.append(node('h4', 'Действия'))
      for (const action of view.result.actions) {
        output.append(claim(action.task))
        for (const [key, label] of [['owner', 'Ответственный'], ['deadline', 'Срок']]) {
          const value = action[key]
          output.append(node('p', `${label}: ${value ? value.value : 'не указан'}`))
          if (value) output.append(node('p', `«${value.quote}» · ${value.evidence.id}@${value.evidence.revision}`, 'hint analysis-evidence'))
        }
      }
    }
    const failures = { authentication: 'Проверьте выбранную авторизацию Codex.', transport: 'Не удалось завершить передачу.', invalid_result: 'Ответ не прошёл проверку структуры и ссылок.', timed_out: 'Время ожидания истекло.', interrupted: 'Процесс был прерван.', agent: 'Агент не завершил анализ. Проверьте модель, получателя и выбранную авторизацию.' }
    $('#analysis-result-note').textContent = view.failure ? failures[view.failure] || 'Анализ не завершён.'
      : view.state === 'uncommitted' ? (view.can_commit ? 'Можно принять сохранённый результат без повторного обращения к агенту.' : 'Проект изменился. Сохранённый результат доступен для чтения; подготовьте новый анализ.')
        : view.state === 'interrupted' ? 'Автоматического повтора нет. Закройте запись, затем подготовьте новый запуск.' : ''
    $('#btn-analysis-commit').hidden = !view.can_commit
    $('#btn-analysis-close').hidden = view.state !== 'interrupted'
    $('#analysis-result').scrollIntoView({ block: 'start' })
  }

  $('#analysis-options').addEventListener('input', () => { invalidate(); availability() })
  for (const [id, kind] of [['#btn-analysis-executable', 'codex'], ['#btn-analysis-auth', 'auth']]) {
    $(id).addEventListener('click', () => act(async () => {
      const path = await invoke('pick_analysis_file', { kind })
      if (path) {
        $(kind === 'codex' ? '#analysis-executable' : '#analysis-auth').value = path
        await invalidate()
      }
    }))
  }
  $('#btn-analysis-prepare').addEventListener('click', () => act(async () => {
    await invalidate()
    const context = bundle()
    if (!context) return
    const options = { executable: $('#analysis-executable').value, auth_file: $('#analysis-auth').value,
      model: $('#analysis-model').value.trim(), recipe: $('#analysis-recipe').value, destination: $('#analysis-destination').value }
    if (!options.model) throw new Error('Укажите модель, доступную в выбранном Codex.')
    startJob('analysis', 'Подготовка запуска без передачи данных агенту')
    try {
      reviewed = await invoke('prepare_project_analysis', { projectId: project().project_id, bundleId: context.bundle_id, expectedRevision: context.project_revision, options })
    } finally { endJob(); show('projects') }
    const s = reviewed.spec
    $('#analysis-review-summary').textContent = `${s.agent} ${s.agent_version} · ${s.recipe} v${s.recipe_version} · модель ${s.model}`
    $('#analysis-review-destination').textContent = s.destination === 'local_fixture' ? 'Получатель: локальный тестовый стенд' : `Получатель контекста: OpenAI · ${s.destination}`
    $('#analysis-review-access').textContent = s.destination === 'local_fixture' ? 'Используются только синтетические данные.'
      : `Агент получит проверенный выше контекст и выбранный файл авторизации только для чтения: ${options.auth_file}. Codex: ${options.executable}.`
    $('#analysis-review').hidden = false
    $('#btn-analysis-run').disabled = false
    $('#analysis-review').scrollIntoView({ block: 'start' })
  }))
  $('#btn-analysis-run').addEventListener('click', () => act(async () => {
    if (!reviewed) return
    const id = reviewed.run_id
    reviewed = null
    $('#btn-analysis-run').disabled = true
    startJob('analysis', 'Анализ выбранного контекста')
    let view
    try { view = await invoke('run_project_analysis', { runId: id }) }
    finally { endJob(); show('projects') }
    await reload()
    result(view)
  }))
  $('#analysis-history').addEventListener('click', (e) => {
    const button = e.target.closest('[data-run]')
    if (button) act(async () => result(await invoke('read_project_analysis', { projectId: project().project_id, runId: button.dataset.run })))
  })
  for (const [id, commit] of [['#btn-analysis-commit', true], ['#btn-analysis-close', false]]) {
    $(id).addEventListener('click', () => act(async () => {
      if (!selected) return
      const view = await invoke('recover_project_analysis', { projectId: project().project_id, runId: selected.run_id, commit })
      await reload()
      result(view)
    }))
  }
  return { reset, invalidate, availability }
}
