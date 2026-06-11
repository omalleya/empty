# Cart import server

A thin FastAPI service in front of the `kroger` package. This is what the iOS
app's **Send to cart** button and the **"Hey Siri, send my list"** intent POST
to. Running it server-side keeps the family's Kroger OAuth refresh token off the
phone — the app only sends plain item-name strings.

## Run

```bash
uv run --extra server uvicorn server.app:app --host 0.0.0.0 --port 8000
```

Needs the same environment as the CLI (`KROGER_CLIENT_ID/SECRET`,
`KROGER_LOCATION_ID`, `ANTHROPIC_API_KEY`). With uv:

```bash
uv run --env-file .env --extra server uvicorn server.app:app --port 8000
```

Interactive API docs at `http://localhost:8000/docs`.

> **One-time cart login.** The cart push needs a Kroger refresh token. Mint it
> once on the server host by running the CLI interactively (`uv run --env-file .env
> kroger-cart --list somelist.txt` and completing the browser authorization).
> After that the token in `KROGER_TOKEN_STORE` is reused unattended.

## Endpoints

### `POST /cart/import`

```json
// request
{ "items": ["2% milk", "bananas", "2 dozen eggs"] }
```

```json
// response
{
  "added": 2,
  "needs_attention": 1,
  "message": "2 added, 1 need attention",
  "items": [
    { "raw_text": "2% milk", "quantity": 1, "status": "added",
      "chosen": { "upc": "0001111041700", "label": "Kroger 2% Milk — $2.49" },
      "candidates": [], "note": "best size/variety match" },
    { "raw_text": "bananas", "quantity": 1, "status": "added_from_cache",
      "chosen": { "upc": "0000000004011", "label": "Bananas" },
      "candidates": [], "note": "" },
    { "raw_text": "fancy oat milk", "quantity": 1, "status": "needs_confirmation",
      "chosen": null,
      "candidates": [ { "upc": "...", "label": "..." } ],
      "note": "needs confirmation (low confidence)" }
  ]
}
```

`status` is one of `added`, `added_from_cache`, `needs_confirmation`, `no_match`.
Confident items (`added*`) are already in the cart; `needs_confirmation` /
`no_match` items come back with candidates so the app can let the user pick.

The iOS app currently just checks for a 2xx, but this richer shape is what lets
it show "8 added, 1 needs confirmation" — wire `ImportResponse` into
`CartBackend` when you want that.

### `GET /stores/{zip}`

Nearby stores for a zip, to fill `KROGER_LOCATION_ID`.

### `GET /health`

`{ "ok": true }`.

## Point the app at it

Set `CartBackendURL` in `ios/ShoppingList/project.yml` to this server's base URL
(e.g. `https://cart.yourdomain.com`); the app POSTs to `{baseURL}/cart/import`.
