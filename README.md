# Down Detector

A single-endpoint HTTP API built in Rust that checks whether a list of domains is up or down.

## Features

- One endpoint: `GET /`
- Accepts a comma-separated list of domains via the `domains` query parameter
- All checks run concurrently (max 50 in flight per request)
- Up to 50 unique domains per request
- 30 second timeout per check
- Redirects are treated as up without being followed

## Rules

| Condition | Result |
|---|---|
| Status `2xx`, `3xx`, `4xx` | `is_down: false` |
| Status `5xx` | `is_down: true` |
| No status (timeout, DNS failure, connection refused/reset) | `is_down: true`, `status_code: null` |

- **Validation**: only valid entries are processed. A value is valid if it parses as an `http`/`https` URL with a non-empty host. Values without a scheme get `https://` prepended. Everything else (empty, whitespace, other schemes, malformed) is silently dropped.
- **Dedup**: duplicates are removed case-insensitively; only the first occurrence is kept.
- **Cap**: only the first 50 unique domains per request are checked.

## Usage

```bash
cargo run --release
```

The server listens on `0.0.0.0:3000`.

```bash
curl "http://localhost:3000/?domains=google.com,github.com,https://example.com:8443/health"
```

### Response

```json
{
  "msg": "success",
  "data": [
    { "domain": "google.com", "is_down": false, "status_code": 200 },
    { "domain": "github.com", "is_down": false, "status_code": 301 },
    { "domain": "https://example.com:8443/health", "is_down": true, "status_code": null }
  ]
}
```

- `domain`: the trimmed value as it appeared in the query
- `is_down`: whether the domain is considered down
- `status_code`: the HTTP status received, or `null` if there was no response

A missing or empty `domains` parameter returns:

```json
{ "msg": "success", "data": [] }
```

## Development

```bash
cargo test    # unit tests for validation, dedup, cap, and status rules
cargo clippy  # lints
```
