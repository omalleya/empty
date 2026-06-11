"""FastAPI service in front of the `kroger` package.

Endpoints:
  POST /cart/import      {"items": ["2% milk", ...]}  -> resolve + add to cart
  GET  /stores/{zip}     nearby store locationIds (to fill KROGER_LOCATION_ID)
  GET  /health

This is what the iOS app's "Send to cart" / Siri intent talks to. Running it
server-side keeps the family's Kroger OAuth refresh token off the phone — the
app only ever sends plain item-name strings.

Run:
  uv run --extra server uvicorn server.app:app --host 0.0.0.0 --port 8000

Operational notes:
  * Needs the same env as the CLI: KROGER_CLIENT_ID/SECRET, KROGER_LOCATION_ID,
    ANTHROPIC_API_KEY (see .env.example).
  * The cart push needs a one-time interactive Kroger login to mint a refresh
    token. Do it once on the server host (e.g. run the CLI `--list` flow), then
    KROGER_TOKEN_STORE holds the refresh token for unattended use.
"""

from __future__ import annotations

from functools import lru_cache

import requests
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field

from kroger import KrogerClient
from kroger.models import ResolvedItem
from kroger.shopping_list import import_list

app = FastAPI(title="Kroger Cart Import", version="0.1.0")


@lru_cache(maxsize=1)
def _client() -> KrogerClient:
    """One shared client so OAuth tokens are reused across requests."""
    return KrogerClient()


# --------------------------------------------------------------------------- #
# Schemas
# --------------------------------------------------------------------------- #
class ImportRequest(BaseModel):
    items: list[str] = Field(..., description="Free-text shopping-list lines.")


class ProductOut(BaseModel):
    upc: str
    label: str


class ItemResult(BaseModel):
    raw_text: str
    quantity: int
    # added | added_from_cache | needs_confirmation | no_match
    status: str
    chosen: ProductOut | None = None
    candidates: list[ProductOut] = []
    note: str = ""


class ImportResponse(BaseModel):
    added: int
    needs_attention: int
    message: str
    items: list[ItemResult]


def _to_result(item: ResolvedItem) -> ItemResult:
    if item.chosen:
        status = "added_from_cache" if item.from_cache else "added"
        chosen = ProductOut(
            upc=item.chosen.upc,
            label=item.chosen.description or item.chosen.label(),
        )
    else:
        status = "no_match" if not item.candidates else "needs_confirmation"
        chosen = None
    return ItemResult(
        raw_text=item.raw_text,
        quantity=item.quantity,
        status=status,
        chosen=chosen,
        candidates=[ProductOut(upc=p.upc, label=p.label()) for p in item.candidates[:5]],
        note=item.note,
    )


# --------------------------------------------------------------------------- #
# Routes
# --------------------------------------------------------------------------- #
@app.get("/health")
def health() -> dict:
    return {"ok": True}


@app.get("/stores/{zip_code}")
def stores(zip_code: str) -> list[dict]:
    try:
        locations = _client().find_location(zip_code)
    except requests.HTTPError as exc:
        raise HTTPException(status_code=502, detail=f"Kroger error: {exc}") from exc
    return [
        {
            "location_id": loc.location_id,
            "name": loc.name,
            "chain": loc.chain,
            "address": loc.address,
        }
        for loc in locations
    ]


@app.post("/cart/import", response_model=ImportResponse)
def cart_import(req: ImportRequest) -> ImportResponse:
    if not req.items:
        raise HTTPException(status_code=400, detail="No items provided.")

    try:
        results = import_list(req.items, client=_client())
    except ValueError as exc:  # e.g. missing KROGER_LOCATION_ID
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except requests.HTTPError as exc:
        code = exc.response.status_code if exc.response is not None else 502
        if code in (401, 403):
            detail = (
                "Cart authorization failed. The server needs a one-time Kroger login "
                "to save a refresh token, and the app must have Cart API access."
            )
        else:
            detail = f"Kroger API error ({code})."
        raise HTTPException(status_code=502, detail=detail) from exc

    items = [_to_result(r) for r in results]
    added = sum(1 for i in items if i.status.startswith("added"))
    needs = len(items) - added
    message = f"{added} added" + (f", {needs} need attention" if needs else "")
    return ImportResponse(added=added, needs_attention=needs, message=message, items=items)
