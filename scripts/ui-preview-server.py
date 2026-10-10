#!/usr/bin/env python3
"""Serve the actual desktop UI with in-memory synthetic IPC, never a real backend.

Run: python3 scripts/ui-preview-server.py --port 4187
Open /?scenario=ready (default), empty, no-analysis, failed, interrupted,
uncommitted, privacy, or export. Use --materialize DIR with an existing server.
The mock is injected into HTTP responses only. Nothing in src-tauri/ui is patched.
This is a visual/frontend test harness, not evidence that native IPC/Rust works.
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlsplit
import mimetypes
import shutil

ROOT = Path(__file__).resolve().parents[1]
UI = ROOT / 'src-tauri' / 'ui'
FIXTURE = ROOT / 'scripts' / 'ui-preview-fixture.js'
CSP = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; form-action 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'"
INJECTION = '<script src="/__preview__/fixture.js"></script>'


def materialize(destination, freeze=False):
    """Create a preview outside production assets for an existing static server."""
    destination = Path(destination).resolve()
    if destination == ROOT or destination.is_relative_to(UI) or UI.is_relative_to(destination):
        raise ValueError('Preview destination must not contain or be production UI')
    destination.mkdir(parents=True, exist_ok=True)
    for source in UI.iterdir():
        target = destination / source.name
        if source.name == 'index.html':
            if target.is_symlink():
                raise ValueError('Refusing to write through an index.html symlink')
            injection = '<meta http-equiv="Content-Security-Policy" content="' + CSP + '"><script src="preview-fixture.js"></script>'
            target.write_text(source.read_text().replace('<head>', '<head>' + injection, 1))
        elif freeze:
            if target.is_symlink():
                raise ValueError('Frozen preview must not overwrite symlinked source files')
            if source.is_dir():
                shutil.copytree(source, target, dirs_exist_ok=True)
            else:
                shutil.copy2(source, target)
        elif not target.exists():
            target.symlink_to(source, target_is_directory=source.is_dir())
    target = destination / 'preview-fixture.js'
    if not target.exists():
        if freeze:
            shutil.copy2(FIXTURE, target)
        else:
            target.symlink_to(FIXTURE)
    elif freeze and not target.is_symlink():
        shutil.copy2(FIXTURE, target)
    print(f'Synthetic preview materialized: {destination}', flush=True)


class PreviewHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        request_path = unquote(urlsplit(self.path).path)
        if request_path == '/__preview__/fixture.js':
            path = FIXTURE
        else:
            path = (UI / request_path.lstrip('/')).resolve()
            if path == UI:
                path = UI / 'index.html'
            if not path.is_relative_to(UI) or not path.is_file():
                self.send_error(404)
                return
        content = path.read_bytes()
        if path == UI / 'index.html':
            content = content.replace(b'<head>', ('<head>' + INJECTION).encode(), 1)
        content_type = mimetypes.guess_type(path.name)[0] or 'application/octet-stream'
        self.send_response(200)
        self.send_header('Content-Type', content_type + ('; charset=utf-8' if content_type.startswith('text/') or content_type == 'application/javascript' else ''))
        self.send_header('Content-Length', str(len(content)))
        self.send_header('Cache-Control', 'no-store')
        self.send_header('X-Content-Type-Options', 'nosniff')
        # The frontend cannot contact accounts, APIs, sockets, or external hosts.
        self.send_header('Content-Security-Policy', CSP)
        self.end_headers()
        self.wfile.write(content)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, default=4187)
    parser.add_argument('--host', default='127.0.0.1')
    parser.add_argument('--materialize', metavar='DIR', help='Prepare a directory for an existing static server, then exit')
    parser.add_argument('--freeze', action='store_true', help='Copy assets instead of linking them when materializing')
    args = parser.parse_args()
    if args.materialize:
        materialize(args.materialize, args.freeze)
        raise SystemExit(0)
    print(f'Synthetic desktop UI only: http://{args.host}:{args.port}/', flush=True)
    ThreadingHTTPServer((args.host, args.port), PreviewHandler).serve_forever()
