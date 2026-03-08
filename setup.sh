#!/bin/bash
# ============================================================================
# Cardano ARB Bot — Setup Script
# ============================================================================
# This script creates the GitHub repo and pushes the initial code.
# Run this from the project root directory.
#
# Prerequisites:
#   - gh CLI installed and authenticated (gh auth login)
#   - Rust toolchain installed (rustup)
#   - git configured with your name and email
#
# Usage:
#   chmod +x setup.sh
#   ./setup.sh
# ============================================================================

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}=== Cardano ARB Bot Setup ===${NC}"
echo ""

# Check prerequisites
echo -e "${YELLOW}Checking prerequisites...${NC}"

if ! command -v gh &> /dev/null; then
    echo -e "${RED}Error: gh CLI not found. Install it from https://cli.github.com/${NC}"
    exit 1
fi

if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: Rust/Cargo not found. Install from https://rustup.rs/${NC}"
    exit 1
fi

if ! command -v git &> /dev/null; then
    echo -e "${RED}Error: git not found.${NC}"
    exit 1
fi

# Check gh auth
if ! gh auth status &> /dev/null; then
    echo -e "${RED}Error: gh CLI not authenticated. Run: gh auth login${NC}"
    exit 1
fi

echo -e "${GREEN}All prerequisites met!${NC}"
echo ""

# Get GitHub username
GH_USER=$(gh api user --jq '.login')
echo -e "GitHub user: ${GREEN}${GH_USER}${NC}"

# Create the repo
REPO_NAME="cardano-arb-bot"
echo -e "${YELLOW}Creating private repo: ${GH_USER}/${REPO_NAME}...${NC}"

gh repo create "${REPO_NAME}" --private --description "High-performance Cardano DEX arbitrage bot (Rust)" || {
    echo -e "${YELLOW}Repo may already exist, continuing...${NC}"
}

# Initialize git and push
if [ ! -d ".git" ]; then
    git init
    git branch -M main
fi

# Update Cargo.toml with actual username
sed -i "s|YOUR_USERNAME|${GH_USER}|g" Cargo.toml

# Add the remote
git remote remove origin 2>/dev/null || true
git remote add origin "https://github.com/${GH_USER}/${REPO_NAME}.git"

# Create initial commit
git add -A
git commit -m "Initial commit: Cardano DEX arbitrage bot scaffolding

- Multi-DEX support: Minswap, SundaeSwap, WingRiders, MuesliSwap
- DEX-to-DEX and triangular arbitrage strategies
- USDCx (Circle USDC bridge) as primary stablecoin focus
- Rust for maximum execution speed
- Blockfrost API integration for chain queries
- EWMA price smoothing and configurable risk parameters"

# Push
git push -u origin main

echo ""
echo -e "${GREEN}=== Setup Complete! ===${NC}"
echo -e "Repo: https://github.com/${GH_USER}/${REPO_NAME}"
echo ""
echo -e "${YELLOW}Next steps:${NC}"
echo "  1. Copy config.example.toml to config.toml and fill in your values"
echo "  2. Get a Blockfrost API key from https://blockfrost.io"
echo "  3. Set up your wallet signing key in ./keys/payment.skey"
echo "  4. Update DEX script hashes in config.toml with current mainnet values"
echo "  5. Run: cargo build --release"
echo "  6. Test with: cargo run -- --dry-run --scan-only"
echo ""
echo -e "${RED}⚠️  IMPORTANT: Never commit config.toml or any .skey files!${NC}"
