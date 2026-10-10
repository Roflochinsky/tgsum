/* Development-server injection only. NEVER import this file from production UI.
 * Payload shapes follow core/src/{project,scope,bundle,recipe,analysis}.rs and
 * src-tauri/src/{lib,analysis,source_access}.rs. No files/accounts/network used.
 * Latest handover follows the selected daily-brief design without merging recipes.
 * Decision claims expose only text + evidence references, never invented author,
 * timestamp, quote or source URL fields. Action owner/deadline quotes are exact.
 * All mutations below affect this page's memory only and disappear on reload.
 */
(() => {
  'use strict'
  if (window.__TAURI__) throw new Error('Synthetic preview must not replace a native Tauri bridge')
  const scenario = new URLSearchParams(location.search).get('scenario') || 'ready'
  const clone = value => JSON.parse(JSON.stringify(value))
  const evidence = id => ({ id: `demo-message-${id}`, revision: 'demo-revision-1' })
  const messages = [
    ['101', 'Алексей', '2026-10-09 14:20', 'Согласовали: начинаем с одного командного чата. Вложения обсудим отдельно.'],
    ['102', 'Илья', '2026-10-08 09:25', 'Илья: проверю экспорт и список пропусков. Срок: 12 окт.'],
    ['103', 'Мария', '2026-10-08 10:40', 'Мария подготовит тексты пустых состояний. Срок: 13 окт.'],
    ['104', 'Анна', '2026-10-09 15:00', 'Нужно согласовать состав пилота: включать ли вложения? Ответственный и срок пока не согласованы.'],
    ['105', 'Мария', '2026-10-09 15:20', 'Пилот готовится к старту. Обратную связь соберём после первого просмотра сводки.'],
  ].map(([id, sender, timestamp, text]) => ({ reference: evidence(id), sender, timestamp, text }))
  const claim = (text, id) => ({ text, evidence: [evidence(id)] })
  const actions = [
    { task: claim('Проверить экспорт и список пропусков', '102'), owner: { value: 'Илья', quote: messages[1].text, evidence: evidence('102') }, deadline: { value: '12 окт', quote: messages[1].text, evidence: evidence('102') } },
    { task: claim('Подготовить тексты пустых состояний', '103'), owner: { value: 'Мария', quote: messages[2].text, evidence: evidence('103') }, deadline: { value: '13 окт', quote: messages[2].text, evidence: evidence('103') } },
    { task: claim('Согласовать состав пилота', '104'), owner: null, deadline: null },
  ]
  const sectionIds = { summary: ['overview', 'topics', 'open_questions'], decisions: ['decisions', 'reversals', 'open_questions'], actions: ['unresolved'], retro: ['worked', 'failed', 'lessons', 'next_steps'], incident: ['timeline', 'symptoms', 'hypotheses', 'actions_taken', 'resolution'], handover: ['context', 'people', 'systems', 'decisions', 'open_questions'] }
  const claims = {
    overview: [claim('Пилот готовится к старту.', '105')],
    context: [claim('Пилот готовится к старту.', '105')],
    topics: [claim('Начинаем с одного командного чата. Вложения обсудим отдельно.', '101')],
    decisions: [claim('Начинаем с одного командного чата.', '101')],
    open_questions: [claim('Включать ли вложения в пилот?', '104')],
  }
  const privacyOptions = { redact_candidates: false, pii: { categories: [] }, infrastructure: { categories: [], internal_domains: [], hostnames: [] }, keep_values: [] }
  const source = { source_id: 'demo-chat', connector_id: 'telegram_json', scope: { platform: 'telegram', account_local_id: 'Демо-экспорт', conversation_id: '100001' }, archive_path: '/synthetic-only/export/result.json', latest_snapshot_id: 'demo-snapshot', selection: { enabled: true, only_changes: false, filter: { topic_ids: null, dates: { from: '2026-10-05', through: '2026-10-09', basis: 'source_date' }, include_unknown_dates: false, include_service: false }, attachments: null } }
  const project = { schema_version: 11, project_id: 'project-demo-product-pilot', revision: 3, name: 'Командный пилот', sources: [source], settings: { default_agent: 'export_only', default_recipe: 'summary', privacy_preset: 'secrets', max_tokens: 90000 }, analysis_run: null, baselines: ['ready', 'privacy'].includes(scenario) ? [{ source_id: source.source_id, source: source.scope, snapshot_id: source.latest_snapshot_id, filter: source.selection.filter, analysis_id: 'run-demo-analysis-handover' }] : [], custom_terms: { entries: [] }, privacy_options: clone(privacyOptions), assisted_exports: {}, telegram_refresh: {}, telegram_continuous: {} }
  const projects = scenario === 'empty' ? [] : [project]
  const stats = { selected: messages.length, created: 0, edited: 0, deleted: 0, unchanged: messages.length, missing: 0, excluded_unknown_dates: 0, included_unknown_dates: 0, has_baseline: true }
  const coverage = [{ source_id: source.source_id, level: 'partial', known_gaps: 1, only_changes: false, messages: messages.length }]
  function view(recipe = 'summary', id = 'run-demo-analysis-summary') {
    const state = ['failed', 'interrupted', 'uncommitted'].includes(scenario) ? scenario : 'succeeded'
    return { run_id: id, spec: { agent: 'synthetic-codex', agent_version: '1', isolation_profile: 'synthetic-local', destination: 'local_fixture', model: 'fixture-success', recipe, recipe_version: 1 }, coverage, state, failure: state === 'failed' ? 'transport' : null, can_commit: state === 'uncommitted', result: ['failed', 'interrupted'].includes(state) ? null : { recipe, version: 1, sections: sectionIds[recipe].map(id => ({ id, claims: clone(claims[id] || []) })), actions: clone(actions) } }
  }
  const analyses = ['empty', 'no-analysis'].includes(scenario) ? [] : [view('handover', 'run-demo-analysis-handover'), view(), view('decisions', 'run-demo-analysis-decisions')]
  let prepared = null
  let reviewed = null
  const calls = []
  const fail = message => { throw { kind: 'failed', message } }
  const getProject = id => projects.find(p => p.project_id === id) || fail('Демо-проект не найден')
  const checkRevision = (p, revision) => { if (revision !== p.revision) throw { kind: 'conflict', message: 'Проект изменён. Откройте его заново.' } }
  const catalog = { fixtures: true, recipes: Object.keys(sectionIds).map(id => ({ id, title: id[0].toUpperCase() + id.slice(1), version: 1 })), agents: ['codex', 'claude'].map(id => ({ id, title: id === 'codex' ? 'Codex' : 'Claude Code', version: 'synthetic', auth_name: id === 'codex' ? 'auth.json' : '.credentials.json', destinations: [{ id: 'local_fixture', title: 'Локальный тестовый стенд' }], executables: [], available: true })) }
  function bundle(p) {
    const selected = p.sources.filter(s => s.selection.enabled)
    const needsReview = scenario === 'privacy' && !p.privacy_options.redact_candidates ? 1 : 0
    return { bundle_id: `bundle-demo-${p.revision}`, project_revision: p.revision, preview: messages.map(m => `${m.timestamp} · ${m.sender}\n${m.text}\n[${m.reference.id}@${m.reference.revision}]`).join('\n\n'), preview_truncated: false, findings: [], omitted_findings: 0, manifest: { schema_version: 1, sanitizer_version: 'synthetic', destination: 'local_fixture', project_title: p.name, messages: messages.length, sources: selected.map(s => ({ id: s.source_id, title: 'Команда продукта · демо', platform: 'telegram', coverage: 'partial', known_gaps: 1, dates: s.selection.filter.dates, selected_topics: s.selection.filter.topic_ids?.length ?? null, only_changes: s.selection.only_changes, stats })), attachment_references: 0, included_attachments: 0, attachments: [], attachment_choices_outside_scope: 0, privacy: { redacted: needsReview ? 0 : 1, needs_review: needsReview }, files: [{ name: 'context.md', bytes: 1824, sha256: '0'.repeat(64) }] } }
  }
  async function dispatch(command, args = {}) {
    calls.push({ command, args: clone(args) })
    switch (command) {
      case 'pick_export': return scenario === 'export' ? '/demo/export/result.json' : null
      case 'pick_out_dir': return scenario === 'export' ? '/demo/output' : null
      case 'initial_path': case 'next_launch_export': case 'desktop_theme': case 'pick_analysis_file': case 'pick_assisted_path': case 'pick_attachment_root': case 'pick_telegram_log_directory': case 'detect_telegram_client': return null
      case 'discard_analysis_review': case 'cancel_job': reviewed = null; return null
      case 'index_export':
        if (scenario !== 'export' || args.path !== '/demo/export/result.json') fail('Демо-файл недоступен')
        return { path: args.path, fileName: 'result.json', size: 1824, outDir: '/demo/output', chats: [{ chatId: '100001', name: 'Команда продукта · демо', type: 'private_group', count: messages.length, firstDate: '2026-10-08T09:10:00', lastDate: '2026-10-09T15:20:00', topics: [] }] }
      case 'export_selection':
        if (scenario !== 'export' || args.path !== '/demo/export/result.json' || args.outDir !== '/demo/output' || !args.selection?.some(item => item.chatId === '100001')) fail('Нет выбранных демо-сообщений')
        return { outDir: '/demo/output', files: [{ name: 'Команда продукта.md', path: '/demo/output/Команда продукта.md', bytes: 1824 }] }
      case 'export_project_bundle': {
        const p = getProject(args.projectId); checkRevision(p, args.expectedRevision)
        if (scenario !== 'export' || args.outDir !== '/demo/output' || !prepared || prepared.bundle_id !== args.bundleId || prepared.manifest.privacy.needs_review) fail('Сначала проверьте демо-сообщения')
        return { directory: '/demo/output/demo-bundle', files: prepared.manifest.files }
      }
      case 'open_folder': case 'reveal_file':
        if (scenario !== 'export' || !args.path?.startsWith('/demo/output')) fail('В демо нельзя открыть настоящую папку')
        return null
      case 'analysis_catalog': return catalog
      case 'list_projects': return projects.map(project => ({ state: 'ready', project }))
      case 'open_project': return getProject(args.projectId)
      case 'create_project': {
        const p = { ...clone(project), project_id: `project-demo-created-${projects.length + 1}`, name: args.name.trim(), revision: 0, sources: [], baselines: [] }
        if (!p.name) fail('Введите название проекта')
        projects.push(p); return p
      }
      case 'update_project': {
        const p = getProject(args.projectId); checkRevision(p, args.expectedRevision)
        const { kind, value } = args.change
        if (kind === 'rename') p.name = value
        else if (kind === 'selection') {
          const dates = value.selection.filter.dates
          if (dates?.from && dates?.through && dates.from > dates.through) fail('Дата начала должна быть раньше даты окончания')
          p.sources.find(s => s.source_id === value.source_id).selection = clone(value.selection)
        } else if (kind === 'source') {
          const next = clone(value)
          next.selection = { ...clone(source.selection), ...next.selection, filter: { ...clone(source.selection.filter), ...next.selection?.filter } }
          const index = p.sources.findIndex(s => s.source_id === next.source_id)
          if (index < 0) p.sources.push(next); else p.sources[index] = next
        } else if (kind === 'privacy') { p.settings.privacy_preset = value.preset; p.privacy_options = clone(value.options); p.custom_terms = clone(value.custom_terms) }
        else if (kind === 'remove_source') p.sources = p.sources.filter(s => s.source_id !== value)
        else fail(`Демо не поддерживает изменение: ${kind}`)
        p.revision++; prepared = null; reviewed = null; return p
      }
      case 'project_source_accesses': {
        const p = getProject(args.projectId); checkRevision(p, args.expectedRevision)
        return { project_revision: p.revision, sources: p.sources.map(s => ({ source_id: s.source_id, selected_scope: s.scope, method: { kind: 'archive', credentials: 'none', refresh: 'reimport', attachments: 'references_only', assisted_export: true }, attachment_choices: 0, continuous: null })) }
      }
      case 'refresh_project_source': {
        const p = getProject(args.projectId); checkRevision(p, args.expectedRevision)
        const s = p.sources.find(s => s.source_id === args.sourceId) || fail('Демо-источник не найден')
        s.latest_snapshot_id = 'demo-snapshot'; p.revision++; prepared = null; reviewed = null; return p
      }
      case 'preview_project_source': return { snapshot_id: 'demo-snapshot', title: 'Команда продукта · демо', topics: [], stats, coverage: { level: 'partial', reason: 'Синтетический фрагмент переписки', range: null, evidence: [], known_gaps: [] }, baseline_analysis_id: 'run-demo-analysis-handover' }
      case 'privacy_presets': return ['secrets', 'people', 'work', 'custom'].map(id => ({ id, title: id, options: clone(privacyOptions) }))
      case 'attachment_catalog': return { items: [], offset: args.offset || 0, next_offset: null, total: 0 }
      case 'local_package_status': return null
      case 'list_project_analyses': return args.projectId === project.project_id ? analyses.map(v => ({ run_id: v.run_id, state: v.state, agent: v.spec.agent, recipe: v.spec.recipe })) : []
      case 'read_project_analysis': return analyses.find(v => v.run_id === args.runId) || fail('Демо-анализ не найден')
      case 'prepare_project_bundle': { const p = getProject(args.projectId); checkRevision(p, args.expectedRevision); prepared = bundle(p); return prepared }
      case 'review_items': return { items: messages.map(m => ({ reference: m.reference, kind: 'message', label: `${m.sender} · ${m.timestamp}`, source_id: source.source_id })), offset: 0, next_offset: null, total: messages.length }
      case 'preview_evidence': { const m = messages.find(m => m.reference.id === args.reference.id) || fail('Демо-сообщение не найдено'); return { before: m.text, after: m.text, before_state: 'snapshot', before_truncated: false, after_truncated: false } }
      case 'prepare_project_analysis': {
        const p = getProject(args.projectId); checkRevision(p, args.expectedRevision)
        if (!prepared || args.bundleId !== prepared.bundle_id || prepared.manifest.privacy.needs_review) fail('Сначала проверьте подготовленные сообщения')
        if (!['fixture-success', 'fixture-failure'].includes(args.options.model)) fail('В демо допустимы модели fixture-success и fixture-failure')
        reviewed = view(args.options.recipe, `run-demo-${analyses.length + 1}`)
        reviewed.spec.model = args.options.model
        reviewed.spec.agent = `synthetic-${args.options.agent}`
        return { run_id: reviewed.run_id, spec: reviewed.spec, coverage }
      }
      case 'run_project_analysis': {
        if (!reviewed || args.runId !== reviewed.run_id) fail('Подготовьте и проверьте этот запуск заново')
        const result = reviewed; reviewed = null
        if (result.spec.model === 'fixture-failure') { result.state = 'failed'; result.failure = 'transport'; result.result = null }
        analyses.unshift(result); return result
      }
      case 'recover_project_analysis': {
        const result = analyses.find(v => v.run_id === args.runId) || fail('Демо-анализ не найден')
        if (args.commit && result.can_commit) { result.state = 'succeeded'; result.can_commit = false }
        else if (!args.commit && result.state === 'interrupted') result.state = 'cancelled'
        else fail('Недоступное восстановление демо-анализа')
        return result
      }
      case 'telegram_refresh_status': return { project: getProject(args.projectId), available: false, run_id: null, state: { state: 'unavailable' }, decision: null }
      case 'telegram_continuous_status': return { project: getProject(args.projectId), supported: false, client: null, journal_path: null, observation: { phase: 'unavailable', counts_known: true, events: 0, applied_events: 0, gaps: [], last_poll: null } }
      case 'background_status': return { available: false, autostart_enabled: false, keeps_running: false, service_path: null }
      case 'telegram_client_status': return { state: 'unmanaged', can_restore: false }
      default: fail(`Демо: операция «${command}» не выполняется. Файлы, аккаунты и сеть недоступны.`)
    }
  }
  window.__TAURI__ = { core: { invoke: async (command, args) => clone(await dispatch(command, args)) }, event: { listen: async () => () => {} } }
  // Read-only inspection surface for tests; no native bridge is ever available.
  window.__TGSUM_PREVIEW__ = { scenario, calls, messages: clone(messages), snapshot: () => clone({ projects, analyses, prepared, reviewed }) }
  document.addEventListener('DOMContentLoaded', () => {
    const badge = document.createElement('div')
    badge.id = 'synthetic-preview-label'
    badge.textContent = 'Демо · синтетические данные'
    badge.setAttribute('role', 'note')
    badge.title = 'Только проверка интерфейса. Настоящие файлы не читаются и не создаются; аккаунты и сеть не используются.'
    badge.style.cssText = 'position:static;flex:none;background:#fff4cf;color:#433518;border:1px solid #bba868;border-radius:5px;padding:4px 6px;font:600 10px/1.25 system-ui;white-space:nowrap'
    const footer = document.querySelector('.workspace-footer')
    if (footer) {
      // Reserve layout space instead of obscuring the real local-processing and
      // manual-refresh warnings. All preview-only styles stay in this fixture.
      footer.classList.add('synthetic-preview-footer')
      document.body.classList.add('has-synthetic-preview')
      const style = document.createElement('style')
      style.id = 'synthetic-preview-footer-style'
      style.textContent = `
        .workspace-footer.synthetic-preview-footer { gap: 12px; }
        .workspace-footer.synthetic-preview-footer > span { min-width: 0; }
        @media (max-width: 900px) {
          body.has-synthetic-preview { --footer: 64px; }
          .workspace-footer.synthetic-preview-footer { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 4px 10px; padding-block: 8px; font-size: 10px; }
          .workspace-footer.synthetic-preview-footer > span:first-child { grid-column: 1; grid-row: 1; }
          .workspace-footer.synthetic-preview-footer > span:nth-child(2) { grid-column: 1 / -1; grid-row: 2; }
          #synthetic-preview-label { grid-column: 2; grid-row: 1; justify-self: end; }
        }
        @media (max-width: 500px) {
          body.has-synthetic-preview { --footer: 88px; }
          .workspace-footer.synthetic-preview-footer { grid-template-columns: minmax(0, 1fr); }
          #synthetic-preview-label { grid-column: 1; grid-row: 3; justify-self: start; }
        }`
      document.head.append(style)
      footer.append(badge)
    } else document.body.append(badge)
  })
})()
