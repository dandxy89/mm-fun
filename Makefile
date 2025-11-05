.PHONY: help build up down restart logs clean test

help:
	@echo "Market Maker Docker Commands"
	@echo "============================"
	@echo "  make build       - Build all Docker images"
	@echo "  make up          - Start all services"
	@echo "  make down        - Stop all services"
	@echo "  make restart     - Restart all services"
	@echo "  make logs        - View logs (all services)"
	@echo "  make logs-follow - Follow logs (all services)"
	@echo "  make clean       - Remove containers, volumes, and images"
	@echo "  make test        - Build and test compilation"
	@echo ""
	@echo "Individual Services:"
	@echo "  make logs-collector  - View collector logs"
	@echo "  make logs-pricing    - View pricing logs"
	@echo "  make logs-strategy   - View strategy logs"
	@echo "  make logs-simulator  - View simulator logs"
	@echo "  make restart-strategy - Restart strategy service"
	@echo "  make restart-simulator - Restart simulator service"

build:
	docker-compose build

up:
	docker-compose up -d
	@echo "Services started. Use 'make logs' to view logs."

down:
	docker-compose down

restart:
	docker-compose restart

logs:
	docker-compose logs

logs-follow:
	docker-compose logs -f

logs-collector:
	docker-compose logs -f mm_collector

logs-pricing:
	docker-compose logs -f mm_pricing

logs-strategy:
	docker-compose logs -f mm_strategy

logs-simulator:
	docker-compose logs -f mm_simulator

restart-strategy:
	docker-compose restart mm_strategy

restart-simulator:
	docker-compose restart mm_simulator

restart-collector:
	docker-compose restart mm_collector

restart-pricing:
	docker-compose restart mm_pricing

clean:
	docker-compose down -v --rmi all
	@echo "Cleaned up all containers, volumes, and images"

test:
	cargo build --release
	@echo "Build successful"

# Development targets
dev-build:
	cargo build

dev-run-collector:
	cargo run --bin mm_collector

dev-run-pricing:
	cargo run --bin mm_pricing

dev-run-strategy:
	cargo run --bin mm_strategy

dev-run-simulator:
	cargo run --bin mm_simulator
