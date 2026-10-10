// Frontend DOM regressions over the actual desktop ES modules and index.html.
// No native IPC, real accounts, inference, filesystem writes, or browser driver.
// Install test dependency outside the repo, then:
// NODE_PATH=/tmp/tgsum-ui-contracts/node_modules node --experimental-vm-modules scripts/ui-contract-tests.mjs
// Optional first argument: another UI directory (e.g. a frozen baseline).
import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import vm from 'node:vm'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
const require = createRequire(import.meta.url)
let JSDOM, VirtualConsole
try { ({ JSDOM, VirtualConsole } = require('jsdom')) }
catch { throw new Error('jsdom is required. Install it in a disposable directory and set NODE_PATH to its node_modules. See the script header.') }
if (!vm.SourceTextModule) throw new Error('Run Node with --experimental-vm-modules')
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const ui = path.resolve(process.argv[2] || path.join(root, 'src-tauri/ui'))
const fixture = await fs.readFile(path.join(root, 'scripts/ui-preview-fixture.js'), 'utf8')
const html = await fs.readFile(path.join(ui, 'index.html'), 'utf8')
const pass = label => console.log(`PASS ${label}`)
const settle = async () => { for (let i = 0; i < 20; i++) await new Promise(resolve => setImmediate(resolve)) }

