"""Thin wrapper over the Kroger public API.

Base URL:  https://api.kroger.com/v1
Endpoints used:
  GET  /locations            find a store by zip
  GET  /products             search by term, scoped to a location
  PUT  /cart/add             add UPCs to the authenticated user's cart
"""

from __future__ import annotations

import os

import requests

from .auth import KrogerAuth
from .models import CartItem, Location, Product

_BASE = "https://api.kroger.com/v1"


class KrogerClient:
    def __init__(self, auth: KrogerAuth | None = None, location_id: str | None = None):
        self.auth = auth or KrogerAuth()
        self.location_id = location_id or os.environ.get("KROGER_LOCATION_ID")
        self._session = requests.Session()

    # ------------------------------------------------------------------ #
    # Locations
    # ------------------------------------------------------------------ #
    def find_location(self, zip_code: str, limit: int = 5) -> list[Location]:
        """Find nearby stores by zip. Pick one and stash its locationId in .env."""
        resp = self._session.get(
            f"{_BASE}/locations",
            headers={"Authorization": f"Bearer {self.auth.app_token()}"},
            params={"filter.zipCode.near": zip_code, "filter.limit": limit},
            timeout=15,
        )
        resp.raise_for_status()
        return [Location.from_api(r) for r in resp.json().get("data", [])]

    # ------------------------------------------------------------------ #
    # Products
    # ------------------------------------------------------------------ #
    def search_products(
        self, term: str, location_id: str | None = None, limit: int = 10
    ) -> list[Product]:
        """Search products for a term, scoped to a store (for price/availability)."""
        loc = location_id or self.location_id
        if not loc:
            raise ValueError(
                "No location_id set. Call find_location() and set KROGER_LOCATION_ID."
            )
        resp = self._session.get(
            f"{_BASE}/products",
            headers={"Authorization": f"Bearer {self.auth.app_token()}"},
            params={
                "filter.term": term,
                "filter.locationId": loc,
                "filter.limit": limit,
            },
            timeout=15,
        )
        resp.raise_for_status()
        return [Product.from_api(r) for r in resp.json().get("data", [])]

    # ------------------------------------------------------------------ #
    # Cart
    # ------------------------------------------------------------------ #
    def add_to_cart(self, items: list[CartItem]) -> None:
        """PUT items into the family account's cart. Requires user auth (one-time login)."""
        if not items:
            return
        resp = self._session.put(
            f"{_BASE}/cart/add",
            headers={
                "Authorization": f"Bearer {self.auth.user_token()}",
                "Content-Type": "application/json",
            },
            json={"items": [i.to_api() for i in items]},
            timeout=15,
        )
        # Success is 204 No Content.
        resp.raise_for_status()
