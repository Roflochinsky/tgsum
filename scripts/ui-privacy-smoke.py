"""Privacy flow in a fresh, isolated analysis-fixtures Tauri host.

Run with the test host's inspector port. Uses generated archives/files only;
native folder selection is exercised separately. Never uses a real account.
"""
import json
import sys
import tempfile
from pathlib import Path

from ui_webkit import Inspector


def main():
    ui = Inspector(int(sys.argv[1]))
    ev = ui.evaluate

    def invoke(command, args=None):
        ev('window.__reply=null;window.__err=null;window.__TAURI__.core.invoke(' + json.dumps(command) + ',' + json.dumps(args or {}) + ').then(v=>window.__reply={value:v}).catch(e=>window.__err=e)')
        ui.wait('window.__reply!==null || window.__err!==null')
        assert ev('window.__err') is None, (command, ev('window.__err'))
        return ev('window.__reply.value')

    def idle():
        ui.wait("document.body.dataset.screen==='projects' && document.querySelector('#screen-projects').getAttribute('aria-busy')==='false'")

    def value(selector, text, event='input'):
        target = 'document.querySelector(' + json.dumps(selector) + ')'
        ev(target + '.value=' + json.dumps(text) + ';' + target + '.dispatchEvent(new Event(' + json.dumps(event) + ',{bubbles:true}))')

    def content(selector):
        return ev('document.querySelector(' + json.dumps(selector) + ').textContent')

    assert invoke('analysis_catalog')['fixtures'] is True
    assert invoke('list_projects') == [], 'Requires a fresh isolated XDG profile'
    ev("window.__uiErrors=[];addEventListener('error',e=>__uiErrors.push(e.message));addEventListener('unhandledrejection',e=>__uiErrors.push(String(e.reason)))")
    ui.wait("document.querySelector('#onboarding').open")
    for _ in range(3):
        ui.click('#onboarding-next')
    ui.wait("!document.querySelector('#onboarding').open")

    with tempfile.TemporaryDirectory(prefix='tgsum-privacy-fixture-') as temp:
        root = Path(temp)
        original = 'FILE password=SYNTHETIC_FILE_SECRET alice@example.test Project Orion 10.20.30.40'
        (root / 'note.log').write_text(original)
        archive = root / 'result.json'
        archive.write_text(json.dumps({'id': 1, 'name': 'Synthetic privacy', 'type': 'private_group', 'messages': [
            {'id': 1, 'type': 'message', 'date': '2026-09-01T12:00:00', 'from': 'Alice Example', 'from_id': 'user1',
             'text': 'MESSAGE password=SYNTHETIC_MESSAGE_SECRET alice@example.test Project Orion 10.20.30.40', 'file': 'note.log'},
            {'id': 2, 'type': 'message', 'date': '2026-09-01T12:01:00', 'from': 'Alice Example', 'from_id': 'user1',
             'text': 'Second message key=REVIEW_VALUE', 'file': 'missing.txt'},
            {'id': 3, 'type': 'message', 'date': '2026-09-01T12:02:00', 'text': 'Archive reference', 'file': 'archive.zip'},
        ]}))
        project = invoke('create_project', {'name': 'Synthetic privacy review'})
        project_id = project['project_id']

        def current():
            return invoke('open_project', {'projectId': project_id})

        def update(change):
            return invoke('update_project', {'projectId': project_id, 'expectedRevision': current()['revision'], 'change': change})

        project = update({'kind': 'source', 'value': {'source_id': 'work', 'connector_id': 'telegram_json',
            'scope': {'platform': 'telegram', 'account_local_id': 'synthetic', 'conversation_id': '1'},
            'archive_path': str(archive), 'latest_snapshot_id': None}})
        project = invoke('refresh_project_source', {'projectId': project_id, 'expectedRevision': project['revision'], 'sourceId': 'work'})
        catalog = invoke('attachment_catalog', {'projectId': project_id, 'expectedRevision': project['revision'], 'sourceId': 'work', 'offset': 0})
        selection = project['sources'][0]['selection']
        selection['attachments'] = {'root': str(root), 'files': [catalog['items'][0]['choice']]}
        update({'kind': 'selection', 'value': {'source_id': 'work', 'selection': selection}})
        ui.click('#btn-projects')
        idle()
        ui.click('[data-project="' + project_id + '"]')
        idle()
        assert ev("document.querySelectorAll('[data-privacy-group]').length") == 11
        assert ev("document.querySelectorAll('[data-attachment-choice]').length") == 3
        assert ev("document.querySelectorAll('[data-attachment-choice]:checked').length") == 1
        assert ev("document.querySelectorAll('[data-attachment-choice]:disabled').length") == 1
        ev("document.querySelector('.attachment-selector').open=true")
        ui.click('[data-attachment-choice="[\\\"2\\\",0]"]')
        value('#privacy-preset', 'work', 'change')
        value('#privacy-terms', 'Project Orion')
        value('#privacy-exceptions', 'alice@example.test')
        # Saving source scope must keep the selected files AND the privacy draft.
        ev("document.querySelector('#project-sources form').requestSubmit()")
        idle()
        assert len(current()['sources'][0]['selection']['attachments']['files']) == 2
        assert ev("document.querySelector('#privacy-preset').value") == 'work'
        assert ev("document.querySelector('#privacy-terms').value") == 'Project Orion'
        ui.click('#btn-project-review')
        idle()
        assert ev("document.querySelector('#project-steps [aria-current]').dataset.go") == 'privacy'
        assert current()['settings']['privacy_preset'] == 'work'
        summary = content('#project-review-summary')
        assert 'вложений включено: 1' in summary and 'невключённые вложения: 2' in summary, summary
        assert 'Отсутствующих выбранных файлов: 1' in content('#privacy-file-gaps')
        assert ev("document.querySelectorAll('#privacy-reviewed-files li').length") >= 2
        assert 'PII:' in content('#project-review-privacy')
        assert 'Требуют решения: 1.' in content('#project-review-privacy')
        assert ev("document.querySelector('#btn-project-destination').disabled")
        assert ev("document.querySelector('#btn-project-export').disabled")
        ui.click('#btn-privacy-settings')
        ui.click('#project-redact-candidates')
        ui.click('#btn-project-review')
        idle()
        assert 'Требуют решения: 0.' in content('#project-review-privacy')
        assert ev("document.querySelector('#privacy-comparison-panes').hidden")
        ev("document.querySelector('#privacy-review-item').value=[...document.querySelector('#privacy-review-item').options].find(o=>o.textContent.startsWith('Сообщение: 1')).value")
        ui.click('#privacy-compare')
        idle()
        assert 'SYNTHETIC_MESSAGE_SECRET' in content('#privacy-before')
        after = content('#privacy-after')
        assert 'SYNTHETIC_MESSAGE_SECRET' not in after
        assert 'alice@example.test' in after and 'Project Orion' not in after and '10.20.30.40' not in after
        ev("document.querySelector('#privacy-review-item').value=[...document.querySelector('#privacy-review-item').options].find(o=>o.textContent.startsWith('Файл:')).value")
        ui.click('#privacy-compare')
        idle()
        assert content('#privacy-before') == original
        sanitized_file = content('#privacy-after')
        assert 'SYNTHETIC_FILE_SECRET' not in sanitized_file
        (root / 'note.log').write_text('Changed since review')
        ui.click('#privacy-compare')
        idle()
        assert 'изменился' in content('#privacy-before')
        assert content('#privacy-after') == sanitized_file
        ev("document.querySelector('#privacy-comparison').scrollIntoView({block:'start'})")
        ui.screenshot('/tmp/tgsum-privacy-review.png')

        # A prepared synthetic Run must be invalidated before a changed policy
        # can reach either export or execution, including unsaved edits.
        ui.click('#btn-project-destination')
        value('#analysis-agent', 'codex')
        value('#analysis-model', 'fixture-success')
        ui.click('#btn-analysis-prepare')
        idle()
        assert not ev("document.querySelector('#btn-analysis-run').disabled")
        ui.click('#project-steps [data-go=source]')
        value('#privacy-preset', 'people', 'change')
        assert ev("document.querySelector('#btn-analysis-run').disabled")
        assert ev("document.querySelector('#btn-project-export').disabled")
        assert ev("document.querySelector('#privacy-before').textContent") == ''
        ev("document.querySelector('#privacy-settings').requestSubmit()")
        idle()
        assert current()['settings']['privacy_preset'] == 'people'
        # Invalid policy retains the user's draft and leaves the saved revision.
        revision = current()['revision']
        value('#privacy-exceptions', 'duplicate\nduplicate')
        ev("document.querySelector('#privacy-settings').requestSubmit()")
        idle()
        assert current()['revision'] == revision
        assert ev("document.querySelector('#privacy-exceptions').value") == 'duplicate\nduplicate'
        assert ev("document.querySelector('#toast').classList.contains('error')")
        ui.click('#btn-projects')
        idle()
        ui.click('[data-project="' + project_id + '"]')
        idle()
        assert ev("document.querySelector('#privacy-preset').value") == 'people'
        assert ev("document.querySelector('#privacy-exceptions').value") == 'alice@example.test'
        assert ev("document.querySelectorAll('[data-attachment-choice]:checked').length") == 2
        ev("document.querySelector('#privacy-settings').scrollIntoView({block:'start'})")
        ui.screenshot('/tmp/tgsum-privacy-settings.png')
        assert ev('window.__uiErrors') == [], ev('window.__uiErrors')
    print('PASS actual Tauri privacy presets, saved files/drafts, counts, local before/after, changed file, Run invalidation and validation errors')
    ui.ws.close()


if __name__ == '__main__':
    main()
