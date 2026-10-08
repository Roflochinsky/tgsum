import json, os, socket, subprocess, sys, time, uuid
from pathlib import Path
sys.dont_write_bytecode = True
REPO = Path('/home/nikitatrubaev/orca/tgsum')
sys.path.insert(0, str(REPO / 'scripts'))
from ui_webkit import Inspector

PREFIX = Path(__file__).parent
ROOT = PREFIX / 'profile'
BINARY = PREFIX / 'bin/tgsum'
UNIT = 'tgsum-background-check-' + uuid.uuid4().hex + '.service'
BUS_NAME = 'com.roflochinsky.tgsum.SingleInstance'
ENV = dict(os.environ, XDG_RUNTIME_DIR='/run/user/1000',
    DBUS_SESSION_BUS_ADDRESS='unix:path=/run/user/1000/bus', DISPLAY=':0',
    WAYLAND_DISPLAY='wayland-1', XDG_DATA_HOME=str(ROOT/'data'),
    XDG_CONFIG_HOME=str(ROOT/'config'), XDG_CACHE_HOME=str(ROOT/'cache'),
    CARGO_INSTALL_ROOT=str(PREFIX))
for name in ['TGSUM_DESKTOP_E2E_ROOT','TGSUM_ANALYSIS_FIXTURE']:
    ENV.pop(name, None)
with socket.socket() as available:
    available.bind(('127.0.0.1',0))
    PORT = available.getsockname()[1]
ENV['WEBKIT_INSPECTOR_HTTP_SERVER'] = f'127.0.0.1:{PORT}'
report = {'status':'failed','checks':[], 'commit':subprocess.check_output(
    ['git','rev-parse','HEAD'],cwd=REPO,text=True).strip(), 'profile':'owned temporary',
    'backend':'ordinary installed debug binary, no harness/analysis fixture',
    'messenger':'synthetic logs only', 'service':'actual transient user unit'}

def command(args, **kw):
    return subprocess.run(args, env=ENV, text=True, capture_output=True,
        timeout=20, **kw)

def control(*args):
    reply=command(['systemctl','--user','--no-pager',*args])
    assert reply.returncode==0, (args,reply.stdout,reply.stderr)
    return reply.stdout.strip()

def owner():
    reply=command(['busctl','--user','call','org.freedesktop.DBus',
        '/org/freedesktop/DBus','org.freedesktop.DBus',
        'GetConnectionUnixProcessID','s',BUS_NAME])
    return int(reply.stdout.split()[1]) if reply.returncode==0 else None

def pid():
    return int(control('show',UNIT,'-p','MainPID','--value'))

def connect(previous=None):
    deadline=time.monotonic()+30
    while time.monotonic()<deadline:
        current=pid()
        if current and current!=previous:
            assert Path(f'/proc/{current}/exe').resolve()==BINARY.resolve()
            actual_owner=owner()
            if actual_owner is None:
                time.sleep(.1)
                continue
            assert actual_owner==current, 'Refusing another application D-Bus owner'
            try:
                ui=Inspector(PORT)
                ui.wait('!!window.__TAURI__?.core && !!document.body')
                return current,ui
            except (OSError,ConnectionError,AttributeError):pass
        time.sleep(.1)
    raise AssertionError('ordinary installed service startup timeout')

def invoke(ui,name,args=None):
    ui.evaluate('window.__probeReply=null;window.__TAURI__.core.invoke('+
        json.dumps(name)+','+json.dumps(args or {})+
        ').then(value=>window.__probeReply={value},error=>window.__probeReply={error})')
    ui.wait('window.__probeReply!==null')
    result=ui.evaluate('window.__probeReply')
    assert 'error' not in result,(name,result)
    return result['value']

def passed(name):
    report['checks'].append(name)
    print('PASS '+name,flush=True)

def append(message_id,text):
    packet='''[12:00:00.123 00-0000001] (dc:2_main) Recv: { core_message
  msg_id: 7352359257580183524 [LONG],
  seq_no: 1 [INT],
  bytes: 400 [INT],
  body: { updateNewChannelMessage
    message: { message
      flags: 256 [LONG],
      id: MESSAGE_ID [INT],
      peer_id: { peerChannel
        channel_id: 222 [LONG],
      },
      from_id: { peerUser
        user_id: 1 [LONG],
      },
      date: 1781913600 [INT],
      message: MESSAGE_TEXT [STRING],
    },
    pts: 1 [INT],
    pts_count: 1 [INT],
  },
} (dc:2,key:123456,session:987654)
'''.replace('MESSAGE_ID',str(message_id)).replace('MESSAGE_TEXT',json.dumps(text,ensure_ascii=False))
    with (ROOT/'DebugLogs/mtp_12_00.txt').open('a') as file:file.write(packet)

