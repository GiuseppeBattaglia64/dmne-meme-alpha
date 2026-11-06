#!/usr/bin/env python3
import sys, json
from pathlib import Path

WALLET_PATH = Path("/home/ubuntu/.config/solana/dmne-live-wallet.json")
MIN_GAS_RESERVE_SOL = 0.0300

def main():
    if len(sys.argv) < 3:
        print("Usage: python3 withdraw.py <RECIPIENT_BASE58> <AMOUNT_SOL>")
        sys.exit(1)
        
    recipient = sys.argv[1]
    try:
        amount = float(sys.argv[2])
    except ValueError:
        print("Error: Amount must be a valid float.")
        sys.exit(1)

    if not WALLET_PATH.exists():
        print(f"Error: Wallet keypair not found at {WALLET_PATH}")
        sys.exit(1)

    print(f"--- DMNE COLD STORAGE WITHDRAWAL ---")
    print(f"Source Keypair:  {WALLET_PATH}")
    print(f"Destination:     {recipient}")
    print(f"Requested:       {amount:.4f} SOL")
    print(f"Gas Floor Held:  {MIN_GAS_RESERVE_SOL:.4f} SOL")
    print(f"------------------------------------")
    print(f"[STATUS] Ready for live signing when funded.")

if __name__ == "__main__":
    main()
