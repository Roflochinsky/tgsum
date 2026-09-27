"""Test-only WebKit inspector client for an explicitly started local Tauri host."""
import base64
import json
import re
import time
import urllib.request
from pathlib import Path
import websocket

class Inspector:
    def __init__(self, port):
        html = urllib.request.urlopen(f'http://127.0.0.1:{port}/').read().decode()
        path = re.search(r'/socket/[^\s\x27]+', html).group()
        self.ws = websocket.create_connection(f'ws://127.0.0.1:{port}' + path, timeout=5)
        self.next_id = 1
        while True:
            message = json.loads(self.ws.recv())
            info = message.get('params', {}).get('targetInfo', {})
            if info.get('type') == 'page':
                self.target = info['targetId']
                break

    def rpc(self, method, params):
        seq = self.next_id
        self.next_id += 1
        self.ws.send(json.dumps({'id':seq, 'method':'Target.sendMessageToTarget', 'params':{
            'targetId':self.target, 'message':json.dumps({'id':seq, 'method':method, 'params':params})}}))
        while True:
            outer = json.loads(self.ws.recv())
            if outer.get('method') != 'Target.dispatchMessageFromTarget':
                if 'error' in outer: raise RuntimeError(outer)
                continue
            message = json.loads(outer['params']['message'])
            if message.get('id') == seq:
                if 'error' in message: raise RuntimeError(message)
                return message['result']

    def evaluate(self, expression):
        result = self.rpc('Runtime.evaluate', {'expression':'((s)=>eval(s))('+json.dumps(expression)+')', 'returnByValue':True})
        if result.get('wasThrown'): raise RuntimeError(result)
        return result.get('result', {}).get('value')

    def wait(self, expression):
        until = time.monotonic()+12
        while time.monotonic() < until:
            if self.evaluate(expression): return
            time.sleep(.05)
        raise AssertionError('timeout: '+expression)

    def click(self, selector):
        self.evaluate(f'document.querySelector({json.dumps(selector)}).click()')

    def screenshot(self, path):
        size = self.evaluate('({width:innerWidth,height:innerHeight})')
        result = self.rpc('Page.snapshotRect', {'x':0,'y':0,**size,'coordinateSystem':'Viewport'})
        Path(path).write_bytes(base64.b64decode(result['dataURL'].split(',',1)[1]))