def observed(ui,project_id,count):
    until=time.monotonic()+15
    while time.monotonic()<until:
        view=invoke(ui,'telegram_continuous_status',{'projectId':project_id,'sourceId':'group'})
        if view['observation']['applied_events']==count:return view
        time.sleep(.1)
    raise AssertionError(('installed observation timeout',view))

ui=None
started=False
try:
    assert owner() is None, 'A user application is running; leave it untouched'
    settings=['XDG_RUNTIME_DIR','DBUS_SESSION_BUS_ADDRESS','DISPLAY','WAYLAND_DISPLAY',
        'XDG_DATA_HOME','XDG_CONFIG_HOME','XDG_CACHE_HOME','CARGO_INSTALL_ROOT',
        'WEBKIT_INSPECTOR_HTTP_SERVER']
    launch=['systemd-run','--user','--no-ask-password','--unit='+UNIT,
        '--service-type=exec','--remain-after-exit','--property=Restart=on-failure',
        '--property=TimeoutStopSec=30','--property=RuntimeMaxSec=180',
        '--property=StandardOutput=append:'+str(ROOT/'app.log'),
        '--property=StandardError=append:'+str(ROOT/'app.log')]
    launch += ['--setenv='+key+'='+ENV[key] for key in settings]
    launch += [str(BINARY),'--background']
    result=command(launch)
    assert result.returncode==0,(result.stdout,result.stderr)
    started=True
    first,ui=connect()
    assert invoke(ui,'analysis_catalog')['fixtures'] is False
    assert ui.evaluate('typeof window.__TGSUM_E2E__')=='undefined'
    background=invoke(ui,'background_status')
    assert background['keeps_running'] and not background['window_visible']
    passed('ordinary cargo-installed app starts hidden under actual user service; fixture backend absent')
    project=invoke(ui,'create_project',{'name':'Synthetic installed service'})
    project_id=project['project_id']
    project=invoke(ui,'update_project',{'projectId':project_id,'expectedRevision':project['revision'],
        'change':{'kind':'source','value':{'source_id':'group','connector_id':'telegram_json',
            'scope':{'platform':'telegram','account_local_id':'synthetic','conversation_id':'222'},
            'archive_path':str(ROOT/'full.json'),'latest_snapshot_id':None}}})
    project=invoke(ui,'refresh_project_source',{'projectId':project_id,'sourceId':'group','expectedRevision':project['revision']})
    project=invoke(ui,'set_telegram_continuous',{'request':{'project_id':project_id,'source_id':'group',
        'expected_revision':project['revision'],'enabled':True,
        'input_directory':str(ROOT/'DebugLogs'),'confirmed_single_account':True}})
    append(700,'Synthetic installed service Ж 😀')
    before=observed(ui,project_id,1)
    passed('hidden ordinary installed worker journals and applies selected synthetic native message')
    generation=before['project']['telegram_continuous']['group']['settings']['generation']
    ui.ws.close();ui=None
    control('restart',UNIT)
    second,ui=connect(first)
    after=observed(ui,project_id,1)
    assert after['project']['telegram_continuous']['group']['settings']['generation']==generation
    assert after['observation']['events']==1
    assert any(item['gap']['reason']=='collector_restarted' for item in after['observation']['gaps'])
    assert not invoke(ui,'background_status')['window_visible']
    append(701,'Synthetic after actual service restart')
    after=observed(ui,project_id,2)
    assert after['observation']['events']==2
    passed('actual service restart changes PID, preserves journal/binding and applies next message without duplicate')
    assert owner()==second
    result=command([str(BINARY),'--quit'])
    assert result.returncode==0,(result.stdout,result.stderr)
    until=time.monotonic()+10
    while pid()!=0 and time.monotonic()<until:time.sleep(.1)
    assert pid()==0
    assert control('show',UNIT,'-p','ExecMainStatus','--value')=='0'
    assert owner() is None
    passed('real secondary --quit shuts down installed service app cleanly and releases D-Bus')
    report.update(status='passed',message_count=2,pid_changed=first!=second,
        clean_exit_code=0,real_system_configuration_changed=False,
        limitations=['Transient service; no login/startup qualification','Personal application unchanged',
            'Stock Telegram lifecycle and real incoming message unqualified'])
finally:
    if ui is not None:ui.ws.close()
    if started:
        # This uniquely named service and all its files were created by this probe.
        command(['systemctl','--user','stop',UNIT])
        command(['systemctl','--user','reset-failed',UNIT])
    (PREFIX/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print('Report: '+str(PREFIX/'report.json'),flush=True)
