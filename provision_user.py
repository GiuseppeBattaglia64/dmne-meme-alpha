#!/usr/bin/env python3
import sys, json, os, hashlib, secrets
from pathlib import Path
import psycopg2

VAULT_DIR = Path("/home/ubuntu/.config/solana/wallets")
VAULT_DIR.mkdir(parents=True, exist_ok=True)
os.chmod(VAULT_DIR, 0o700)

alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
def b58encode(raw_bytes):
    num = int.from_bytes(raw_bytes, 'big')
    encoded = []
    while num > 0:
        num, rem = divmod(num, 58)
        encoded.append(alphabet[rem])
    pad = 0
    for b in raw_bytes:
        if b == 0: pad += 1
        else: break
    return '1' * pad + ''.join(reversed(encoded))

def hash_pw(pw: str) -> str:
    return hashlib.sha256(pw.encode('utf-8')).hexdigest()

def main():
    if len(sys.argv) < 5:
        print(json.dumps({"success": False, "error": "Usage: provision_user.py <user_id> <display_name> <password> <mode>"}))
        sys.exit(1)

    user_id = sys.argv[1].strip().lower()
    display_name = sys.argv[2].strip()
    password = sys.argv[3].strip()
    mode = sys.argv[4].strip().upper()
    if mode not in ["PAPER", "LIVE"]:
        mode = "PAPER"

    keypair_path = VAULT_DIR / f"{user_id}.json"

    # Generate 64-byte Ed25519 keypair
    try:
        from cryptography.hazmat.primitives.asymmetric import ed25519
        from cryptography.hazmat.primitives import serialization
        priv = ed25519.Ed25519PrivateKey.generate()
        priv_bytes = priv.private_bytes(
            encoding=serialization.Encoding.Raw,
            format=serialization.PrivateFormat.Raw,
            encryption_algorithm=serialization.NoEncryption()
        )
        pub_bytes = priv.public_key().public_bytes(
            encoding=serialization.Encoding.Raw,
            format=serialization.PublicFormat.Raw
        )
        kp_bytes = list(priv_bytes + pub_bytes)
    except ImportError:
        try:
            from nacl.signing import SigningKey
            seed = os.urandom(32)
            sk = SigningKey(seed)
            kp_bytes = list(seed + sk.verify_key.encode())
        except ImportError:
            kp_bytes = list(secrets.token_bytes(64))

    with open(keypair_path, "w") as f:
        json.dump(kp_bytes, f)
    os.chmod(keypair_path, 0o600)

    raw_kp = bytes(kp_bytes)
    pubkey_bytes = raw_kp[32:] if len(raw_kp) == 64 else raw_kp[:32]
    pubkey = b58encode(pubkey_bytes)

    # Insert into PostgreSQL
    try:
        try:
            conn = psycopg2.connect("dbname=dmne_quant user=ubuntu")
        except Exception:
            conn = psycopg2.connect("dbname=dmne_quant user=postgres")

        cur = conn.cursor()
        cur.execute("""
            INSERT INTO users (id, display_name, role, execution_mode, pubkey, keypair_path, paper_bankroll_sol, password_hash, has_started)
            VALUES (%s, %s, 'user', %s, %s, %s, 0.5000, %s, false)
            ON CONFLICT (id) DO UPDATE
            SET display_name = EXCLUDED.display_name,
                execution_mode = EXCLUDED.execution_mode,
                pubkey = EXCLUDED.pubkey,
                password_hash = EXCLUDED.password_hash;
        """, (user_id, display_name, mode, pubkey, str(keypair_path), hash_pw(password)))
        conn.commit()
        cur.close()
        conn.close()
        print(json.dumps({
            "success": True,
            "user_id": user_id,
            "display_name": display_name,
            "mode": mode,
            "pubkey": pubkey,
            "keypair_path": str(keypair_path)
        }))
    except Exception as e:
        print(json.dumps({"success": False, "error": str(e)}))
        sys.exit(1)

if __name__ == "__main__":
    main()
