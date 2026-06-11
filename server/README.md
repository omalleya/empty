# Cart import server

A thin FastAPI service in front of the `kroger` package. This is what the iOS
app's **Send to cart** button and the **"Hey Siri, send my list"** intent POST
to. Running it server-side keeps the family's Kroger OAuth refresh token off the
phone — the app only sends plain item-name strings.

## Run with Docker (recommended)

```bash
# 1. One-time: authorize the family Kroger account. Publishes 8088 so the OAuth
#    redirect reaches the container; it prints a URL you open in a browser.
docker compose run --rm --service-ports server uv run --frozen kroger-cart --login

# 2. Run the API
docker compose up -d
```

Credentials come from `.env` (`KROGER_CLIENT_ID/SECRET`, `KROGER_LOCATION_ID`,
`ANTHROPIC_API_KEY`). The refresh token and UPC cache persist on the
`kroger-data` volume, so step 1 is genuinely once. API on
`http://localhost:8000` (`/docs` for interactive docs).

## Run without Docker

```bash
# One-time cart login (opens a browser to authorize)
uv run --env-file .env kroger-cart --login

# Serve
uv run --env-file .env --extra server uvicorn server.app:app --port 8000
```

> **Why the one-time login.** The cart push needs a Kroger refresh token, and
> minting it requires a browser authorization that can't happen on a headless
> first boot. `--login` does exactly that and saves the token to
> `KROGER_TOKEN_STORE`; everything after runs unattended.

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
