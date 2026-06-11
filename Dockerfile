# Cart import server. Wraps the `kroger` package behind FastAPI.
# Build: docker build -t kroger-cart-server .
# Run:   docker compose up   (see docker-compose.yml)

FROM python:3.11-slim

# uv for fast, locked installs (pinned to match the dev environment).
COPY --from=ghcr.io/astral-sh/uv:0.8.17 /uv /bin/uv

ENV UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy \
    UV_PROJECT_ENVIRONMENT=/app/.venv \
    PATH="/app/.venv/bin:$PATH"

WORKDIR /app

# 1. Install dependencies first (cached unless pyproject/uv.lock change).
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-install-project --extra server

# 2. Add the source and install the project itself.
COPY README.md ./
COPY kroger ./kroger
COPY server ./server
RUN uv sync --frozen --extra server

# Persist OAuth token + UPC cache on a volume so they survive restarts.
ENV KROGER_TOKEN_STORE=/data/token.json \
    KROGER_UPC_CACHE=/data/upc_cache.json
VOLUME ["/data"]

EXPOSE 8000
EXPOSE 8088

# Default: serve the API. Override for one-time login (see docker-compose.yml).
CMD ["uv", "run", "--frozen", "--extra", "server", \
     "uvicorn", "server.app:app", "--host", "0.0.0.0", "--port", "8000"]
