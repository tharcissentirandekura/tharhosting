COMPOSE := docker compose -f dev/compose.yaml

.PHONY: dev watch build lint test up down logs clean

dev: 
	cargo run
build: 
	cargo build --release

lint:
	cargo clippy -- -D warnings

test: 
	cargo test

up: 
	$(COMPOSE) up --build

down:
	$(COMPOSE) down

logs:
	$(COMPOSE) logs -f backend

clean:
	cargo clean 
	$(COMPOSE) down -v

