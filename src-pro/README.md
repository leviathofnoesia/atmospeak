# Atmospeak Pro modules

This crate is linked **only** into the Pro desktop build (`cargo build --features pro`).

## Production hardening

For the decided architecture (separate unpirateable-as-practical Pro binary):

1. Move this directory to a **private** repository (e.g. `novpax/atmospeak-pro`).
2. In Pro CI, depend on it via a private git source or vendored checkout that free CI never sees.
3. Keep the public `atmospeak` remote free of Pro feature implementations.

Until that move, developing Pro capabilities here is fine — free builds omit the
`pro` feature so this code is not linked into the free binary, but the source
is still visible in the public tree.

## Capabilities (v1)

| Id | Label | Role |
| --- | --- | --- |
| `airplane_mode` | Airplane mode | Refuse new outbound sockets on dictation-related paths |
| `network_ledger` | Network ledger | Append-only outbound connection log for compliance export |
