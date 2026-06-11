"""Orchestration + a small CLI.

Usage:
  # one-time: find your store and copy the locationId into .env
  python -m kroger.shopping_list --find-store 97232

  # import a list (one item per line) into the family cart
  python -m kroger.shopping_list --list groceries.txt

  # or pipe it
  echo "2% milk\\nbananas\\n2 dozen eggs" | python -m kroger.shopping_list -
"""

from __future__ import annotations

import argparse
import sys

from .client import KrogerClient
from .models import CartItem, ResolvedItem
from .resolver import ListResolver


def import_list(lines: list[str], client: KrogerClient | None = None) -> list[ResolvedItem]:
    """Resolve free-text lines to products and push the confident ones to the cart."""
    client = client or KrogerClient()
    resolver = ListResolver(client)

    items = resolver.resolve(lines)

    cart = [
        CartItem(upc=i.chosen.upc, quantity=i.quantity) for i in items if i.chosen
    ]
    if cart:
        client.add_to_cart(cart)
    return items


def _print_summary(items: list[ResolvedItem]) -> None:
    added, unresolved = 0, 0
    for it in items:
        if it.chosen:
            tag = "cache" if it.from_cache else "added"
            print(f"  [{tag}] {it.raw_text!r} x{it.quantity} -> {it.chosen.description}")
            added += 1
        else:
            unresolved += 1
            print(f"  [skip ] {it.raw_text!r} -> {it.note}")
            for n, p in enumerate(it.candidates[:5]):
                print(f"           {n}: {p.label()}")
    print(f"\n{added} added, {unresolved} need attention.")


def _read_lines(source: str) -> list[str]:
    if source == "-":
        raw = sys.stdin.read()
    else:
        with open(source) as f:
            raw = f.read()
    return [ln.strip() for ln in raw.splitlines() if ln.strip()]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Import a shopping list into a Kroger cart.")
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--find-store", metavar="ZIP", help="List nearby stores for a zip code.")
    g.add_argument("--list", metavar="FILE", help="File of shopping-list lines (or - for stdin).")
    g.add_argument(
        "--login",
        action="store_true",
        help="One-time Kroger cart authorization (saves a refresh token). "
        "Needed once before the server can push to the cart unattended.",
    )
    g.add_argument("source", nargs="?", help="Same as --list; '-' reads stdin.")
    args = ap.parse_args(argv)

    client = KrogerClient()

    if args.login:
        client.auth.user_token()  # triggers the browser flow + persists the token
        print("Kroger cart authorization complete; refresh token saved.")
        return 0

    if args.find_store:
        for loc in client.find_location(args.find_store):
            print(f"{loc.location_id}  {loc.chain:12} {loc.name}  ({loc.address})")
        print("\nCopy the locationId you want into KROGER_LOCATION_ID in .env")
        return 0

    source = args.list or args.source
    lines = _read_lines(source)
    if not lines:
        print("No list items found.", file=sys.stderr)
        return 1

    print(f"Resolving {len(lines)} item(s)...")
    items = import_list(lines, client)
    _print_summary(items)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
