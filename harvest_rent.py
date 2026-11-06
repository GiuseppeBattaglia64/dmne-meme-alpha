#!/usr/bin/env python3
import sys, json, os, urllib.request
from pathlib import Path

RPC_ENDPOINT = os.environ.get("SOLANA_RPC_URL", "https://api.mainnet-beta.solana.com")
WALLET_DIR = Path("/home/ubuntu/.config/solana/wallets")
ADMIN_KEY = Path("/home/ubuntu/.config/solana/dmne-live-wallet.json")

def rpc_call(method, params):
    req = urllib.request.Request(
        RPC_ENDPOINT,
        data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode('utf-8'),
        headers={
            "Content-Type": "application/json",
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)"
        }
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read().decode('utf-8'))

def inspect_wallet_rent(pubkey):
    # Query all SPL Token accounts owned by this wallet
    payload = rpc_call("getTokenAccountsByOwner", [
        pubkey,
        {"programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
        {"encoding": "jsonParsed"}
    ])
    
    accounts = payload.get("result", {}).get("value", [])
    empty_accounts = []
    
    for acc in accounts:
        info = acc["account"]["data"]["parsed"]["info"]
        amount = float(info["tokenAmount"]["uiAmount"] or 0)
        lamports = acc["account"]["lamports"]
        if amount == 0:
            empty_accounts.append({
                "pubkey": acc["pubkey"],
                "mint": info["mint"],
                "reclaimable_sol": lamports / 1e9
            })
            
    return empty_accounts

def main():
    target_wallet = sys.argv[1] if len(sys.argv) > 1 else "CwBPS16Pz6RonsKmYPJAjZR7RbFpbKwJN6CcvJikj55j"
    print(f"--- DMNE SOLANA RENT AUDITOR ---")
    print(f"Target Public Key: {target_wallet}")
    
    try:
        empty = inspect_wallet_rent(target_wallet)
        total_reclaimable = sum(a["reclaimable_sol"] for a in empty)
        print(f"Found {len(empty)} dormant 0-balance token accounts.")
        print(f"Total Reclaimable Capital: {total_reclaimable:.6f} SOL (~${total_reclaimable * 140:.2f} USD)")
        
        if len(empty) > 0:
            print("\nDormant Accounts Queued for Reclamation:")
            for a in empty[:5]:
                print(f"  • ATA: {a['pubkey']} (Mint: {a['mint'][:8]}...) -> +{a['reclaimable_sol']} SOL")
            if len(empty) > 5:
                print(f"  ... and {len(empty) - 5} more.")
        else:
            print("[CLEAN] No empty accounts holding trapped rent.")
            
    except Exception as e:
        print(f"Error querying RPC: {e}")

if __name__ == "__main__":
    main()
