// Reviewed run IDs are capabilities owned by the desktop controller. Changing
// options discards that capability before another preparation can begin.
import { preferredAgent } from './onboarding.js'

export function mountAnalysis({ invoke, act, project, bundle, reload, startJob, endJob, show, toast, navigate, changed }) {
  const $ = (s) => document.querySelector(s)
  const states = { interrupted: 'Запуск прерван — повторный запуск только вручную', cancelled: 'Отменён', failed: 'Не завершён', uncommitted: 'Результат сохранён; сообщения ещё не отмечены как проанализированные', succeeded: 'Готово · сообщения отмечены как проанализированные', unavailable: 'Запись недоступна' }
  const sections = { overview: 'Обзор', topics: 'Темы', open_questions: 'Открытые вопросы', worked: 'Что получилось', failed: 'Что не получилось', lessons: 'Выводы', next_steps: 'Следующие шаги', decisions: 'Решения', reversals: 'Изменённые решения', unresolved: 'Незавершённые действия', timeline: 'Хронология', symptoms: 'Симптомы', hypotheses: 'Гипотезы', actions_taken: 'Принятые меры', resolution: 'Решение проблемы', context: 'Контекст', people: 'Участники', systems: 'Системы' }
  const recipes = { summary: 'Краткая сводка', actions: 'Задачи и ответственные', decisions: 'Принятые решения', retro: 'Итоги работы', incident: 'Разбор проблемы', handover: 'Передача дел' }
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
  const agent = () => catalog?.agents.find((a) => a.id === $('#analysis-agent').value)

  function configureAgent() {
    const selectedAgent = agent()
    const exportOnly = !selectedAgent
    $('#analysis-export-only').hidden = !exportOnly
    $('#analysis-task-fields').hidden = exportOnly
    $('#btn-analysis-prepare').hidden = exportOnly
    $('#analysis-run-hint').hidden = exportOnly
    $('#analysis-executable').value = ''
    $('#analysis-auth').value = ''
    $('#analysis-model').value = ''
    if (exportOnly) {
      $('#analysis-runtime-fields').hidden = true
      $('#analysis-status').textContent = 'Файлы сохранятся в выбранную папку на компьютере. Вход в аккаунт и анализ с помощью ИИ не нужны.'
      return
    }
    $('#analysis-executable').value = selectedAgent.executables[0] || ''
    $('#analysis-auth').value = ''
    $('#analysis-model').value = catalog.fixtures ? 'fixture-success' : ''
    $('#analysis-model').placeholder = `ID модели из ${selectedAgent.title}`
    $('#analysis-executable-label').textContent = `Исполняемый файл ${selectedAgent.title}`
    $('#btn-analysis-executable').textContent = `Выбрать ${selectedAgent.title}…`
    $('#analysis-auth-label').textContent = `Файл авторизации ${selectedAgent.title}`
    $('#btn-analysis-auth').textContent = `Выбрать ${selectedAgent.auth_name}…`
    $('#analysis-runtime-hint').textContent = `Нужна установленная программа ${selectedAgent.title} версии ${selectedAgent.version}. При подготовке TGSUM проверит выбранный файл программы.`
    $('#analysis-auth-hint').textContent = `Авторизация остаётся в выбранном файле. Если вход истёк, обновите его в ${selectedAgent.title} отдельно и подготовьте запуск заново.`
    $('#analysis-destination').replaceChildren(...selectedAgent.destinations.map((d) => {
      const option = node('option', d.title)
      option.value = d.id
      return option
    }))
    $('#analysis-destination').disabled = catalog.fixtures || selectedAgent.destinations.length === 1
    $('#analysis-status').textContent = catalog.fixtures
      ? `Локальный тестовый стенд · ${selectedAgent.title}. Данные не передаются агенту или в сеть.`
      : selectedAgent.available ? `${selectedAgent.title} ${selectedAgent.version} · Linux x86_64. Подготовка проверит совместимость без обращения к аккаунту.`
        : `Запуск ${selectedAgent.title} недоступен: требуется Linux x86_64 и установленный компонент запуска. Подготовленные сообщения можно сохранить в папку.`
    $('#analysis-runtime-fields').hidden = catalog.fixtures || !selectedAgent.available
  }

  function invalidate() {
    reviewed = null
    $('#analysis-review').hidden = true
    $('#btn-analysis-run').disabled = true
    changed()
    invalidation = invalidation.then(() => invoke('discard_analysis_review')).catch((e) => toast(e.message || String(e), 'error'))
    return invalidation
  }

  function availability() {
    $('#btn-analysis-prepare').disabled = !agent()?.available || !bundle() || bundle().manifest.privacy.needs_review > 0
  }

  async function reset({ openLatest = false, preservePanel = false } = {}) {
    await invalidate()
    selected = null
    $('#analysis-result').hidden = true
    if (!catalog) {
      catalog = await invoke('analysis_catalog')
      $('#analysis-recipe').replaceChildren(...catalog.recipes.map((r) => {
        const option = node('option', recipes[r.id] || r.title)
        option.value = r.id
        return option
      }))
      $('#analysis-agent').replaceChildren(new Option('Сохранить в папку без ИИ', 'export'), ...catalog.agents.map((a) => {
        const option = node('option', a.title)
        option.value = a.id
        return option
      }))
      $('#analysis-agent').value = preferredAgent()
      configureAgent()
    }
    availability()
    const entries = await history()
    if (openLatest) {
      const latest = entries.find((entry) => entry.state !== 'unavailable')
      if (latest) result(await invoke('read_project_analysis', { projectId: project().project_id, runId: latest.run_id }), { navigateToResult: !preservePanel })
    }
  }

  async function history() {
    const current = project()
    const list = $('#analysis-history')
    list.replaceChildren()
    if (!current) return []
    const entries = await invoke('list_project_analyses', { projectId: current.project_id })
    for (const entry of entries) {
      const title = recipes[entry.recipe] || catalog?.recipes.find((recipe) => recipe.id === entry.recipe)?.title || 'Анализ'
      const button = node('button', `${title} · ${states[entry.state] || entry.state}`, 'btn btn-ghost')
      button.append(node('span', `Запуск ${entry.run_id.slice(-8)}`, 'history-run-id'))
      button.type = 'button'
      button.dataset.run = entry.run_id
      button.disabled = entry.state === 'unavailable'
      button.title = `${entry.agent || 'Агент'} · ${entry.run_id}`
      list.append(button)
    }
    $('#analysis-history-empty').hidden = entries.length > 0
    $('#analysis-history-disclosure').hidden = entries.length === 0
    return entries
  }

  function result(view, { navigateToResult = true } = {}) {
    selected = view
    $('#analysis-result').hidden = false
    if (navigateToResult) navigate('result')
    $('#analysis-result-status').textContent = view.state === 'succeeded' ? 'Сохранённая сводка' : states[view.state]
    $('#analysis-result-status').dataset.state = view.state
    $('#analysis-result-origin').textContent = `${view.spec.agent} · ${view.spec.model} · ${view.spec.recipe} v${view.spec.recipe_version} · ${view.spec.destination} · ${view.run_id}`
    const coverage = { complete: 'полнота подтверждена', partial: 'неполная история', own_messages_only: 'только собственные сообщения', future_only: 'только новые события', unknown: 'полнота не подтверждена' }
    const sourceCount = view.coverage.length
    const messages = view.coverage.reduce((total, source) => total + source.messages, 0)
    const plural = (n, forms) => forms[n % 100 >= 11 && n % 100 <= 14 ? 2 : n % 10 === 1 ? 0 : n % 10 >= 2 && n % 10 <= 4 ? 1 : 2]
    $('#analysis-result-context').textContent = `Сохранённый анализ · ${sourceCount} ${plural(sourceCount, ['источник', 'источника', 'источников'])} · ${messages} ${plural(messages, ['сообщение', 'сообщения', 'сообщений'])}`
    const coverageText = view.coverage.map((c, i) => `${sourceCount > 1 ? `Источник ${i + 1}: ` : ''}${coverage[c.level] || coverage.unknown}`).join(' · ')
    $('#analysis-result-coverage').textContent = `Выводы из сохранённой переписки. ${coverageText.charAt(0).toUpperCase()}${coverageText.slice(1)}.`
    const output = $('#analysis-result-body')
    output.replaceChildren()
    const icon = (name, className = '') => {
      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
      svg.setAttribute('class', `ui-icon ${className}`)
      svg.setAttribute('viewBox', '0 0 16 16')
      svg.setAttribute('aria-hidden', 'true')
      const use = document.createElementNS('http://www.w3.org/2000/svg', 'use')
      use.setAttribute('href', `icons/bootstrap.svg#${name}`)
      svg.append(use)
      return svg
    }
    const evidence = (items, label = 'Основание') => {
      const details = node('details', '', 'claim-evidence')
      const trigger = node('summary', label)
      trigger.append(icon('chevron-down'))
      details.append(trigger)
      for (const text of items) details.append(node('p', text, 'hint analysis-evidence'))
      return details
    }
    const claim = (item, className = '') => {
      const block = node('div', '', `analysis-claim ${className}`)
      block.append(node('p', item.text), evidence(item.evidence.map((e) => `Сообщение ${e.id} · версия ${e.revision}`)))
      return block
    }
    const summary = $('#analysis-result-summary')
    summary.replaceChildren()
    $('#analysis-heading-evidence').replaceChildren()
    $('#result-title').textContent = recipes[view.spec.recipe] || 'Сводка проекта'
    if (view.result) {
      const result = view.result
      // The headline is a saved, evidence-backed claim, never an invented status.
      // Different recipes expose different sections; do not merge prior runs.
      const lead = result.sections.find((section) => ['overview', 'context'].includes(section.id) && section.claims.length)
      if (lead) {
        $('#result-title').textContent = lead.claims[0].text
        $('#analysis-heading-evidence').append(evidence(lead.claims[0].evidence.map((e) => `Сообщение ${e.id} · версия ${e.revision}`), 'Основание главного вывода'))
      }
      const questions = result.sections.find((section) => section.id === 'open_questions')
      const steps = result.actions.length
      const questionCount = questions?.claims.length || 0
      summary.append(node('p', `${steps} ${plural(steps, ['следующий шаг', 'следующих шага', 'следующих шагов'])}${questions ? ` · ${questionCount} ${plural(questionCount, ['открытый вопрос', 'открытых вопроса', 'открытых вопросов'])}` : ''}`, 'brief-summary'))
      if (lead && lead.claims.length > 1) {
        const context = node('section', '', 'brief-context')
        for (const item of lead.claims.slice(1)) context.append(claim(item))
        output.append(context)
      }
      if (questionCount) {
        const questionsBlock = node('section', '', 'result-section result-open_questions')
        questionsBlock.setAttribute('aria-label', 'Открытые вопросы')
        for (const item of questions.claims) {
          const block = node('article', '', 'decision-callout')
          block.append(icon('exclamation-circle-fill', 'callout-icon'))
          const content = node('div', '', 'callout-content')
          content.append(node('h4', 'Нужно решение'), node('p', item.text))
          // Claims have no owner/deadline fields. Do not imply those were supplied.
          block.append(content, evidence(item.evidence.map((e) => `Сообщение ${e.id} · версия ${e.revision}`), 'Посмотреть основание'))
          questionsBlock.append(block)
        }
        output.append(questionsBlock)
      }
      const actions = node('section', '', 'result-section result-actions')
      const heading = node('div', '', 'brief-section-heading')
      heading.append(node('h2', 'Что делаем дальше'), node('span', `${steps} ${plural(steps, ['шаг', 'шага', 'шагов'])}`, 'hint'))
      actions.append(heading)
      if (!steps) actions.append(node('p', 'В этом анализе нет подтверждённых задач.', 'hint'))
      else {
        const table = node('table', '', 'action-table')
        const caption = node('caption', 'Следующие шаги, ответственные, сроки и основания', 'sr-only')
        const head = node('thead', '')
        const row = node('tr', '')
        for (const label of ['Задача', 'Кто', 'Когда', 'Основание']) {
          const cell = node('th', label)
          cell.scope = 'col'
          row.append(cell)
        }
        head.append(row)
        const body = node('tbody', '')
        for (const action of result.actions) {
          const card = node('tr', '', 'action-item')
          const task = node('td', action.task.text, 'action-task')
          const owner = node('td', '', 'action-owner')
          owner.append(node('span', 'Ответственный: ', 'sr-only'), node('span', action.owner?.value || 'Не указан'))
          const deadline = node('td', '', 'action-deadline')
          deadline.append(node('span', 'Срок: ', 'sr-only'), node('span', action.deadline?.value || 'Не указан'))
          const citations = action.task.evidence.map((e) => `Сообщение ${e.id} · версия ${e.revision}`)
          for (const [key, label] of [['owner', 'Ответственный'], ['deadline', 'Срок']]) {
            const value = action[key]
            if (value) citations.push(`${label}: «${value.quote}» · ${value.evidence.id}@${value.evidence.revision}`)
          }
          const source = node('td', '', 'action-source')
          source.append(evidence(citations, 'Сообщение'))
          card.append(task, owner, deadline, source)
          body.append(card)
        }
        table.append(caption, head, body)
        actions.append(table)
      }
      output.append(actions)
      const sectionBlock = (section) => {
        const block = node('section', '', `result-section result-${section.id}`)
        block.append(node('h2', section.id === 'decisions' ? 'О чём договорились' : sections[section.id] || section.id))
        if (!section.claims.length) block.append(node('p', 'Нет подтверждённых выводов.', 'hint'))
        for (const item of section.claims) {
          const content = claim(item)
          if (section.id === 'decisions') {
            const row = node('div', '', 'decision-row')
            row.append(icon('check2', 'decision-icon'), content)
            block.append(row)
          } else block.append(content)
        }
        return block
      }
      const order = ['decisions', 'unresolved']
      const emptySections = node('details', '', 'settings-disclosure result-empty-sections')
      emptySections.append(node('summary', 'Другие разделы сводки'))
      for (const section of [...result.sections].filter((item) => item !== lead && item.id !== 'open_questions').sort((a, b) => {
        const rank = (id) => order.includes(id) ? order.indexOf(id) : order.length
        return rank(a.id) - rank(b.id)
      })) {
        // Empty optional recipe sections stay available without displacing the
        // decisions and next steps that a reader came for.
        if (section.claims.length) output.append(sectionBlock(section))
        else emptySections.append(sectionBlock(section))
      }
      if (emptySections.children.length > 1) output.append(emptySections)
    }
    const failures = { authentication: 'Проверьте выбранную авторизацию агента.', transport: 'Не удалось завершить передачу.', invalid_result: 'Ответ не прошёл проверку структуры и ссылок.', timed_out: 'Время ожидания истекло.', interrupted: 'Процесс был прерван.', agent: 'Агент не завершил анализ. Проверьте модель, получателя и выбранную авторизацию.' }
    $('#analysis-result-note').textContent = view.failure ? failures[view.failure] || 'Анализ не завершён.'
      : view.state === 'uncommitted' ? (view.can_commit ? 'Можно принять сохранённый результат без повторного обращения к агенту.' : 'Проект изменился. Сохранённый результат доступен для чтения; подготовьте новый анализ.')
        : view.state === 'interrupted' ? 'Автоматического повтора нет. Закройте запись, затем подготовьте новый запуск.'
          : !navigateToResult && view.result ? 'Это сохранённая сводка. Изменения источников попадут только в новый анализ.' : ''
    $('#btn-analysis-commit').hidden = !view.can_commit
    $('#btn-analysis-close').hidden = view.state !== 'interrupted'
    if (navigateToResult) $('#screen-projects').scrollTop = 0
  }

  $('#analysis-options').addEventListener('input', (event) => {
    if (event.target.id === 'analysis-agent') configureAgent()
    invalidate(); availability()
  })
  for (const [id, kind] of [['#btn-analysis-executable', 'executable'], ['#btn-analysis-auth', 'auth']]) {
    $(id).addEventListener('click', () => act(async () => {
      const selectedAgent = agent().id
      const path = await invoke('pick_analysis_file', { kind, agent: selectedAgent })
      if (path && agent().id === selectedAgent) {
        $(kind === 'executable' ? '#analysis-executable' : '#analysis-auth').value = path
        await invalidate()
      }
    }))
  }
  $('#btn-analysis-prepare').addEventListener('click', () => act(async () => {
    await invalidate()
    const context = bundle()
    if (!context) return
    const options = { agent: agent().id, executable: $('#analysis-executable').value, auth_file: $('#analysis-auth').value,
      model: $('#analysis-model').value.trim(), recipe: $('#analysis-recipe').value, destination: $('#analysis-destination').value }
    if (!options.model) throw new Error(`Укажите модель, доступную в ${agent().title}.`)
    startJob('analysis', 'Проверка настроек анализа без отправки сообщений')
    try {
      reviewed = await invoke('prepare_project_analysis', { projectId: project().project_id, bundleId: context.bundle_id, expectedRevision: context.project_revision, options })
    } finally { endJob(); show('projects') }
    const s = reviewed.spec
    $('#analysis-review-summary').textContent = `${recipes[s.recipe] || s.recipe} · ${s.agent} ${s.agent_version} · модель ${s.model} · сценарий v${s.recipe_version}`
    $('#analysis-review-destination').textContent = s.destination === 'local_fixture' ? 'Получатель: локальный тестовый стенд' : `Кому будут отправлены сообщения: ${s.agent === 'claude' ? 'Anthropic' : 'OpenAI'} · ${s.destination}`
    $('#analysis-review-access').textContent = s.destination === 'local_fixture' ? 'Используются только синтетические данные.'
      : `Программа получит подготовленные сообщения и сможет прочитать выбранный файл входа: ${options.auth_file}. ${agent().title}: ${options.executable}.`
    $('#analysis-review').hidden = false
    $('#btn-analysis-run').disabled = false
    navigate('review')
    $('#analysis-review').scrollIntoView({ block: 'start' })
  }))
  $('#btn-analysis-run').addEventListener('click', () => act(async () => {
    if (!reviewed) return
    const id = reviewed.run_id
    reviewed = null
    $('#btn-analysis-run').disabled = true
    startJob('analysis', 'Анализ выбранных сообщений')
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
  return { reset, invalidate, availability, hasReview: () => reviewed !== null, hasResult: () => selected !== null }
}
