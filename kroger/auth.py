"""Kroger OAuth2.

Two grant types, two purposes:

  * client_credentials  -> Products + Locations APIs. App-level, no user login.
                           Scope: "product.compact". Short-lived, fetched on demand.

  * authorization_code  -> Cart API. Needs the *user* (your family account) to
                           authorize once in a browser. Scope: "cart.basic:write".
                           We persist the refresh_token so you only log in once;
                           every later run silently refreshes the access token.

Token endpoint:  https://api.kroger.com/v1/connect/oauth2/token
Authorize URL:   https://api.kroger.com/v1/connect/oauth2/authorize
"""

from __future__ import annotations

import base64
import json
import os
import threading
import time
import urllib.parse
import webbrowser
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import requests

_TOKEN_URL = "https://api.kroger.com/v1/connect/oauth2/token"
_AUTHORIZE_URL = "https://api.kroger.com/v1/connect/oauth2/authorize"

# Where the refresh token lives between runs. Per-user, never committed.
_TOKEN_STORE = Path(os.environ.get("KROGER_TOKEN_STORE", "~/.kroger/token.json")).expanduser()

CART_SCOPE = "cart.basic:write profile.compact"
PRODUCT_SCOPE = "product.compact"


def _basic_auth_header(client_id: str, client_secret: str) -> str:
    raw = f"{client_id}:{client_secret}".encode()
    return "Basic " + base64.b64encode(raw).decode()


@dataclass
class _Token:
    access_token: str
    expires_at: float
    refresh_token: str | None = None

    @property
    def valid(self) -> bool:
        # 30s safety margin.
        return bool(self.access_token) and time.time() < (self.expires_at - 30)


