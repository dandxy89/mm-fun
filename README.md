# mm-dd

A market maker system written in Rust, using Aeron IPC for low-latency inter-process communication.

## Architecture

The system consists of multiple services:

- **mm_collector**: Market data collector service (WebSocket data ingestion)
- **mm_pricing**: Pricing engine service
- **aeronmd**: Aeron Media Driver for IPC messaging

Services communicate via Aeron IPC using shared memory for ultra-low latency.

## Prerequisites

- Rust (latest stable)
- Docker and Docker Compose
- Cargo

## Building

```bash
cargo build --release
```

## Running

### With Docker Compose

```bash
docker-compose up
```

This will start all services with proper dependencies.

### Local Development

```bash
# Run individual components
cargo run --bin mm_collector
cargo run --bin mm_pricing
```

## Project Structure

- `mm_aeron/` - Aeron IPC messaging library
- `mm_app/` - Main application logic
- `mm_binary/` - Binary protocol utilities
- `mm_http/` - HTTP client/server
- `mm_orderbook/` - Order book data structure
- `mm_ratelimit/` - Rate limiting utilities
- `mm_tg/` - Telegram integration
- `mm_ws/` - WebSocket client/server

## Testing

```bash
cargo test
```
