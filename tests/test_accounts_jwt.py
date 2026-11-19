"""The primary's token signer: its key file, RS256, the JWKS and the claims of AUTH §5.3."""

import base64
import json
import stat

from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import padding, rsa

from dreamference.accounts.accounts_jwt import (
    ACCESS_LIFETIME_S,
    OPENAI_AUTH_CLAIM,
    RAGON_AUTH_CLAIM,
    AccountsJwt,
)

ISSUER = "https://box.local:8000"
USER = {"user_id": "u1", "username": "t-alice", "role": "admin"}


def b64(segment: str) -> bytes:
    return base64.urlsafe_b64decode(segment + "=" * (-len(segment) % 4))


def signer(tmp_path):
    return AccountsJwt(tmp_path / "state" / "signing-key.pem", ISSUER, "node-1", "box")


def test_the_key_is_created_once_with_mode_0600_and_reused(tmp_path):
    first = signer(tmp_path)
    key = tmp_path / "state" / "signing-key.pem"
    assert stat.S_IMODE(key.stat().st_mode) == 0o600
    before = key.read_bytes()
    key.chmod(0o644)
    second = signer(tmp_path)
    assert second.kid == first.kid and key.read_bytes() == before
    assert stat.S_IMODE(key.stat().st_mode) == 0o600
    # A token of the first load verifies with the second: one key pair, never rotated.
    assert second.verify(first.sign(first.claims(USER, "c", 1, "m"))) is not None


def test_the_claims_carry_every_name_codex_reads(tmp_path):
    claims = signer(tmp_path).claims(USER, "client-x", 7, "laptop", now=1000)
    assert claims["iss"] == ISSUER and claims["sub"] == "u1" and claims["aud"] == "client-x"
    assert (claims["iat"], claims["exp"]) == (1000, 1000 + ACCESS_LIFETIME_S) == (1000, 4600)
    assert claims["email"] == claims["name"] == claims["preferred_username"] == "t-alice"
    assert claims[OPENAI_AUTH_CLAIM] == {
        "chatgpt_plan_type": "admin",
        "chatgpt_user_id": "u1",
        "chatgpt_account_id": "u1",
        "user_id": "u1",
    }
    assert claims[RAGON_AUTH_CLAIM] == {
        "org": "node-1",
        "primary_name": "box",
        "machine": "laptop",
        "sid": 7,
    }


def test_a_token_is_rs256_and_its_payload_reads_without_padding(tmp_path):
    jwt = signer(tmp_path)
    token = jwt.sign(jwt.claims(USER, "c", 1, "m"))
    header, payload, signature = token.split(".")
    assert "=" not in token
    assert json.loads(b64(header)) == {"alg": "RS256", "typ": "JWT", "kid": jwt.kid}
    assert json.loads(b64(payload))["sub"] == "u1"
    assert b64(signature)


def test_verify_refuses_a_tampered_expired_foreign_or_malformed_token(tmp_path):
    jwt = signer(tmp_path)
    claims = jwt.claims(USER, "c", 1, "m", now=1000)
    token = jwt.sign(claims)
    assert jwt.verify(token, now=1001) == claims
    assert jwt.verify(token, now=1000 + ACCESS_LIFETIME_S) is None  # expired
    header, _, signature = token.split(".")
    forged = AccountsJwt.b64url(json.dumps({**claims, "sub": "u2"}).encode())
    assert jwt.verify(f"{header}.{forged}.{signature}", now=1001) is None
    none_header = AccountsJwt.b64url(json.dumps({"alg": "none"}).encode())
    assert jwt.verify(f"{none_header}.{token.split('.')[1]}.", now=1001) is None
    other = AccountsJwt(tmp_path / "other.pem", ISSUER)
    assert jwt.verify(other.sign(claims), now=1001) is None  # another primary's key
    foreign = AccountsJwt(tmp_path / "state" / "signing-key.pem", "https://elsewhere.local:8000")
    assert jwt.verify(foreign.sign({**claims, "iss": foreign.issuer}), now=1001) is None
    for junk in ("", "a.b", "a.b.c", "!!.??.**", f"{header}.e30.{signature}"):
        assert jwt.verify(junk, now=1001) is None, junk


def test_the_jwks_verifies_a_token(tmp_path):
    jwt = signer(tmp_path)
    token = jwt.sign(jwt.claims(USER, "c", 1, "m"))
    (key,) = jwt.jwks()["keys"]
    assert (key["kty"], key["alg"], key["use"], key["kid"]) == ("RSA", "RS256", "sig", jwt.kid)
    public = rsa.RSAPublicNumbers(
        int.from_bytes(b64(key["e"]), "big"), int.from_bytes(b64(key["n"]), "big")
    ).public_key()
    head, payload, signature = token.split(".")
    public.verify(b64(signature), f"{head}.{payload}".encode(), padding.PKCS1v15(), hashes.SHA256())