class KrogerAuth:
    def __init__(
        self,
        client_id: str | None = None,
        client_secret: str | None = None,
        redirect_uri: str | None = None,
    ):
        self.client_id = client_id or os.environ["KROGER_CLIENT_ID"]
        self.client_secret = client_secret or os.environ["KROGER_CLIENT_SECRET"]
        self.redirect_uri = redirect_uri or os.environ.get(
            "KROGER_REDIRECT_URI", "http://localhost:8088/callback"
        )
        self._app_token: _Token | None = None      # client_credentials
        self._user_token: _Token | None = None      # authorization_code

    # ------------------------------------------------------------------ #
    # client_credentials — products & locations
    # ------------------------------------------------------------------ #
    def app_token(self, scope: str = PRODUCT_SCOPE) -> str:
        if self._app_token and self._app_token.valid:
            return self._app_token.access_token
        resp = requests.post(
            _TOKEN_URL,
            headers={
                "Authorization": _basic_auth_header(self.client_id, self.client_secret),
                "Content-Type": "application/x-www-form-urlencoded",
            },
            data={"grant_type": "client_credentials", "scope": scope},
            timeout=15,
        )
        resp.raise_for_status()
        self._app_token = self._store_from_response(resp.json(), keep_refresh=False)
        return self._app_token.access_token

    # ------------------------------------------------------------------ #
    # authorization_code — cart (the family account)
    # ------------------------------------------------------------------ #
    def user_token(self) -> str:
        """Return a valid cart access token, refreshing or prompting login as needed."""
        if self._user_token and self._user_token.valid:
            return self._user_token.access_token

        # Try the persisted refresh token first (the common path after first login).
        stored = self._load_stored()
        if stored and stored.refresh_token:
            try:
                self._user_token = self._refresh(stored.refresh_token)
                return self._user_token.access_token
            except requests.HTTPError:
                pass  # refresh expired/revoked -> fall through to interactive login

        # First run (or refresh dead): one-time browser authorization.
        self._user_token = self._interactive_login()
        return self._user_token.access_token

    def _refresh(self, refresh_token: str) -> _Token:
        resp = requests.post(
            _TOKEN_URL,
            headers={
                "Authorization": _basic_auth_header(self.client_id, self.client_secret),
                "Content-Type": "application/x-www-form-urlencoded",
            },
            data={"grant_type": "refresh_token", "refresh_token": refresh_token},
            timeout=15,
        )
        resp.raise_for_status()
        tok = self._store_from_response(resp.json(), keep_refresh=True, fallback_refresh=refresh_token)
        self._persist(tok)
        return tok

    def _interactive_login(self) -> _Token:
        """Open the browser, capture the redirect, exchange the code."""
        state = base64.urlsafe_b64encode(os.urandom(12)).decode()
        params = {
            "scope": CART_SCOPE,
            "response_type": "code",
            "client_id": self.client_id,
            "redirect_uri": self.redirect_uri,
            "state": state,
        }
        url = _AUTHORIZE_URL + "?" + urllib.parse.urlencode(params)
        print("Opening browser to authorize your Kroger account...")
        print("If it doesn't open, paste this into a browser:\n  " + url)
        webbrowser.open(url)

        code = _capture_redirect_code(self.redirect_uri, expected_state=state)

        resp = requests.post(
            _TOKEN_URL,
            headers={
                "Authorization": _basic_auth_header(self.client_id, self.client_secret),
                "Content-Type": "application/x-www-form-urlencoded",
            },
            data={
                "grant_type": "authorization_code",
                "code": code,
                "redirect_uri": self.redirect_uri,
            },
            timeout=15,
        )
        resp.raise_for_status()
        tok = self._store_from_response(resp.json(), keep_refresh=True)
        self._persist(tok)
        print("Authorized. Refresh token saved — you won't need to log in again.")
        return tok

    # ------------------------------------------------------------------ #
    # token (de)serialization
    # ------------------------------------------------------------------ #
    @staticmethod
    def _store_from_response(
        body: dict, keep_refresh: bool, fallback_refresh: str | None = None
    ) -> _Token:
        return _Token(
            access_token=body["access_token"],
            expires_at=time.time() + int(body.get("expires_in", 1800)),
            refresh_token=(body.get("refresh_token") or fallback_refresh) if keep_refresh else None,
        )

    def _persist(self, tok: _Token) -> None:
        _TOKEN_STORE.parent.mkdir(parents=True, exist_ok=True)
        _TOKEN_STORE.write_text(
            json.dumps(
                {"refresh_token": tok.refresh_token, "saved_at": time.time()}, indent=2
            )
        )
        try:
            _TOKEN_STORE.chmod(0o600)
        except OSError:
            pass

    def _load_stored(self) -> _Token | None:
        if not _TOKEN_STORE.exists():
            return None
        try:
            data = json.loads(_TOKEN_STORE.read_text())
        except (json.JSONDecodeError, OSError):
            return None
        if not data.get("refresh_token"):
            return None
        return _Token(access_token="", expires_at=0, refresh_token=data["refresh_token"])


# ---------------------------------------------------------------------- #
# Tiny one-shot loopback server to catch the OAuth redirect.
# ---------------------------------------------------------------------- #
def _capture_redirect_code(redirect_uri: str, expected_state: str, timeout_s: int = 300) -> str:
    parsed = urllib.parse.urlparse(redirect_uri)
    host = parsed.hostname or "localhost"
    port = parsed.port or 8088

    captured: dict[str, str] = {}
    done = threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            qs = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query)
            if "code" in qs:
                captured["code"] = qs["code"][0]
                captured["state"] = (qs.get("state") or [""])[0]
                self.send_response(200)
                self.end_headers()
                self.wfile.write(b"Authorized. You can close this tab.")
            else:
                self.send_response(400)
                self.end_headers()
                self.wfile.write(b"No authorization code in redirect.")
            done.set()

        def log_message(self, *args):  # silence the default stderr logging
            pass

    server = HTTPServer((host, port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        if not done.wait(timeout=timeout_s):
            raise TimeoutError("Timed out waiting for Kroger authorization redirect.")
    finally:
        server.shutdown()

    if captured.get("state") != expected_state:
        raise RuntimeError("OAuth state mismatch — possible CSRF, aborting.")
    return captured["code"]
