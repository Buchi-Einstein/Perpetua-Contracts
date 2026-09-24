# Perpetua stream URI scheme (`stellar:stream`)

Standard URI for representing a Perpetua stream on a mobile device. Designed to
be encoded as a QR code so a sender can share a stream link and a recipient can
open its status in any conforming wallet or dashboard.

## Format

```
stellar:stream?v=1&id=<stream_id>&contract=<contract_address>[&network=<network>][&token=<token_address>]
```

| param | required | type | meaning |
|---|---|---|---|
| `v` | no | int | scheme version; absent means `1` |
| `id` | yes | u64 | stream id (decimal) |
| `contract` | yes | `C…` strkey | Perpetua stream contract address |
| `network` | no | enum | `public` (default), `testnet`, `futurenet`, `standalone` |
| `token` | no | `C…` strkey | SEP-41 token the stream pays out (for display) |

Percent-encoding applies to query values per RFC 3986. Parameters are
case-sensitive; unknown parameters are tolerated and ignored by consumers.

## Examples

```
stellar:stream?id=123&contract=CBCGTSCJXBMPPPE4BPDIPYZXPE2J5TQEKD2KCS7VQF533NKKEYGUTHXW
stellar:stream?v=1&id=123&contract=CBCGTSCJ…&network=testnet&token=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
```

## QR encoding

- Encode the URI bytes UTF-8, then render as a standard QR (QR Code Model 2,
  any error-correction level; the scheme is short enough to fit a version-3+ QR
  comfortably at level M).
- No QR-level wrapper is defined: the URI *is* the payload. Consumers must not
  prepend `https://` or other schemes.

## Resolving a stream URI

A conforming client resolves the URI by simulation:

1. Normalize: lowercase the scheme + query parameter names, keep values verbatim.
2. If `network` is absent, assume the client's active network; if it is present
   and differs from the active network, fail with a clear message rather than
   silently switching.
3. `send_request` a simulation of `get_stream(id)` against `contract`. Fetch the
   token metadata from the address in `token` if present, else from the stored
   `Stream`.
4. Display: stream status, recipient, `vested_of` / `withdrawable_of`, and a
   `Withdraw` action (recipient auth only).

## Rationale for the shape

- The `stellar:` scheme keeps Share/QR deep links wallet-safe and avoids an
  HTTPS middle-man redirect that could be repointed.
- `id` + `contract` is the complete on-chain addressing (v1 has no per-user
  index and no discovery — see the README), so no more context is needed to
  prove the stream exists.
- Versioning via `v` rather than a new path keeps `stellar:stream` stable as the
  query contract evolves.

## See also

- [docs/ABI.md](ABI.md) for the `get_stream` / `withdrawable_of` interface the
  resolver calls.