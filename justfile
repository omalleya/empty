# Local dev tasks. Install just: https://github.com/casey/just
# Run `just` (no args) to list everything.

set dotenv-load := true          # auto-load .env for every recipe

# Show available recipes
default:
    @just --list

# Install/sync all deps (incl. server + dev extras) into .venv
install:
    uv sync --extra server --extra dev

# Run the API server (Rust) — the "npm start" of this project
dev:
    cargo run --manifest-path server-rs/Cargo.toml

# Run the optimized server (closer to production)
serve port="8000":
    PORT={{port}} cargo run --release --manifest-path server-rs/Cargo.toml

# Run the legacy Python API server with hot-reload
dev-py:
    uv run --extra server uvicorn server.app:app --reload --port 8000

# One-time Kroger cart authorization (prints a URL, saves a refresh token)
login:
    cargo run --manifest-path server-rs/Cargo.toml -- --login

# Find nearby stores for a zip (copy the locationId into .env)
find-store zip:
    cargo run --manifest-path server-rs/Cargo.toml -- --find-store {{zip}}

# Import a shopping-list file into the cart
import file:
    uv run kroger-cart --list {{file}}

# Lint
lint:
    uv run --extra dev ruff check .

# Auto-format / fix lint
fmt:
    uv run --extra dev ruff check --fix .
    uv run --extra dev ruff format .

# --- Docker ---

# One-time cart login inside the container (publishes 8088 for the redirect)
docker-login:
    docker compose run --rm --service-ports server kroger-cart-server --login

# Build + run the server in Docker
docker-up:
    docker compose up -d --build

# Tail server logs
docker-logs:
    docker compose logs -f server
