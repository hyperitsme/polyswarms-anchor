# PolySwarms Anchor (Parimutuel YES/NO)

- Solana mainnet-beta anchor program for YES/NO markets with proportional payout.
- Vaults are PDAs owned by the program (`vault_yes`, `vault_no`, `fee_vault`).
- Fees are applied pro-rata during claim.

## Quick start

```bash
# Install toolchains
rustup default stable
cargo install --locked anchor-cli
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"

# Set RPC to mainnet (or your provider)
solana config set --url https://api.mainnet-beta.solana.com

# Init repo deps
npm i
