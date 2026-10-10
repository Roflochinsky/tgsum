"""Actual isolated Linux Tauri + native GTK assisted-export regression.

Start cargo run -p tgsum --features analysis-fixtures with fresh XDG directories,
TGSUM_ANALYSIS_FIXTURE=1, GTK_CSD=1, NO_AT_BRIDGE=0 and a local WebKit inspector.
Run: uv run --with websocket-client python scripts/ui-assisted-smoke.py PORT
Requires system Python/PyGObject, Hyprland and wtype; never opens a real client.
"""
import json
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from ui_webkit import Inspector


def main():
    port = int(sys.argv[1])
    assert 1024 <= port <= 65535
    sockets = subprocess.check_output(['ss', '-ltnp', f'sport = :{port}']).decode()
    pid = int(re.search(r'\("tgsum",pid=(\d+)', sockets).group(1))
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
    ev = ui.evaluate

    def idle():
        ui.wait("document.body.dataset.screen==='projects' && document.querySelector('#screen-projects').getAttribute('aria-busy')==='false'")

    def invoke(command, args=None):
        ev('window.__reply=null;window.__error=null;window.__TAURI__.core.invoke(' + json.dumps(command) + ',' + json.dumps(args or {}) + ').then(v=>window.__reply={value:v}).catch(e=>window.__error=e)')
        ui.wait('window.__reply!==null || window.__error!==null')
        assert ev('window.__error') is None, (command, ev('window.__error'))
        return ev('window.__reply.value')

    def picker(selector, path):
        ui.click(selector)
        subprocess.run(['/usr/bin/python', str(Path(__file__).with_name('ui_assisted_picker.py')), str(pid), str(path)], check=True)
        idle()

    assert invoke('analysis_catalog')['fixtures'] is True
    assert invoke('list_projects') == [], 'Fresh isolated Project store required'
    root = Path(tempfile.mkdtemp(prefix='tgsum-assisted-ui-'))
    export = root / 'exports' / 'ChatExport_2026-09-27'
    export.mkdir(parents=True)
    client = root / 'fake client ; $literal'
    subprocess.run(['rustc', str(Path(__file__).resolve().parents[1] / 'src-tauri/tests/fixtures/assisted-client.rs'), '-o', str(client)], check=True)
    archive = {'id':111, 'name':'Synthetic pilot', 'type':'personal_chat', 'messages':[{'id':1, 'type':'message', 'text':'Initial context'}]}
    original = root / 'result.json'
    original.write_text(json.dumps(archive))
    archive['messages'] = [{'id':1,'type':'message','text':'Edited context'}, {'id':2,'type':'message','text':'New context'}]
    completed = export / 'result.json'
    completed.write_text(json.dumps(archive))
    wrong = root / 'wrong.json'
    wrong.write_text(json.dumps({'id':222,'name':'Wrong synthetic chat','type':'personal_chat','messages':[]}))
    invalid_client = root / 'not-a-native-client'
    invalid_client.write_text('#!/bin/sh\nexit 1\n')

    ui.wait("document.body.dataset.screen==='projects'")
    assert not ev("document.querySelector('#onboarding').open")
    ui.click('#btn-help')
    ui.wait("document.querySelector('#onboarding').open")
    for _ in range(3):
        ui.click('#onboarding-next')
    ui.wait("!document.querySelector('#onboarding').open")
    assert ev("document.body.dataset.screen") == 'projects'
    ev("window.__uiErrors=[];addEventListener('error',e=>__uiErrors.push(e.message));addEventListener('unhandledrejection',e=>__uiErrors.push(String(e.reason)))")
    p = invoke('create_project', {'name':'Synthetic assisted export'})
    p = invoke('update_project', {'projectId':p['project_id'],'expectedRevision':p['revision'],'change':{'kind':'source','value':{
        'source_id':'synthetic-chat','connector_id':'telegram_json','scope':{'platform':'telegram','account_local_id':'synthetic-account','conversation_id':'111'},'archive_path':str(original),'latest_snapshot_id':None}}})
    p = invoke('refresh_project_source', {'projectId':p['project_id'],'expectedRevision':p['revision'],'sourceId':'synthetic-chat'})
    project_id = p['project_id']
    previous_snapshot = p['sources'][0]['latest_snapshot_id']
    ui.click('#btn-projects')
    idle()
    ui.click('[data-project="' + project_id + '"]')
    idle()
    ui.click('#project-steps [data-go=source]')
    ui.click('.assisted-export summary')
    picker('[data-assisted-folder]', root / 'exports')
    picker('[data-assisted-pick-client]', client)
    ui.click('[data-assisted-launch]')
    idle()
    deadline = time.monotonic()+5
    while not (root / 'fake-client-launched.txt').exists() and time.monotonic()<deadline:
        time.sleep(.05)
    assert (root / 'fake-client-launched.txt').read_text() == 'arguments=0'
    assert ev("document.querySelector('.assisted-export').dataset.state") == 'needs_user_action'
    assert invoke('open_project', {'projectId':project_id})['sources'][0]['latest_snapshot_id'] == previous_snapshot

    picker('[data-assisted-pick-archive]', wrong)
    assert ev("document.querySelector('[data-assisted-import]').disabled")
    ui.click('[data-assisted-confirm]')
    assert not ev("document.querySelector('[data-assisted-import]').disabled")
    assert ev("document.querySelector('#project-unsaved').hidden")
    ui.click('[data-assisted-import]')
    idle()
    assert invoke('open_project', {'projectId':project_id})['sources'][0]['latest_snapshot_id'] == previous_snapshot
    assert not ev("document.querySelector('[data-assisted-confirm]').checked")
    assert 'Предыдущая копия сообщений сохранена' in ev("document.querySelector('[data-assisted-status]').textContent")

    # An unsupported selected wrapper fails without disabling manual fallback.
    picker('[data-assisted-pick-client]', invalid_client)
    ui.click('[data-assisted-launch]')
    idle()
    assert 'самостоятельно' in ev("document.querySelector('[data-assisted-status]').textContent")
    picker('[data-assisted-pick-archive]', completed)
    ui.click('[data-assisted-confirm]')
    ui.click('[data-assisted-import]')
    idle()
    p = invoke('open_project', {'projectId':project_id})
    assert p['sources'][0]['archive_path'] == str(completed)
    assert p['sources'][0]['latest_snapshot_id'] != previous_snapshot
    assert p['assisted_exports']['synthetic-chat']['directory'] == str(root / 'exports')
    preview = invoke('preview_project_source', {'projectId':project_id,'sourceId':'synthetic-chat'})
    assert preview['stats']['selected'] == 2
    ui.click('#btn-projects')
    idle()
    ui.click('[data-project="' + project_id + '"]')
    idle()
    ui.click('#project-steps [data-go=source]')
    ui.click('.assisted-export summary')
    assert ev("document.querySelector('[data-assisted-directory]').value") == str(root / 'exports')
    assert ev("document.querySelector('[data-assisted-client]').value") == str(invalid_client)
    assert ev("document.querySelector('[data-assisted-archive]').value") == ''
    assert ev("document.querySelector('[data-assisted-import]').disabled")
    ui.click('[data-assisted-clear-client]')
    idle()
    assert ev("document.querySelector('[data-assisted-launch]').disabled")
    assert not ev("document.querySelector('[data-assisted-pick-archive]').disabled")
    assert ev('window.__uiErrors') == []
    ev("document.querySelector('.assisted-export').scrollIntoView({block:'start'})")
    ui.screenshot(root / 'assisted.png')
    print('PASS: native pickers, zero-arg fake launch, wrong scope, failed launch/manual fallback, nested JSON import, persisted settings; screenshot:', root / 'assisted.png')


if __name__ == '__main__':
    main()
