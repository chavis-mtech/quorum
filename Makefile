# Quorum - quick dev commands
# Usage: make <target>

.PHONY: help setup all infra ai backend frontend-dev frontend-build test migrate ollama-pull stop

help:
	@echo ""
	@echo "  ⚖️  Quorum — Multi-agent Consensus Trading"
	@echo ""
	@echo "  First time:  make setup && make infra && make ollama-pull"
	@echo "  Run all:     make all   (terminal tabs: ai | backend | frontend-dev)"
	@echo ""
	@echo "  make setup          - copy .env.example -> .env (first time)"
	@echo "  make infra          - start PostgreSQL (docker compose)"
	@echo "  make ollama-pull    - pull the recommended LLMs (qwen3:14b + qwen3:8b)"
	@echo "  make ai             - start Python AI sidecar (port 8765)"
	@echo "  make backend        - start Rust backend + auto-migrate (port 8080)"
	@echo "  make frontend-dev   - start SolidJS dev server (port 5173)"
	@echo "  make frontend-build - build production frontend into frontend/dist"
	@echo "  make test           - run all tests (Python aggregator + Rust 24 tests)"
	@echo "  make stop           - stop docker services"
	@echo ""

# ─── First-time setup ────────────────────────────────────────────────────────

setup:
	@if [ -f .env ]; then \
		echo "✓ .env already exists — skipping (edit manually if needed)"; \
	else \
		cp .env.example .env; \
		echo "✓ .env created from .env.example"; \
		echo ""; \
		echo "  ⚠️  Set these before running:"; \
		echo "    JWT_SECRET  - generate with: openssl rand -hex 32"; \
		echo "    ADMIN_PASSWORD - initial password for owner@quorum.local"; \
		echo ""; \
	fi
	@if [ ! -f backend/config/quorum.toml ] && [ -f config/quorum.toml ]; then \
		echo "✓ config/quorum.toml already in place"; \
	fi
	@echo "Next: make infra && make ollama-pull"

infra:
	docker compose up -d postgres

ollama-pull:
	ollama pull qwen3:14b
	ollama pull qwen3:8b

ai:
	python3 ai-layer/http_server.py

backend:
	cd backend && cargo run

frontend-dev:
	cd frontend && npm run dev

frontend-build:
	cd frontend && npm install && npm run build

test:
	python3 ai-layer/tests/test_aggregator.py
	python3 ai-layer/tests/test_entry_discipline.py
	cd backend && cargo test

migrate:
	cd backend && cargo run   # migration runs automatically on startup

stop:
	docker compose down