async function boot(scenario = 'ready', mutateRead = null) {
  const errors = []
  const virtualConsole = new VirtualConsole()
  virtualConsole.on('jsdomError', error => errors.push(error.message))
  const dom = new JSDOM(html, { url: `http://localhost/?scenario=${scenario}`, runScripts: 'outside-only', pretendToBeVisual: true, virtualConsole })
  const { window } = dom
  window.CSS ||= {}
  window.CSS.escape = value => String(value).replace(/[^a-zA-Z0-9_-]/g, '\\$&')
  window.HTMLElement.prototype.scrollIntoView = () => {}
  window.HTMLDialogElement.prototype.showModal = function () { this.open = true }
  window.HTMLDialogElement.prototype.close = function () { this.open = false; this.dispatchEvent(new window.Event('close')) }
  // Timers are irrelevant to these deterministic event-driven tests.
  window.setInterval = () => 0
  window.clearInterval = () => {}
  window.setTimeout = () => 0
  window.clearTimeout = () => {}
  window.addEventListener('error', event => errors.push(event.message))
  window.addEventListener('unhandledrejection', event => errors.push(String(event.reason)))
  window.eval(fixture)
  if (mutateRead) {
    const original = window.__TAURI__.core.invoke
    window.__TAURI__.core.invoke = async (command, args) => {
      const result = await original(command, args)
      return command === 'read_project_analysis' ? mutateRead(result) : result
    }
  }
  window.eval(await fs.readFile(path.join(ui, 'theme.js'), 'utf8'))
  const context = dom.getInternalVMContext()
  const modules = new Map()
  async function load(file) {
    file = path.resolve(file)
    if (modules.has(file)) return modules.get(file)
    const module = new vm.SourceTextModule(await fs.readFile(file, 'utf8'), { context, identifier: file })
    modules.set(file, module)
    await module.link((specifier, referencing) => load(path.resolve(path.dirname(referencing.identifier), specifier)))
    return module
  }
  await (await load(path.join(ui, 'app.js'))).evaluate()
  await settle()
  if (!window.document.querySelector('#synthetic-preview-label')) window.document.dispatchEvent(new window.Event('DOMContentLoaded'))
  const ids = [...window.document.querySelectorAll('[id]')].map(element => element.id)
  assert.equal(new Set(ids).size, ids.length, 'Element IDs must be unique')
  for (const [file] of modules) {
    const source = await fs.readFile(file, 'utf8')
    for (const match of source.matchAll(/['"]#([a-z][a-z0-9-]+)(?=['"\s.\[:])/g)) {
      assert.ok(window.document.getElementById(match[1]), `Missing #${match[1]} referenced by ${path.basename(file)}`)
    }
  }
  const $ = selector => { const node = window.document.querySelector(selector); assert.ok(node, `Missing DOM node ${selector}`); return node }
  const click = async selector => { $(selector).click(); await settle() }
  const input = (selector, value) => { $(selector).value = value; $(selector).dispatchEvent(new window.Event('input', { bubbles: true })) }
  const invoke = window.__TAURI__.core.invoke
  const calls = command => window.__TGSUM_PREVIEW__.calls.filter(call => call.command === command)
  const clean = () => assert.deepEqual(errors, [], `DOM errors: ${errors.join('; ')}`)
  const open = async () => {
    if (!$('#workspace-menu').open) await click('#workspace-menu > summary')
    await click($('#btn-projects').hidden ? '#btn-projects-back' : '#btn-projects')
    await click('[data-project="project-demo-product-pilot"]')
    assert.equal($('#project-detail').hidden, false)
  }
  const history = async () => {
    await click('#project-steps [data-go=result]')
    await click('[data-run="run-demo-analysis-summary"]')
  }
  const stage = name => {
    assert.equal($('[data-go][aria-current]').dataset.go, name)
    assert.equal(window.document.querySelectorAll('[data-project-panel]:not([hidden])').length, 1)
  }
  return { dom, window, $, click, input, invoke, calls, clean, open, history, stage }
}

// The preview is deliberately a dev-only injection, not a production fallback.
assert.ok(!(await fs.readFile(path.join(root, 'src-tauri/ui/index.html'), 'utf8')).includes('preview-fixture'), 'Production HTML must not import preview fixtures')
for (const name of ['app.js', 'projects.js', 'analysis.js']) {
  const source = await fs.readFile(path.join(ui, name), 'utf8')
  assert.ok(!source.includes('__TGSUM_PREVIEW__'), `Production ${name} must not depend on the preview`)
}
pass('mock remains outside production assets')

{
  const t = await boot()
  assert.equal(t.$('#synthetic-preview-label').textContent, 'Демо · синтетические данные')
  assert.equal(t.$('#synthetic-preview-label').parentElement, t.$('.workspace-footer'), 'Demo label reserves its own footer slot')
  assert.equal(t.$('#synthetic-preview-label').style.position, 'static', 'Demo label cannot overlay production warnings')
  assert.equal(t.$('.workspace-footer').querySelectorAll(':scope > span').length, 2, 'Local-processing and manual-refresh warnings remain intact')
  assert.match(t.$('#synthetic-preview-footer-style').textContent, /max-width: 900px/)
  assert.match(t.$('#synthetic-preview-footer-style').textContent, /--footer: 64px/)
  assert.equal(t.window.document.body.dataset.screen, 'projects', 'Startup should land in the project workspace')
  assert.equal(t.$('#onboarding').open, false, 'Help must be optional')
  assert.equal(t.calls('run_project_analysis').length, 0)
  await t.open()
  t.stage('result')
  assert.equal(t.$('#analysis-result').hidden, false, 'Opening a project reads its saved result')
  assert.equal(t.$('#analysis-history-disclosure').open, false)
  assert.equal(t.$('#project-privacy-settings').open, false)
  assert.match(t.$('#analysis-result-summary').textContent, /3/)
  assert.match(t.$('#analysis-result-summary').textContent, /1/)
  assert.equal(t.$('#analysis-result').querySelectorAll('.action-item').length, 3)
  assert.equal(t.$('#analysis-result-body').querySelector('.claim-evidence').open, false)
  assert.equal(t.$('#analysis-agent').value, 'export', 'Default is local export')
  assert.equal(t.$('#analysis-auth').value, '', 'No credential path is silently populated')
  assert.equal(t.$('#btn-analysis-run').disabled, true)
  assert.equal(t.$('#btn-project-export').disabled, true, 'No export before local review')
  assert.equal(t.$('#btn-analysis-prepare').disabled, true)
  await t.history()
  assert.equal(t.$('#analysis-result').hidden, false)
  assert.match(t.$('#analysis-result-body').textContent, /Илья/)
  assert.match(t.$('#analysis-result-body').textContent, /12 окт/)
  assert.match(t.$('#analysis-result-body').textContent, /не указан/i)
  assert.match(t.$('#analysis-result-coverage').textContent, /неполная история/i)
  assert.equal(t.calls('prepare_project_analysis').length, 0, 'Browsing existing result does not prepare a new run')
  assert.equal(t.calls('run_project_analysis').length, 0, 'Browsing existing result does not run analysis')
  const entries = await t.invoke('list_project_analyses', { projectId: 'project-demo-product-pilot' })
  const allowed = { summary: ['overview', 'topics', 'open_questions'], decisions: ['decisions', 'reversals', 'open_questions'], handover: ['context', 'people', 'systems', 'decisions', 'open_questions'] }
  const messages = t.window.__TGSUM_PREVIEW__.messages
  assert.equal(entries[0].recipe, 'handover', 'The editorial landing uses one valid saved handover, not merged analyses')
  const exactKeys = (value, keys, label) => assert.deepEqual(Object.keys(value).sort(), [...keys].sort(), label)
  const checkReference = reference => {
    exactKeys(reference, ['id', 'revision'], 'Evidence refs cannot invent a URL, sender or timestamp')
    assert.ok(messages.some(message => message.reference.id === reference.id && message.reference.revision === reference.revision))
  }
  const checkClaim = item => {
    exactKeys(item, ['text', 'evidence'], 'Claims cannot invent quote, author, time or task state fields')
    assert.ok(item.text && item.evidence.length)
    for (const reference of item.evidence) checkReference(reference)
  }
  for (const entry of entries) {
    const view = await t.invoke('read_project_analysis', { projectId: 'project-demo-product-pilot', runId: entry.run_id })
    exactKeys(view, ['run_id', 'spec', 'coverage', 'state', 'failure', 'result', 'can_commit'], 'AnalysisView remains the actual Rust payload')
    exactKeys(view.result, ['recipe', 'version', 'sections', 'actions'], 'Result remains RecipeOutput')
    assert.equal(view.result.recipe, view.spec.recipe)
    assert.deepEqual(Array.from(view.result.sections, section => section.id), allowed[view.spec.recipe])
    for (const section of view.result.sections) {
      exactKeys(section, ['id', 'claims'], 'Only actual section fields are allowed')
      for (const claim of section.claims) checkClaim(claim)
    }
    for (const action of view.result.actions) {
      exactKeys(action, ['task', 'owner', 'deadline'], 'Actions have no invented status')
      checkClaim(action.task)
      for (const kind of ['owner', 'deadline']) {
        const fact = action[kind]
        if (!fact) continue
        exactKeys(fact, ['value', 'quote', 'evidence'], 'Only owner/deadline facts carry source quotes')
        checkReference(fact.evidence)
        const message = messages.find(message => message.reference.id === fact.evidence.id && message.reference.revision === fact.evidence.revision)
        assert.ok(message?.text.includes(fact.quote))
        assert.ok(fact.quote.includes(fact.value), 'Owner/deadline must be a verbatim evidenced value')
      }
    }
  }
  t.clean(); t.dom.window.close()
  pass('existing overview/actions, coverage, evidence and safe startup defaults')
}

{
  const t = await boot()
  await t.open()
  assert.ok(t.$('.titlebar').contains(t.$('#project-steps')), 'Primary tabs belong in the top navigation')
  assert.deepEqual([...t.$('#project-steps').querySelectorAll('[data-go]')].map(button => button.textContent.trim()), ['Сводка', 'Источники'])
  assert.equal(t.$('#workspace-project-name').textContent, 'Командный пилот')
  assert.equal(t.$('#btn-result-update').hidden, false)
  assert.equal(t.$('#project-flow-steps').hidden, true)
  const result = t.$('[data-project-panel=result]')
  const headline = [...result.querySelectorAll('h1,h2,h3,h4')].find(node => node.textContent.trim() === 'Пилот готовится к старту.')
  assert.ok(headline, 'The saved context claim becomes the editorial headline')
  assert.match(t.$('#analysis-result-summary').textContent, /3.*следующ.*1.*открыт/i)
  assert.ok(!result.textContent.includes('в работе'), 'Assigned actions do not establish an in-progress state')
  assert.ok(!result.textContent.includes('Алексей'), 'Decision references do not contain an author field')
  assert.ok(!result.textContent.includes('14:20'), 'Decision references do not contain a timestamp field')
  assert.ok(!result.textContent.includes('2026-10-05'), 'Current selected dates cannot be presented as a saved run period')
  const table = result.querySelector('table, [role="table"]')
  assert.ok(table, 'Next actions use the selected reference table semantics')
  assert.equal(table.querySelectorAll('.action-item').length, 3)
  for (const label of ['Задача', 'Кто', 'Когда', 'Основание']) assert.ok(table.textContent.includes(label), `Missing action column ${label}`)
  const rows = [...table.querySelectorAll('.action-item')]
  assert.match(rows[0].textContent, /Проверить экспорт и список пропусков/)
  assert.match(rows[0].querySelector('.action-owner').textContent, /^Ответственный:\s*Илья$/)
  assert.match(rows[0].querySelector('.action-deadline').textContent, /^Срок:\s*12 окт$/)
  assert.match(rows[1].textContent, /Подготовить тексты пустых состояний/)
  assert.match(rows[1].textContent, /Мария/)
  assert.match(rows[1].textContent, /13 окт/)
  assert.match(rows[2].textContent, /Согласовать состав пилота/)
  assert.match(rows[2].querySelector('.action-owner').textContent, /^Ответственный:\s*не указан$/i)
  assert.ok(!/не назначен/i.test(rows[2].textContent), 'A missing owner field does not prove nobody was assigned')
  assert.match(rows[2].querySelector('.action-deadline').textContent, /^Срок:\s*не указан$/i)
  const openQuestion = t.$('.result-open_questions')
  const decisions = t.$('.result-decisions')
  assert.match(openQuestion.textContent, /Включать ли вложения в пилот/)
  assert.match(decisions.textContent, /Начинаем с одного командного чата/)
  assert.ok(openQuestion.compareDocumentPosition(table) & t.window.Node.DOCUMENT_POSITION_FOLLOWING, 'Open question is highlighted above next actions')
  assert.ok(table.compareDocumentPosition(decisions) & t.window.Node.DOCUMENT_POSITION_FOLLOWING, 'Agreements follow the next actions')
  assert.equal(result.querySelector('a[href^="https://t.me/"]'), null, 'No fabricated deep-link to source messages')
  const disclosure = rows[0].querySelector('.claim-evidence')
  assert.ok(disclosure)
  assert.equal(disclosure.open, false)
  disclosure.querySelector('summary').click(); await settle()
  assert.equal(disclosure.open, true)
  assert.match(disclosure.textContent, /demo-message-102@demo-revision-1/)
  assert.match(disclosure.textContent, /Илья: проверю экспорт и список пропусков. Срок: 12 окт./)
  assert.equal(t.calls('prepare_project_analysis').length, 0)
  assert.equal(t.calls('run_project_analysis').length, 0)
  await t.history()
  assert.equal(result.querySelector('.result-decisions'), null, 'Switching to summary cannot retain handover decisions')
  assert.equal(result.querySelectorAll('.action-item').length, 3, 'Switching history cannot duplicate actions')
  await t.click('[data-run="run-demo-analysis-decisions"]')
  assert.ok(result.querySelector('.result-decisions'))
  assert.equal(result.querySelectorAll('.action-item').length, 3)
  assert.equal(t.calls('run_project_analysis').length, 0)
  await t.click('#btn-result-update'); t.stage('source')
  assert.equal(t.$('#project-flow-steps').hidden, true)
  assert.equal(t.$('#project-sources .source-options').open, false)
  assert.equal(t.$('#project-privacy-settings').open, false)
  await t.click('#app-menu > summary')
  assert.equal(t.$('#app-menu').open, true)
  await t.click('#btn-show-history'); t.stage('result')
  assert.equal(t.$('#analysis-history-disclosure').open, true)
  assert.equal(t.calls('prepare_project_analysis').length, 0, 'Navigation and New summary do not silently prepare a run')
  assert.equal(t.calls('run_project_analysis').length, 0)
  t.clean(); t.dom.window.close()
  pass('selected daily brief: evidenced headline, question callout, action table and separate agreements')
}

{
  const t = await boot()
  await t.open()
  const projectId = 'project-demo-product-pilot'
  const initial = await t.invoke('open_project', { projectId })
  await t.click('#project-steps [data-go=source]')
  await t.click('#project-sources .source-options > summary')
  t.input('#project-sources [name=from]', '2026-10-06')
  assert.equal(t.$('#project-unsaved').hidden, false)
  for (const selector of ['#workspace-menu', '#app-menu']) {
    await t.click(selector + ' > summary')
    assert.equal(t.$(selector).open, true)
    t.$(selector).dispatchEvent(new t.window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await settle()
    assert.equal(t.$(selector).open, false)
    assert.equal(t.window.document.activeElement, t.$(selector + ' > summary'), 'Escape returns focus to the menu trigger')
    t.stage('source')
    assert.equal(t.$('#project-sources [name=from]').value, '2026-10-06')
    await t.click(selector + ' > summary')
    assert.equal(t.$(selector).open, true)
    await t.click('#project-sources h3')
    assert.equal(t.$(selector).open, false, 'Outside click dismisses the header menu')
    t.stage('source')
  }
  await t.click('#workspace-menu > summary')
  await t.click('#app-menu > summary')
  assert.equal(t.$('#workspace-menu').open, false, 'Opening another header menu dismisses the first')
  assert.equal(t.$('#app-menu').open, true)
  await t.click('#btn-help')
  assert.equal(t.$('#app-menu').open, false, 'Choosing an action closes its menu')
  assert.equal(t.$('#onboarding').open, true)
  t.$('#onboarding').close(); await settle()
  t.stage('source')
  assert.equal(t.$('#project-sources [name=from]').value, '2026-10-06')
  assert.equal((await t.invoke('open_project', { projectId })).revision, initial.revision, 'Menu dismissal does not save or discard the Project')
  t.$('#project-sources form').dispatchEvent(new t.window.Event('submit', { bubbles: true, cancelable: true }))
  await settle()
  const saved = await t.invoke('open_project', { projectId })
  assert.equal(saved.revision, initial.revision + 1)
  assert.equal(saved.sources[0].selection.filter.dates.from, '2026-10-06')
  assert.equal(t.$('#project-unsaved').hidden, true)
  await t.open(); t.stage('result')
  await t.click('#project-steps [data-go=source]')
  assert.equal(t.$('#project-sources [name=from]').value, '2026-10-06', 'Explicitly saved scope survives reopening')
  assert.equal(t.calls('prepare_project_analysis').length, 0)
  assert.equal(t.calls('run_project_analysis').length, 0)
  t.clean(); t.dom.window.close()
  pass('header Escape/outside/action dismissal preserves drafts, focus and explicit Project saving')
}

{
  const t = await boot()
  await t.open(); t.stage('result')
  const projectId = 'project-demo-product-pilot'
  const entries = await t.invoke('list_project_analyses', { projectId })
  const savedRun = await t.invoke('read_project_analysis', { projectId, runId: entries[0].run_id })
  const originalTitle = t.$('#result-title').textContent
  const originalBody = t.$('#analysis-result-body').textContent
  const originalCoverage = t.$('#analysis-result-coverage').textContent
  await t.click('#project-steps [data-go=source]')
  assert.equal(t.$('#project-sources [name=only_changes]').checked, false)
  await t.click('#project-sources [name=only_changes]')
  assert.equal(t.$('#project-sources [name=only_changes]').checked, true)
  assert.equal(t.$('#project-unsaved').hidden, false)
  t.$('#project-sources form').dispatchEvent(new t.window.Event('submit', { bubbles: true, cancelable: true }))
  await settle()
  t.stage('source')
  assert.equal(t.$('#project-unsaved').hidden, true)
  assert.equal((await t.invoke('open_project', { projectId })).sources[0].selection.only_changes, true)
  await t.click('#project-steps [data-go=result]'); t.stage('result')
  assert.equal(t.$('#analysis-result').hidden, false, 'Saving source selection must not leave an existing summary blank')
  assert.equal(t.$('#analysis-history-empty').hidden, true)
  assert.equal(t.$('#result-title').textContent, originalTitle)
  assert.equal(t.$('#analysis-result-body').textContent, originalBody, 'Scope save does not recompute saved conclusions')
  assert.equal(t.$('#analysis-result-coverage').textContent, originalCoverage, 'Saved analysis coverage is not replaced by current selection')
  assert.ok(t.$('#analysis-result-origin').textContent.includes(savedRun.run_id))
  assert.match(t.$('#analysis-result-note').textContent, /сохран/i)
  assert.match(t.$('#analysis-result-note').textContent, /нов.*(?:анализ|сводк)/i, 'Changed source scope requires a new explicit analysis')
  assert.equal(JSON.stringify(await t.invoke('list_project_analyses', { projectId })), JSON.stringify(entries))
  assert.equal(JSON.stringify(await t.invoke('read_project_analysis', { projectId, runId: entries[0].run_id })), JSON.stringify(savedRun))
  for (const command of ['prepare_project_bundle', 'prepare_project_analysis', 'run_project_analysis']) {
    assert.equal(t.calls(command).length, 0, `${command} must not run as a side effect of a scope save`)
  }
  t.clean(); t.dom.window.close()
  pass('scope Save stays in Sources; Summary still reads the saved run and requires explicit reanalysis')
}

{
  const t = await boot()
  await t.open()
  await t.click('#project-steps [data-go=source]')
  t.input('#project-sources [name=from]', '2026-10-12')
  t.input('#project-sources [name=through]', '2026-10-08')
  assert.equal(t.$('#btn-project-review').disabled, true)
  t.$('#project-sources form').dispatchEvent(new t.window.Event('submit', { bubbles: true, cancelable: true }))
  await settle()
  assert.equal(t.$('#project-sources [name=from]').value, '2026-10-12')
  assert.equal(t.$('#project-sources [name=through]').value, '2026-10-08')
  assert.equal(t.$('#project-unsaved').hidden, false)
  assert.equal(t.$('#btn-project-export').disabled, true)
  assert.equal(t.$('#btn-analysis-run').disabled, true)
  assert.equal(t.$('#screen-projects').getAttribute('aria-busy'), 'false')
  assert.equal(t.$('#screen-projects').inert, false)
  assert.ok(t.calls('update_project').length)
  assert.match(t.$('#toast').textContent, /Дата начала/)
  // A concurrent edit must not silently overwrite either the Project or draft.
  const current = await t.invoke('open_project', { projectId: 'project-demo-product-pilot' })
  await t.invoke('update_project', { projectId: current.project_id, expectedRevision: current.revision, change: { kind: 'rename', value: 'Другой сохранённый заголовок' } })
  t.input('#project-sources [name=through]', '2026-10-13')
  t.$('#project-sources form').dispatchEvent(new t.window.Event('submit', { bubbles: true, cancelable: true }))
  await settle()
  assert.equal(t.$('#project-conflict').hidden, false)
  assert.equal(t.$('#project-sources [name=from]').value, '2026-10-12')
  await t.click('#btn-project-reload')
  assert.equal(t.$('#project-conflict').hidden, true)
  assert.equal(t.$('#projects-title').textContent, 'Другой сохранённый заголовок')
  assert.equal(t.$('#project-sources [name=from]').value, '2026-10-05')
  t.clean(); t.dom.window.close()
  pass('invalid scope retains user draft and keeps review/export/run gated')
}

{
  const t = await boot()
  await t.open()
  await t.click('#project-steps [data-go=source]')
  await t.click('#btn-project-review'); t.stage('privacy')
  assert.equal(t.$('#btn-project-export').disabled, false)
  await t.click('#btn-project-destination'); t.stage('analyze')
  assert.equal(t.$('#analysis-agent').value, 'export')
  assert.equal(t.$('#analysis-runtime-fields').hidden, true)
  t.input('#analysis-agent', 'codex'); await settle()
  await t.click('#btn-analysis-prepare'); t.stage('review')
  assert.equal(t.calls('run_project_analysis').length, 0, 'Prepare cannot execute analysis')
  assert.match(t.$('#analysis-review-destination').textContent, /локальный тестовый стенд/)
  assert.equal(t.$('#btn-analysis-run').disabled, false)
  t.input('#analysis-model', 'fixture-failure'); await settle()
  assert.equal(t.$('#btn-analysis-run').disabled, true, 'Editing invalidates the reviewed capability')
  assert.equal(t.$('#analysis-review').hidden, true)
  t.input('#analysis-model', 'fixture-success'); await settle()
  await t.click('#btn-analysis-prepare'); t.stage('review')
  t.$('#btn-analysis-run').click(); t.$('#btn-analysis-run').click(); await settle()
  assert.equal(t.calls('run_project_analysis').length, 1, 'Repeated click cannot run twice')
  t.stage('result')
  assert.equal(t.$('#analysis-result-status').dataset.state, 'succeeded')
  assert.equal(t.$('#analysis-result-note').textContent, '', 'A newly completed analysis clears the saved-scope notice')
  assert.equal(t.$('#btn-analysis-run').disabled, true)
  t.clean(); t.dom.window.close()
  pass('explicit privacy → destination → prepare → confirm; option invalidation and single-use run')
}

{
  const t = await boot('privacy')
  await t.open(); await t.click('#project-steps [data-go=source]'); await t.click('#btn-project-review')
  t.stage('privacy')
  assert.equal(t.$('#btn-project-destination').disabled, true)
  assert.equal(t.$('#btn-project-export').disabled, true)
  t.input('#analysis-agent', 'codex'); await settle()
  assert.equal(t.$('#btn-analysis-prepare').disabled, true)
  assert.equal(t.calls('run_project_analysis').length, 0)
  t.clean(); t.dom.window.close()
  pass('unresolved privacy finding blocks recipient selection, export and analysis')
}

{
  const t = await boot('no-analysis')
  await t.open(); t.stage('result')
  assert.equal(t.$('#analysis-result').hidden, true)
  assert.equal(t.$('#analysis-history-empty').hidden, false)
  assert.equal(t.$('#btn-result-update').disabled, false)
  assert.equal(t.calls('prepare_project_analysis').length, 0)
  assert.equal(t.calls('run_project_analysis').length, 0)
  await t.click('#btn-result-update'); t.stage('source')
  assert.equal(t.$('#btn-project-review').disabled, false)
  t.clean(); t.dom.window.close()
  pass('saved project without analysis opens an actionable empty result')
}

for (const scenario of ['failed', 'interrupted', 'uncommitted']) {
  const t = await boot(scenario)
  await t.open(); await t.history()
  assert.equal(t.calls('run_project_analysis').length, 0)
  if (scenario === 'failed') { assert.match(t.$('#analysis-result-note').textContent, /передач/); assert.equal(t.$('#analysis-result-body').textContent, '') }
  if (scenario === 'interrupted') { assert.equal(t.$('#btn-analysis-close').hidden, false); assert.match(t.$('#analysis-result-note').textContent, /Автоматического повтора нет/) }
  if (scenario === 'uncommitted') assert.equal(t.$('#btn-analysis-commit').hidden, false)
  t.clean(); t.dom.window.close()
  pass(`${scenario} result is explicit, with no automatic retry`)
}

{
  const t = await boot('empty')
  await t.click('#btn-projects')
  assert.equal(t.$('#project-list-empty').hidden, false)
  assert.equal(t.$('#project-home').hidden, false)
  assert.equal(t.$('#project-detail').hidden, true)
  assert.equal(t.calls('run_project_analysis').length, 0)
  t.input('#new-project-name', 'Новый демо-проект')
  t.$('#new-project-form').dispatchEvent(new t.window.Event('submit', { bubbles: true, cancelable: true }))
  await settle()
  assert.equal(t.$('#project-empty').hidden, false)
  t.stage('source')
  assert.equal(t.$('#btn-project-review').disabled, true)
  assert.equal(t.$('#btn-analysis-prepare').disabled, true)
  await t.click('#btn-projects-back')
  assert.equal(t.$('#project-home').hidden, false)
  await t.click('#btn-start')
  assert.equal(t.window.document.body.dataset.screen, 'start')
  await t.click('#btn-help')
  assert.equal(t.$('#onboarding').open, true)
  await t.click('#onboarding-next'); await t.click('#onboarding-next'); await t.click('#onboarding-next')
  assert.equal(t.$('#onboarding').open, false)
  assert.equal(t.window.document.body.dataset.screen, 'start')
  t.clean(); t.dom.window.close()
  pass('empty projects, new-project setup, Back, one-off archive and optional Help')
}

{
  const t = await boot('export')
  await t.click('#btn-start'); await t.click('#dropzone')
  assert.equal(t.window.document.body.dataset.screen, 'select')
  assert.equal(t.$('#file-name').textContent, 'result.json')
  assert.equal(t.$('#btn-next').disabled, true)
  await t.click('[data-key="c:100001"]')
  assert.equal(t.$('#btn-next').disabled, false)
  await t.click('#btn-next')
  assert.equal(t.window.document.body.dataset.screen, 'save')
  await t.click('#btn-pick-dir')
  await t.click('#btn-export')
  assert.equal(t.window.document.body.dataset.screen, 'done')
  assert.match(t.$('#done-sub').textContent, /\/demo\/output/)
  assert.equal(t.$('#done-files').children.length, 1)
  assert.equal(t.calls('export_selection').length, 1)
  assert.equal(t.calls('export_selection')[0].args.selection[0].chatId, '100001')
  assert.equal(t.calls('run_project_analysis').length, 0)
  await t.open(); await t.click('#project-steps [data-go=source]'); await t.click('#btn-project-review')
  t.stage('privacy'); await t.click('#btn-project-destination'); t.stage('analyze')
  assert.equal(t.$('#analysis-agent').value, 'export')
  await t.click('#btn-project-export'); t.stage('result')
  assert.equal(t.calls('export_project_bundle').length, 1)
  assert.equal(t.$('#project-export-result').hidden, false)
  assert.match(t.$('#project-export-result').textContent, /\/demo\/output\/demo-bundle/)
  assert.equal(t.$('#btn-project-open-export').hidden, false)
  assert.equal(t.calls('prepare_project_analysis').length, 0)
  assert.equal(t.calls('run_project_analysis').length, 0)
  t.clean(); t.dom.window.close()
  pass('synthetic one-off archive → selection → save → done and reviewed Project export')
}

{
  const t = await boot('ready')
  await t.open(); await t.click('#project-steps [data-go=source]'); await t.click('#btn-project-review')
  await t.click('#btn-project-destination'); await t.click('#btn-project-export')
  assert.equal(t.calls('pick_out_dir').length, 1)
  assert.equal(t.calls('export_project_bundle').length, 0)
  assert.equal(t.$('#project-export-result').hidden, true)
  assert.equal(t.$('#screen-projects').getAttribute('aria-busy'), 'false')
  await t.click('#btn-start'); await t.click('#dropzone')
  assert.equal(t.window.document.body.dataset.screen, 'start')
  assert.equal(t.calls('index_export').length, 0)
  t.clean(); t.dom.window.close()
  pass('cancelled native pickers leave the previous state safe and do not fake a saved file')
}

{
  const attack = '<img src=x onerror="window.__unsafeExecuted=true">'
  const t = await boot('ready', view => { if (view.result) view.result.sections[0].claims[0].text = attack; return view })
  await t.open(); await t.history()
  assert.ok(t.$('[data-project-panel=result]').textContent.includes(attack))
  assert.equal(t.$('[data-project-panel=result]').querySelector('img'), null)
  assert.equal(t.window.__unsafeExecuted, undefined)
  await assert.rejects(t.invoke('launch_assisted_client', {}), error => error.kind === 'failed')
  await assert.rejects(t.invoke('configure_local_package', {}), error => error.kind === 'failed')
  t.clean(); t.dom.window.close()
  pass('untrusted claims remain text; preview denies native/external mutations')
}
console.log('PASS all frontend DOM contracts. Native Tauri/Rust integration remains a separate required check.')
