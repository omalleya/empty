"""Typed shapes for the bits of the Kroger API we touch.

These are intentionally partial — the API returns much more per product/location
than we model here. Add fields as you need them.
"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class Location:
    """A Kroger-family store (Fred Meyer, Ralphs, King Soopers, ...)."""

    location_id: str
    name: str
    chain: str = ""
    address: str = ""

    @classmethod
    def from_api(cls, raw: dict) -> "Location":
        # VERIFY: shape per https://developer.kroger.com Locations API
        addr = raw.get("address", {}) or {}
        address = ", ".join(
            p for p in [addr.get("addressLine1"), addr.get("city"), addr.get("state")] if p
        )
        return cls(
            location_id=raw["locationId"],
            name=raw.get("name", ""),
            chain=raw.get("chain", ""),
            address=address,
        )


@dataclass
class Product:
    """One product hit from the Products API."""

    upc: str
    description: str
    brand: str = ""
    size: str = ""
    price: float | None = None
    in_stock: bool | None = None

    @classmethod
    def from_api(cls, raw: dict) -> "Product":
        # VERIFY: Products API nests price/size under `items[0]`.
        items = raw.get("items") or [{}]
        item0 = items[0]
        price_block = item0.get("price") or {}
        # Kroger returns promo + regular; prefer promo when non-zero.
        price = price_block.get("promo") or price_block.get("regular")
        fulfillment = item0.get("fulfillment") or {}
        return cls(
            upc=raw["upc"],
            description=raw.get("description", ""),
            brand=raw.get("brand", ""),
            size=item0.get("size", ""),
            price=float(price) if price else None,
            in_stock=fulfillment.get("inStore") or fulfillment.get("delivery"),
        )

    def label(self) -> str:
        bits = [self.brand, self.description, self.size]
        text = " ".join(b for b in bits if b).strip()
        if self.price is not None:
            text += f" — ${self.price:.2f}"
        return text


@dataclass
class CartItem:
    """A line to push into the cart."""

    upc: str
    quantity: int = 1
    # Kroger supports modality PICKUP / DELIVERY on cart add.
    modality: str = "PICKUP"

    def to_api(self) -> dict:
        return {"upc": self.upc, "quantity": self.quantity, "modality": self.modality}


@dataclass
class ResolvedItem:
    """Result of turning one free-text list line into a concrete product."""

    raw_text: str
    quantity: int = 1
    search_term: str = ""
    chosen: Product | None = None
    candidates: list[Product] = field(default_factory=list)
    from_cache: bool = False
    note: str = ""
