# Kroger shopping-list → cart importer

Imports a free-text shopping list into the cart of a single Kroger-family account
(Fred Meyer, Ralphs, King Soopers, QFC, …). Built for family use: one shared
login, resolved once.

It uses the **official Kroger public API** — no scraping, no browser automation.
An LLM ([Claude](https://docs.claude.com)) turns messy list lines like
`a couple gallons of 2% milk` into a real product UPC.

## This repo

| Path | What it is |
|------|------------|
| `kroger/` | the core package + `kroger-cart` CLI (this README) |
| `server/` | FastAPI service wrapping the importer — `POST /cart/import` ([server/README.md](server/README.md)) |
| `ios/` | SwiftUI app: shopping list + dictation + Siri, posts to the server ([ios/README.md](ios/README.md)) |

Flow end to end: **iOS app / Siri → `server` → `kroger` package → Kroger cart.**
The CLI below drives the same `kroger` package directly.

## How it works

```
shopping list ──▶ resolver (LLM) ──▶ Kroger Products API ──▶ pick UPC ──▶ Cart API
   text lines        search term         candidates           (cached)      PUT
```

1. **Locations API** — find your store once, save its `locationId`.
2. **Products API** — search each list item (scoped to that store for real
   prices/availability). App-level auth, no login needed.
3. **Resolver (LLM)** — phrases the search term and picks the best matching
   product from the results. Choices are cached in `upc_cache.json`, so each
   item is only disambiguated once.
4. **Cart API** — pushes the chosen UPCs into the family account's cart. This is
   the one step that needs a user login — done once in the browser, then a
   refresh token is saved so you never log in again.

## What you need before running

| Requirement | Why | Where |
|-------------|-----|-------|
| [uv](https://docs.astral.sh/uv/) | env + dependency management | `curl -LsSf https://astral.sh/uv/install.sh \| sh` |
| Python ≥ 3.10 | runtime (uv can install it for you) | `uv python install 3.11` |
| A Kroger account | the family account whose cart gets filled | <https://www.kroger.com> (or fredmeyer.com, etc.) |
| Kroger developer app | API client id + secret | see below |
| Anthropic API key | the LLM resolver | <https://console.anthropic.com> |

### 1. Get Kroger API credentials

1. Go to <https://developer.kroger.com> and sign in (same login as your store
   account works).
2. Create an application (look for **Applications → Add Application** /
   **Create Application**). Fill in:
   - **App name** — anything, e.g. `family-cart`.
   - **Environment** — **Production** (the public API serves real store data).
   - **Redirect URI** — must match `KROGER_REDIRECT_URI` in your `.env`
     **exactly**. Use `http://localhost:8088/callback` (the default this tool
     listens on). A loopback URI is fine for a personal tool.
3. After it's created, copy the **Client ID** and **Client Secret** — these go in
   `.env` as `KROGER_CLIENT_ID` / `KROGER_CLIENT_SECRET`.
4. **Scopes / API access.** This tool uses two scopes:
   - `product.compact` — Products + Locations search. Available to all apps.
   - `cart.basic:write` — adding to the cart. Some accounts need to **request
     access** to the Cart API for the app (there's an option on the app/API
     page). If your first cart push fails with a 403, that's almost always
     missing Cart API access — request it and wait for approval.

> The Client Secret is shown once. If you lose it, regenerate it on the app page
> and update `.env`.

### 2. Get an Anthropic API key

Create one at <https://console.anthropic.com> → **API Keys**. Put it in `.env` as
`ANTHROPIC_API_KEY`. (Used only by the resolver — search-term phrasing and
product selection.)

### 3. Put the credentials in `.env`

```bash
cp .env.example .env
```

Then edit `.env`:

| Variable | Required | What it is |
|----------|----------|------------|
| `KROGER_CLIENT_ID` | ✅ | from your Kroger developer app |
| `KROGER_CLIENT_SECRET` | ✅ | from your Kroger developer app |
| `KROGER_REDIRECT_URI` | ✅ | must match the app's Redirect URI; default `http://localhost:8088/callback` |
| `ANTHROPIC_API_KEY` | ✅ | from console.anthropic.com |
| `KROGER_LOCATION_ID` | ✅ (after step 5) | your store; filled in once you've looked it up |
| `KROGER_TOKEN_STORE` | optional | where the saved refresh token lives (default `~/.kroger/token.json`) |

`.env` is gitignored — credentials never get committed.

### 4. Install

```bash
uv sync   # creates .venv, installs from uv.lock, builds the kroger-cart script
```

### 5. Find your store's locationId

Product prices and availability are per-store, so you need a `locationId`. Look
it up by zip:

```bash
uv run --env-file .env kroger-cart --find-store 97232
```

That prints nearby stores like:

```
70100123  FRED MEYER   Interstate  (3030 NE Weidler St, Portland, OR)
...
```

Copy the id of the store you want into `.env` as `KROGER_LOCATION_ID`. (This step
only needs `product.compact`, so it works before you've sorted out cart access.)

## Usage

`uv sync` installs a `kroger-cart` console script. Pass `--env-file .env` so uv
loads your credentials:

```bash
# Import a list — the FIRST cart push opens a browser to authorize the family
# account once, then saves a refresh token so you never log in again.
uv run --env-file .env kroger-cart --list groceries.txt

# Pipe it instead of using a file
echo "2% milk\nbananas\n2 dozen eggs" | uv run --env-file .env kroger-cart -
```

`groceries.txt` is just one item per line (`2% milk`, `2 dozen eggs`, …).

Items the LLM wasn't confident about are printed with their candidate products so
you can confirm them; confirmed choices get cached in `upc_cache.json` and reused
next time.

> Don't want to pass `--env-file` every time? Either `export $(grep -v '^#' .env | xargs)`
> in your shell first, or run under a tool that auto-loads `.env`. The
> credentials are read from the process environment either way.

## Layout

| File | Purpose |
|------|---------|
| `kroger/auth.py` | OAuth2 — client-credentials (search) + authorization-code w/ saved refresh token (cart) |
| `kroger/client.py` | `find_location` / `search_products` / `add_to_cart` |
| `kroger/resolver.py` | LLM: list line → search term → chosen UPC, with `upc_cache.json` |
| `kroger/models.py` | Typed `Location` / `Product` / `CartItem` |
| `kroger/shopping_list.py` | Orchestration + CLI |

## Status

This is a working skeleton. The Kroger request/response shapes follow the public
API docs but should be verified against live responses — spots that parse the
response payload are marked `# VERIFY`. The OAuth flows, caching, and LLM
resolution are complete.

## Adding other stores later

Safeway/Albertsons has **no public cart API**, so it can't plug into this same
connector. If you add it, expect a separate session-based path (reverse the
internal API, or a browser extension) behind the same `resolve → add_to_cart`
shape.
