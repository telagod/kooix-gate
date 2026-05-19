# Security waiver: RUSTSEC-2023-0071 via OpenID Connect JWK support

Status: active
Last reviewed: 2026-05-19
Advisory: RUSTSEC-2023-0071 (`rsa` Marvin timing sidechannel)

## Why this waiver exists

`cargo audit` reports `rsa 0.9.10` through `openidconnect 4.0.1`. The RustSec advisory currently has no fixed `rsa` release. Kooix Gate uses OpenID Connect for authentication/JWK verification, not for decrypting attacker-supplied ciphertexts or exposing an RSA private-key operation in the gateway data plane.

## Compensating controls

- Keep the ignore scoped to this one advisory only: `cargo audit --ignore RUSTSEC-2023-0071`.
- Keep all fixable advisories failing CI normally.
- Re-review when `rsa` or `openidconnect` publishes a patched dependency path.
- OIDC remains in the control-plane auth boundary; do not reuse this waiver for data-plane cryptographic signing/decryption code.

## Exit plan

1. Track upstream `rsa` / `openidconnect` releases.
2. Remove the ignore once a patched path is available.
3. Run plain `cargo audit` and update this file when cleared.
