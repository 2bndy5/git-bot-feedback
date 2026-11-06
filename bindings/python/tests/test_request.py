from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from threading import Thread
from urllib.parse import parse_qs, urlparse

import pytest
from git_bot_feedback import GitClient, Request, RequestError


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


class _Handler(BaseHTTPRequestHandler):
    """Serve `/redirect?to=<url>` as 302 and record headers of other paths."""

    def do_GET(self):
        parsed = urlparse(self.path)
        if parsed.path == "/loop":
            self.send_response(302)
            self.send_header("Location", "/loop")
            self.end_headers()
            return
        if parsed.path == "/redirect":
            self.send_response(302)
            self.send_header("Location", parse_qs(parsed.query)["to"][0])
            self.end_headers()
            return
        self.server.received.append((parsed.path, dict(self.headers.items())))
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"ok")

    def log_message(self, format, *args):
        pass


@contextmanager
def _serve():
    server = ThreadingHTTPServer(("127.0.0.1", 0), _Handler)
    server.received = []
    thread = Thread(target=server.serve_forever)
    thread.start()
    try:
        yield server
    finally:
        server.shutdown()
        thread.join()
        server.server_close()


@pytest.fixture
def gitlab_client(monkeypatch):
    monkeypatch.setenv("GITLAB_CI", "true")
    monkeypatch.setenv("GITLAB_TOKEN", "secret")
    monkeypatch.setenv("CI_API_V4_URL", "http://127.0.0.1:1/api/v4")
    monkeypatch.setenv("CI_PROJECT_ID", "1")
    monkeypatch.setenv("CI_COMMIT_SHA", "abc123")
    monkeypatch.delenv("GITEA_ACTIONS", raising=False)
    monkeypatch.delenv("FORGEJO_ACTIONS", raising=False)
    monkeypatch.delenv("CI_MERGE_REQUEST_IID", raising=False)
    return GitClient()


@pytest.mark.asyncio
async def test_same_origin_redirect_is_followed(gitlab_client):
    with _serve() as server:
        base = f"http://127.0.0.1:{server.server_port}"
        # an arbitrary initial URL is allowed
        request = Request(f"{base}/redirect?to={base}/final")
        request.set_token("secret")
        response = await gitlab_client.send_api_request(request)
    assert response.status == 200
    assert response.body == "ok"
    assert server.received[0][0] == "/final"
    assert server.received[0][1]["private-token"] == "secret"


@pytest.mark.asyncio
async def test_cross_origin_redirect_is_not_followed(gitlab_client):
    with _serve() as origin, _serve() as other:
        # `localhost` and `127.0.0.1` are different origins
        target = f"http://localhost:{other.server_port}/final"
        request = Request(
            f"http://127.0.0.1:{origin.server_port}/redirect?to={target}",
            headers={"PRIVATE-TOKEN": "caller"},
        )
        response = await gitlab_client.send_api_request(request)
    # the 3xx response is returned as is
    assert response.status == 302
    assert response.headers["location"] == target
    assert other.received == []


@pytest.mark.asyncio
async def test_redirect_limit(gitlab_client):
    with _serve() as server:
        base = f"http://127.0.0.1:{server.server_port}"
        with pytest.raises(RequestError):
            await gitlab_client.send_api_request(Request(f"{base}/loop"))


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
