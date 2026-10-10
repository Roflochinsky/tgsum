"""Actual Tauri UI regression: use a clean, isolated analysis-fixtures host.

Run: uv run --with websocket-client python scripts/ui-onboarding-smoke.py PORT
The port must belong to your synthetic test app. Never attaches to an agent or
messenger. Native file dialogs are exercised separately by the maintainer.
"""
import json
import sys
from pathlib import Path

from ui_webkit import Inspector


def main():
    port = int(sys.argv[1])
    assert 1024 <= port <= 65535
    ui = Inspector(port)

    # Exercise the visible header controls rather than clicking through a
    # closed disclosure. The project-list action is "All projects" when open.
    click_control = ui.click

    def click(selector):
        if selector == '#btn-projects' and ui.evaluate("document.querySelector('#btn-projects').hidden"):
            selector = '#btn-projects-back'
        menu = ui.evaluate("(() => { const target = document.querySelector(" + json.dumps(selector) + "); const menu = target?.closest('.header-menu'); return menu && !menu.open && !target.closest('summary') ? menu.id : null })()")
        if menu:
            click_control('#' + menu + ' > summary')
            ui.wait('document.getElementById(' + json.dumps(menu) + ').open')
        click_control(selector)

    ui.click = click

    def ev(expression):
        return ui.evaluate(expression)

    def idle():
        ui.wait("document.body.dataset.screen==='projects' && document.querySelector('#screen-projects').getAttribute('aria-busy')==='false'")

    def invoke(command, args=None):
        ev('window.__testReply=null;window.__testError=null;window.__TAURI__.core.invoke(' + json.dumps(command) + ',' + json.dumps(args or {}) + ').then(v=>window.__testReply={value:v}).catch(e=>window.__testError=e)')
        ui.wait('window.__testReply!==null || window.__testError!==null')
        assert ev('window.__testError') is None, command
        return ev('window.__testReply.value')

    def value(selector, content):
        element = 'document.querySelector(' + json.dumps(selector) + ')'
        ev(element + '.value=' + json.dumps(content) + ';' + element + '.dispatchEvent(new Event("input",{bubbles:true}))')

    def stage(name):
        assert ev("document.querySelector('[data-go][aria-current]').dataset.go") == name
        assert ev("[...document.querySelectorAll('[data-project-panel]')].filter(p=>!p.hidden).length") == 1

    # Refuse ordinary builds and nonempty Project stores before any mutation.
    assert invoke('analysis_catalog')['fixtures'] is True, 'Requires synthetic backend'
    assert invoke('list_projects') == [], 'Requires a fresh isolated XDG data directory'
    ev("window.__uiErrors=[];addEventListener('error',e=>__uiErrors.push(e.message));addEventListener('unhandledrejection',e=>__uiErrors.push(String(e.reason)))")
    ui.wait("document.body.dataset.screen==='projects'")
    assert not ev("document.querySelector('#onboarding').open"), 'Startup must leave the project workspace usable without onboarding'
    ui.click('#btn-help')
    ui.wait("document.querySelector('#onboarding').open")
    ui.click('#onboarding-next')
    ui.click('#onboarding-next')
    assert not ev("document.querySelector('#onboarding details').open")
    ui.click('#onboarding details summary')
    ui.wait("document.querySelectorAll('#onboarding-agent option').length===3")
    assert ev("document.querySelector('#onboarding-agent').value") == 'export'
    ui.click('#onboarding-next')
    assert not ev("document.querySelector('#onboarding').open")
    assert ev("document.body.dataset.screen") == 'projects'
    ui.click('#btn-projects')
    idle()
    assert not ev("document.querySelector('#project-home').hidden")
    value('#new-project-name', 'Synthetic onboarding regression')
    ev("document.querySelector('#new-project-form').requestSubmit()")
    idle()
    stage('source')
    assert ev("document.querySelector('#analysis-auth').value") == ''
    project = invoke('list_projects')[0]['project']
    project_id = project['project_id']
    archive = str(Path(__file__).resolve().parents[1] / 'core/tests/fixtures/sample-export.json')
    project = invoke('update_project', {'projectId': project_id, 'expectedRevision': project['revision'], 'change': {'kind': 'source', 'value': {
        'source_id': 'synthetic-chat', 'connector_id': 'telegram_json', 'scope': {'platform': 'telegram', 'account_local_id': 'synthetic', 'conversation_id': '111'}, 'archive_path': archive, 'latest_snapshot_id': None}}})
    invoke('refresh_project_source', {'projectId': project_id, 'expectedRevision': project['revision'], 'sourceId': 'synthetic-chat'})
    ui.click('#btn-projects')
    idle()
    ui.click('[data-project="' + project_id + '"]')
    idle()

    stage('result')
    assert ev("document.querySelector('#analysis-result').hidden")
    assert not ev("document.querySelector('#analysis-history-empty').hidden")
    ui.click('#btn-result-update')
    stage('source')

    # Regression: act() used to rerender on every error, losing these draft dates.
    value('#project-sources [name=from]', '2026-06-20')
    value('#project-sources [name=through]', '2026-06-18')
    ev("document.querySelector('#project-sources form').requestSubmit()")
    idle()
    assert ev("document.querySelector('#toast').classList.contains('error')")
    assert ev("document.querySelector('#project-sources [name=from]').value") == '2026-06-20'
    assert ev("document.querySelector('#project-sources [name=through]').value") == '2026-06-18'
    assert ev("document.querySelector('#btn-project-review').disabled")
    ui.click('#btn-help')
    assert ev("document.querySelector('#onboarding').open")
    ui.click('#onboarding-next')
    ui.click('#onboarding-next')
    ui.click('#onboarding-next')
    assert ev("document.querySelector('#project-sources [name=from]').value") == '2026-06-20'

    # Simulate a second editor. Failed CAS retains draft; reload discards it only
    # after the explicit button and cannot silently reuse an old reviewed Run.
    project = invoke('open_project', {'projectId': project_id})
    invoke('update_project', {'projectId': project_id, 'expectedRevision': project['revision'], 'change': {'kind': 'rename', 'value': 'Synthetic concurrent edit'}})
    value('#project-sources [name=through]', '2026-06-21')
    ev("document.querySelector('#project-sources form').requestSubmit()")
    idle()
    assert not ev("document.querySelector('#project-conflict').hidden")
    assert ev("document.querySelector('#project-sources [name=from]').value") == '2026-06-20'
    ui.click('#btn-project-reload')
    idle()
    assert ev("document.querySelector('#project-conflict').hidden")
    assert ev("document.querySelector('#projects-title').textContent") == 'Synthetic concurrent edit'
    assert ev("document.querySelector('#project-sources [name=from]').value") == ''
    value('#project-sources [name=from]', '2026-06-18')
    value('#project-sources [name=through]', '2026-06-19')
    ev("document.querySelector('#project-sources form').requestSubmit()")
    idle()

    for agent in ['codex', 'claude']:
        ui.click('#project-steps [data-go=source]')
        ui.click('#btn-project-review')
        idle()
        stage('privacy')
        assert 'Чатов: 1' in ev("document.querySelector('#project-review-summary').textContent")
        assert 'Pilot Forum' not in ev("document.querySelector('#project-review-sources').textContent")
        ui.click('#btn-project-destination')
        stage('analyze')
        value('#analysis-agent', 'export')
        assert ev("document.querySelector('#analysis-runtime-fields').hidden")
        assert not ev("document.querySelector('#btn-project-export').disabled")
        value('#analysis-agent', agent)
        value('#analysis-model', 'invalid-fixture-model')
        ui.click('#btn-analysis-prepare')
        idle()
        stage('analyze')
        assert ev("document.querySelector('#analysis-model').value") == 'invalid-fixture-model'
        assert not ev("document.querySelector('#btn-analysis-prepare').disabled")
        value('#analysis-model', 'fixture-success')
        ui.click('#btn-analysis-prepare')
        idle()
        stage('review')
        assert 'synthetic-' + agent in ev("document.querySelector('#analysis-review-summary').textContent")
        ui.click('#btn-analysis-run')
        idle()
        stage('result')
        assert ev("document.querySelector('#analysis-result-status').dataset.state") == 'succeeded'
        assert ev("document.querySelector('#analysis-result-body img')===null")
    ui.click('#btn-projects-back')
    idle()
    assert not ev("document.querySelector('#project-home').hidden")
    assert ev("document.querySelector('#project-detail').hidden")
    assert ev("document.querySelector('#project-list button').dataset.project") == project_id
    saved = invoke('list_project_analyses', {'projectId': project_id})
    ui.click('[data-project="' + project_id + '"]')
    idle()
    stage('result')
    assert ev("document.querySelector('#analysis-result-status').dataset.state") == 'succeeded'
    assert saved[0]['run_id'] in ev("document.querySelector('#analysis-result-origin').textContent")
    assert invoke('list_project_analyses', {'projectId': project_id}) == saved, 'Reopening must not prepare or run analysis'
    assert not ev("document.querySelector('#analysis-history-disclosure').open")
    assert ev("[...document.querySelectorAll('.claim-evidence')].every(d=>!d.open)")
    ui.click('#btn-projects-back')
    idle()
    assert not ev("document.querySelector('#project-home').hidden")
    ui.click('#btn-start')
    ui.wait("document.body.dataset.screen==='start'")
    assert ev('window.__uiErrors') == [], ev('window.__uiErrors')
    print('PASS actual Tauri project-first startup, read-only saved result, project Back, one-off archive, optional help, local Project, draft/error/conflict preservation, scope/privacy, local export choice and both synthetic agent flows')
    ui.ws.close()


if __name__ == '__main__':
    main()
