"""A small Kroger (Fred Meyer / Ralphs / etc.) shopping-list -> cart importer.

Pieces:
  - auth.py          OAuth2 (client-credentials for search, auth-code for cart)
  - client.py        thin wrapper over the Kroger public API
  - resolver.py      LLM layer: list item -> search term -> chosen UPC (+ cache)
  - shopping_list.py orchestration / CLI

This is scaffolding. The request/response shapes follow Kroger's public API
docs as of writing, but you'll want to run it against the real API and adjust
the response parsing where marked `# VERIFY`.
"""

from .client import KrogerClient
from .models import Location, Product, CartItem
from .resolver import ListResolver

__all__ = ["KrogerClient", "Location", "Product", "CartItem", "ListResolver"]
