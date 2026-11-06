from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from threading import Thread

import pytest

from git_bot_feedback import GitClient, Request


@pytest.fixture
def local_server():
    received_headers = {}

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            received_headers.update(self.headers.items())
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"ok")

        def log_message(self, format, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = Thread(target=server.serve_forever)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", received_headers
    finally:
        server.shutdown()
        thread.join()
        server.server_close()


@pytest.mark.asyncio
async def test_token_overrides_authorization_header(monkeypatch, local_server):
    url, received_headers = local_server
    monkeypatch.setenv("GITHUB_ACTIONS", "true")
    monkeypatch.setenv("GITHUB_REPOSITORY", "owner/repo")
    monkeypatch.setenv("GITHUB_SHA", "abc123")
    monkeypatch.setenv("GITHUB_API_URL", url)
    monkeypatch.delenv("GITEA_ACTIONS", raising=False)
    monkeypatch.delenv("FORGEJO_ACTIONS", raising=False)
    monkeypatch.delenv("GITLAB_CI", raising=False)
    monkeypatch.delenv("GITHUB_EVENT_NAME", raising=False)
    monkeypatch.delenv("GITHUB_EVENT_PATH", raising=False)

    client = GitClient()
    request = Request(f"{url}/request", headers={"Authorization": "plain"})
    request.set_token("secret")

    response = await client.send_api_request(request)

    assert response.status == 200
    assert response.body == "ok"
    assert received_headers["authorization"] == "token secret"
