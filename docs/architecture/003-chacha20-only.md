# ADR-003: ChaCha20-Poly1305 Only (No AES-GCM)

## Status
Accepted

## Context
HyprDrive encrypts all data before it leaves the device (P2P transfer, cloud upload). The two standard AEAD ciphers are:
- **AES-256-GCM**: Hardware-accelerated on x86 (AES-NI) and Apple Silicon.
- **ChaCha20-Poly1305**: Software-only, constant-time on all architectures.

Spacedrive supports BOTH ciphers. This doubles the test surface and adds algorithm negotiation complexity.

## Decision
Use **ChaCha20-Poly1305** as the sole encryption cipher. No AES-GCM.

## Consequences

### Positive
- **One code path**: Single cipher = one encrypt function, one decrypt function, one test suite. Half the cryptographic surface area.
- **No timing attacks**: ChaCha20 is constant-time in software. AES without hardware acceleration is vulnerable to cache-timing attacks.
- **Cross-platform parity**: Same performance on x86, ARM, RISC-V, WASM. No AES-NI dependency.
- **Mobile-friendly**: On older ARM devices without ARMv8 Crypto Extensions, ChaCha20 is 3× faster than AES-GCM.
- **No negotiation**: Peers don't need to agree on a cipher suite. Every device speaks ChaCha20. Zero handshake complexity.

### Negative
- **~15% slower on AES-NI hardware**: On modern x86/Apple Silicon, AES-GCM with hardware acceleration is ~15% faster than software ChaCha20. Mitigated: encryption is not the bottleneck — disk I/O and network are.
- **Unusual choice**: Most enterprise systems default to AES. May raise questions from security auditors.

### Neutral
- Both ciphers are NIST-approved and considered secure as of 2024.
- TLS 1.3 supports both equally.

## References
- [RFC 8439 — ChaCha20-Poly1305](https://tools.ietf.org/html/rfc8439)
- [Google chose ChaCha20 for Android](https://security.googleblog.com/2014/04/)
- [WireGuard uses ChaCha20 exclusively](https://www.wireguard.com/protocol/)
