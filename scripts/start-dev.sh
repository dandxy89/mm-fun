#!/bin/bash
# Development startup script for market maker system
# This script starts all services locally without Docker

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}Market Maker Development Startup${NC}"
echo "=================================="

# Check if Aeron media driver is running
if ! pgrep -f "aeronmd" > /dev/null; then
    echo -e "${YELLOW}Warning: Aeron media driver not detected${NC}"
    echo "You may need to start it manually or install Aeron"
fi

# Create necessary directories
mkdir -p logs config

# Build the project
echo -e "\n${GREEN}Building project...${NC}"
cargo build --release

# Function to run a service in the background
run_service() {
    local service=$1
    local binary=$2

    echo -e "${GREEN}Starting $service...${NC}"
    RUST_LOG=info ./target/release/$binary > logs/$service.log 2>&1 &
    local pid=$!
    echo $pid > logs/$service.pid
    echo -e "${GREEN}$service started (PID: $pid)${NC}"
}

# Trap to cleanup on exit
cleanup() {
    echo -e "\n${YELLOW}Stopping all services...${NC}"

    if [ -f logs/mm_simulator.pid ]; then
        kill $(cat logs/mm_simulator.pid) 2>/dev/null || true
        rm logs/mm_simulator.pid
    fi

    if [ -f logs/mm_strategy.pid ]; then
        kill $(cat logs/mm_strategy.pid) 2>/dev/null || true
        rm logs/mm_strategy.pid
    fi

    if [ -f logs/mm_pricing.pid ]; then
        kill $(cat logs/mm_pricing.pid) 2>/dev/null || true
        rm logs/mm_pricing.pid
    fi

    if [ -f logs/mm_collector.pid ]; then
        kill $(cat logs/mm_collector.pid) 2>/dev/null || true
        rm logs/mm_collector.pid
    fi

    echo -e "${GREEN}All services stopped${NC}"
}

trap cleanup EXIT INT TERM

# Start services in order
echo -e "\n${GREEN}Starting services...${NC}"

run_service "mm_collector" "mm_collector"
sleep 2

run_service "mm_pricing" "mm_pricing"
sleep 2

run_service "mm_strategy" "mm_strategy"
sleep 2

run_service "mm_simulator" "mm_simulator"

echo -e "\n${GREEN}All services started!${NC}"
echo -e "${YELLOW}Press Ctrl+C to stop all services${NC}"
echo -e "\nView logs:"
echo "  tail -f logs/mm_collector.log"
echo "  tail -f logs/mm_pricing.log"
echo "  tail -f logs/mm_strategy.log"
echo "  tail -f logs/mm_simulator.log"

# Wait for user interrupt
wait
