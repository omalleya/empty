# Kroger shopping-list → cart importer

Imports a free-text shopping list into the cart of a single Kroger-family account
(Fred Meyer, Ralphs, King Soopers, QFC, …). Built for family use: one shared
login, resolved once.

It uses the **official Kroger public API** — no scraping, no browser automation.
An LLM ([Claude](https://docs.claude.com)) turns messy list lines like
`a couple gallons of 2% milk` into a real product UPC.

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

## Setup

Uses [uv](https://docs.astral.sh/uv/) for env + dependency management.

1. Register an app at <https://developer.kroger.com> to get a client id/secret.
   Add a Redirect URI (e.g. `http://localhost:8088/callback`) and request the
   `product.compact` and `cart.basic:write` scopes.
2. `uv sync` (creates `.venv` and installs from `uv.lock`).
3. `cp .env.example .env` and fill it in. The `dev` extra includes
   `python-dotenv`; `uv run` also auto-loads `.env` if you pass `--env-file .env`.

## Usage

`uv sync` installs a `kroger-cart` console script. Run it via `uv run`:

```bash
# Find your store, then paste the locationId into .env (KROGER_LOCATION_ID)
uv run kroger-cart --find-store 97232

# Import a list — first cart push opens the browser to authorize once
uv run kroger-cart --list groceries.txt

# Pipe it instead
echo "2% milk\nbananas\n2 dozen eggs" | uv run kroger-cart -
```

Items the LLM wasn't confident about are printed with their candidates so you
can confirm them; confirmed choices get cached for next time.

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
