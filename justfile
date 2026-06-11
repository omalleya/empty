# Local dev tasks. Install just: https://github.com/casey/just
# Run `just` (no args) to list everything.

set dotenv-load := true          # auto-load .env for every recipe

# Show available recipes
default:
    @just --list

# Install/sync all deps (incl. server + dev extras) into .venv
install:
    uv sync --extra server --extra dev

# Run the API server with hot-reload — the "npm start" of this project
dev:
    uv run --extra server uvicorn server.app:app --reload --port 8000

# Run the server without reload (closer to production)
serve port="8000":
    uv run --extra server uvicorn server.app:app --host 0.0.0.0 --port {{port}}

# One-time Kroger cart authorization (opens a browser, saves a refresh token)
login:
    uv run kroger-cart --login

# Find nearby stores for a zip (copy the locationId into .env)
find-store zip:
    uv run kroger-cart --find-store {{zip}}

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
    docker compose run --rm --service-ports server uv run --frozen kroger-cart --login

# Build + run the server in Docker
docker-up:
    docker compose up -d --build

# Tail server logs
docker-logs:
    docker compose logs -f server
