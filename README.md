# page-warmup

Rust CLI that opens a web app in Chromium and waits until it responds.
It is useful for warming up apps that only start on a real browser navigation.

## Run with Docker

```sh
docker build -t page-warmup .
docker run --init --rm --cap-add=SYS_ADMIN \
  -e TARGET_URL=https://your-app.example.com/ page-warmup
```

Or with Docker Compose:

```sh
TARGET_URL=https://your-app.example.com/ docker compose run --rm page-warmup
```

To propagate the CLI result through `docker compose up`:

```sh
TARGET_URL=https://your-app.example.com/ \
  docker compose up --build --abort-on-container-exit --exit-code-from page-warmup
```

Google Chrome Stable runs in headed mode by default; the container provides its display
with Xvfb. Set `HEADLESS=true` only for local checks: some services do not warm up from
headless navigations, so runs against the real target must stay headed.
The container runs Chromium as a non-root user with its sandbox enabled. The Docker host must allow Chromium's
setuid sandbox helper (do not use `no-new-privileges`). `SYS_ADMIN` is added
because some Docker hosts block the namespaces Chromium needs; it grants extra
container privileges, so run only a trusted image and avoid mounting sensitive
host paths into it.

## Optional readiness header

If the target reports whether it is still warming up in a response header, set
`READINESS_HEADER` to the header name. The CLI keeps waiting while the header value
matches `NOT_READY_VALUE` (default: `asleep`):

```sh
READINESS_HEADER=x-example-readiness NOT_READY_VALUE=asleep \
  TARGET_URL=https://your-app.example.com/ \
  docker compose run --rm page-warmup
```

Without `READINESS_HEADER`, the CLI judges readiness from the HTTP status only.

## GitHub Actions

Run the `Page warmup` workflow manually and provide `target_url`. Optionally set
`readiness_header` and `not_ready_value` for the target.
