"""LLM layer: turn free-text shopping-list lines into concrete Kroger UPCs.

Two jobs, both handled by Claude:

  1. Phrase a good *search term* for the Products API
     ("a couple gallons of 2% milk"  ->  "2% milk gallon")

  2. *Select* the best matching product from the search results
     (dozens of milk UPCs -> the one the user meant)

A JSON cache (`upc_cache.json`) remembers raw_text -> UPC so each line is only
disambiguated once. First run you confirm; every run after is instant + free.

Model: claude-opus-4-8 (see https://docs.claude.com). Structured outputs via
`client.messages.parse(..., output_format=PydanticModel)`.
"""

from __future__ import annotations

import json
import os
import re
from pathlib import Path

import anthropic
from pydantic import BaseModel, Field

from .client import KrogerClient
from .models import Product, ResolvedItem

_MODEL = "claude-opus-4-8"
# Override with KROGER_UPC_CACHE to persist the cache (e.g. onto a Docker volume).
_CACHE_PATH = Path(os.environ.get("KROGER_UPC_CACHE", "upc_cache.json"))

# Leading quantity like "2 milk", "2x milk", "milk x2"
_QTY_RE = re.compile(r"^\s*(\d+)\s*x?\s+|\s+x\s*(\d+)\s*$", re.IGNORECASE)


# ----------------------- structured-output schemas ----------------------- #
class _SearchTerm(BaseModel):
    raw: str = Field(description="The original list line, verbatim.")
    term: str = Field(description="A concise grocery search term for that line.")


class _SearchTerms(BaseModel):
    items: list[_SearchTerm]


class _Selection(BaseModel):
    chosen_index: int = Field(
        description="0-based index of the best matching product, or -1 if none fit."
    )
    confidence: float = Field(description="0.0–1.0 confidence in the choice.")
    reason: str = Field(description="One short sentence explaining the pick.")


class ListResolver:
    def __init__(
        self,
        client: KrogerClient,
        llm: anthropic.Anthropic | None = None,
        cache_path: Path = _CACHE_PATH,
        auto_accept_confidence: float = 0.75,
    ):
        self.client = client
        self.llm = llm or anthropic.Anthropic()
        self.cache_path = cache_path
        self.auto_accept_confidence = auto_accept_confidence
        self._cache: dict[str, dict] = self._load_cache()

    # ------------------------------------------------------------------ #
    # public entry point
    # ------------------------------------------------------------------ #
    def resolve(self, lines: list[str]) -> list[ResolvedItem]:
        parsed = [self._split_quantity(line) for line in lines]

        # Batch the search-term phrasing for all not-yet-cached lines in one call.
        misses = [text for text, _ in parsed if self._cache_key(text) not in self._cache]
        terms = self._search_terms(misses) if misses else {}

        results: list[ResolvedItem] = []
        for text, qty in parsed:
            key = self._cache_key(text)
            if key in self._cache:
                c = self._cache[key]
                results.append(
                    ResolvedItem(
                        raw_text=text,
                        quantity=qty,
                        chosen=Product(upc=c["upc"], description=c.get("label", text)),
                        from_cache=True,
                    )
                )
                continue

            term = terms.get(text, text)
            candidates = self.client.search_products(term)
            item = ResolvedItem(
                raw_text=text, quantity=qty, search_term=term, candidates=candidates
            )
            if not candidates:
                item.note = "no search results"
                results.append(item)
                continue

            sel = self._select(text, candidates)
            if 0 <= sel.chosen_index < len(candidates) and sel.confidence >= self.auto_accept_confidence:
                item.chosen = candidates[sel.chosen_index]
                item.note = sel.reason
                self._remember(key, item.chosen)
            else:
                # Low confidence or no fit — leave unchosen for the caller to confirm.
                item.note = f"needs confirmation ({sel.reason})"
            results.append(item)

        self._save_cache()
        return results

    # ------------------------------------------------------------------ #
    # LLM calls
    # ------------------------------------------------------------------ #
    def _search_terms(self, lines: list[str]) -> dict[str, str]:
        prompt = (
            "Convert each grocery shopping-list line into a short search term suited "
            "to a grocery store product search. Drop quantities and filler words; keep "
            "brand, variety, and size cues.\n\n"
            + "\n".join(f"- {line}" for line in lines)
        )
        resp = self.llm.messages.parse(
            model=_MODEL,
            max_tokens=1024,
            output_config={"effort": "low"},
            messages=[{"role": "user", "content": prompt}],
            output_format=_SearchTerms,
        )
        out = resp.parsed_output
        mapping = {st.raw: st.term for st in out.items} if out else {}
        # Fall back to the raw line for anything the model didn't echo back.
        return {line: mapping.get(line, line) for line in lines}

    def _select(self, raw_text: str, candidates: list[Product]) -> _Selection:
        listing = "\n".join(f"{i}: {p.label()}" for i, p in enumerate(candidates))
        prompt = (
            f'Shopping-list item: "{raw_text}"\n\n'
            "Candidate products:\n"
            f"{listing}\n\n"
            "Pick the single best match for what the shopper most likely wants. "
            "Prefer the common/default size and variety unless the item specifies "
            "otherwise. If nothing is a reasonable match, return chosen_index -1."
        )
        resp = self.llm.messages.parse(
            model=_MODEL,
            max_tokens=512,
            output_config={"effort": "low"},
            messages=[{"role": "user", "content": prompt}],
            output_format=_Selection,
        )
        return resp.parsed_output or _Selection(
            chosen_index=-1, confidence=0.0, reason="no structured output"
        )

    # ------------------------------------------------------------------ #
    # quantity parsing + cache
    # ------------------------------------------------------------------ #
    @staticmethod
    def _split_quantity(line: str) -> tuple[str, int]:
        qty = 1
        m = _QTY_RE.search(line)
        if m:
            qty = int(m.group(1) or m.group(2))
            line = _QTY_RE.sub(" ", line).strip()
        return line.strip(), qty

    @staticmethod
    def _cache_key(text: str) -> str:
        return " ".join(text.lower().split())

    def _remember(self, key: str, product: Product) -> None:
        self._cache[key] = {"upc": product.upc, "label": product.label()}

    def _load_cache(self) -> dict[str, dict]:
        if self.cache_path.exists():
            try:
                return json.loads(self.cache_path.read_text())
            except (json.JSONDecodeError, OSError):
                return {}
        return {}

    def _save_cache(self) -> None:
        self.cache_path.write_text(json.dumps(self._cache, indent=2, sort_keys=True))

    # Let the caller record a manual choice so it's remembered next time.
    def confirm(self, item: ResolvedItem, product: Product) -> None:
        item.chosen = product
        self._remember(self._cache_key(item.raw_text), product)
        self._save_cache()
