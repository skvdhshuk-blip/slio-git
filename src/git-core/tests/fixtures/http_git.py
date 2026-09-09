"""Loopback smart HTTP Git fixture. Credentials are disposable test values."""
import base64
import http.server
import os
import pathlib
import subprocess
import sys
import urllib.parse

root, port_file = sys.argv[1:]

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        self.serve_git()

    def do_POST(self):
        self.serve_git()

    def serve_git(self):
        expected = 'Basic ' + base64.b64encode(b'e2e:disposable-token').decode()
        if self.headers.get('Authorization') != expected:
            self.send_response(401)
            self.send_header('WWW-Authenticate', 'Basic realm="git-e2e"')
            self.send_header('Content-Length', '0')
            self.end_headers()
            return
        url = urllib.parse.urlsplit(self.path)
        if self.headers.get('Transfer-Encoding', '').lower() == 'chunked':
            chunks = []
            while True:
                size = int(self.rfile.readline().split(b';')[0], 16)
                if not size:
                    while self.rfile.readline() not in (b'\r\n', b''):
                        pass
                    break
                chunks.append(self.rfile.read(size))
                assert self.rfile.read(2) == b'\r\n'
            body = b''.join(chunks)
        else:
            body = self.rfile.read(int(self.headers.get('Content-Length', '0')))
        env = dict(os.environ, GIT_PROJECT_ROOT=root, GIT_HTTP_EXPORT_ALL='1',
                   PATH_INFO=url.path, QUERY_STRING=url.query,
                   REQUEST_METHOD=self.command, CONTENT_TYPE=self.headers.get('Content-Type', ''),
                   CONTENT_LENGTH=str(len(body)), REMOTE_USER='e2e')
        result = subprocess.run(['git', 'http-backend'], input=body, capture_output=True, env=env, check=True)
        headers, content = result.stdout.split(b'\r\n\r\n', 1)
        pairs = [line.decode().split(':', 1) for line in headers.split(b'\r\n')]
        self.send_response(next((int(v.strip().split()[0]) for k,v in pairs if k.lower() == 'status'), 200))
        for key, value in pairs:
            if key.lower() != 'status':
                self.send_header(key, value.strip())
        self.send_header('Content-Length', str(len(content)))
        self.end_headers()
        self.wfile.write(content)

server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
pathlib.Path(port_file).write_text(str(server.server_port))
server.serve_forever()
