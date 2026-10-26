# asset-server

Lightweight asset/CDN server (Rust + axum). Assets are stored per project on the
filesystem; a small server-rendered UI lists public assets.

## Config

```toml
# config.toml
api_key      = "admin-secret"
storage_path = "./data"
bind_addr    = "0.0.0.0:3000"
```

`api_key` is the admin key and can access everything.

## Run

```sh
cargo run --release -- --config config.toml
```

## Docker

```sh
docker build -t asset-server .
docker run --rm -p 3000:3000 \
  -v "$PWD/config.toml:/etc/asset-server/config.toml:ro" \
  -v asset-data:/var/lib/asset-server/data \
  asset-server
```

Use `storage_path = "/var/lib/asset-server/data"` in the config. The image runs as
UID 10001; for bind mounts make sure the directory is writable by that user.

## Auth

Every request may send `X-Api-Key: <key>`. There are three roles:

| Role | Key | Permissions |
|---|---|---|
| Anonymous | none | Read public files/metadata, view UI |
| Project | project key | Full access to its own project |
| Admin | `api_key` from config | Full access to everything |

An unknown key is rejected with `401`. A project key cannot access other projects.

## API

Base URL: `http://localhost:3000`.

### Create project (admin)

```sh
curl -X POST http://localhost:3000/assets \
  -H "X-Api-Key: admin-secret" \
  -H "Content-Type: application/json" \
  -d '{"name":"game","api_key":"project-secret"}'
```

`201 Created`; `409 Conflict` if the project already exists.
Project names: `[A-Za-z0-9_-]{1,64}`.

### Upload asset (admin or project key)

```sh
curl -X POST http://localhost:3000/assets/game \
  -H "X-Api-Key: project-secret" \
  -F "file=@./texture.png" \
  -F "public=true"
```

- `public` is optional: `true` / `false` / `1` / `0` (default `false`)
- exactly one file per request
- existing files are overwritten
- the project must exist (`404` otherwise)

### Metadata

```sh
# anonymous: only public files
curl http://localhost:3000/assets/game

# project/admin key: all files
curl -H "X-Api-Key: project-secret" http://localhost:3000/assets/game
```

```json
[
  {
    "name": "texture.png",
    "size": 204800,
    "modified_at": "2026-05-01T12:00:00Z",
    "public": true
  }
]
```

Anonymous requests to unknown/private-only projects return `200 []`.

### Download

```sh
# public file: no key needed
curl -O http://localhost:3000/assets/game/texture.png

# private file: admin or project key
curl -O -H "X-Api-Key: project-secret" \
  http://localhost:3000/assets/game/secret.ktx2
```

Private files return `404` without a valid key.

### Update project key (admin or project key)

```sh
curl -X PUT http://localhost:3000/assets/game \
  -H "X-Api-Key: admin-secret" \
  -H "Content-Type: application/json" \
  -d '{"api_key":"new-project-secret"}'
```

`204 No Content`. The old key is invalid immediately. Renaming is not supported.

### Web UI

- `GET /` — projects that have at least one public file
- `GET /ui/{project}` — public files of a project

The UI needs no key and never shows private files.

## Storage layout

```
{storage_path}/
  _metadata.json
  {project}/
    {file}
```

`_metadata.json` stores project API key hashes and per-file public flags. It is the
source of truth; manual filesystem changes are not picked up. Delete and rename are
not supported.

## Notes

- Uploads are streamed to `{storage_path}/.tmp` before being moved into place.
- No upload size limit; use a reverse proxy if exposed publicly.
- Run behind a reverse proxy (nginx/caddy) for TLS.
- Logging: `RUST_LOG=debug ./asset-server --config config.toml`.

## just helpers

Set `API_KEY` (admin or project key) and optionally `ASSET_URL` (default `http://localhost:3000`),
e.g. in a `.env` file. `CURL_OPTS="-k"` is passed to curl (self-signed certs).

```sh
just create <name> <project-key>     # admin
just upload <project> <file> [true]  # optional public flag
just files <project>
just download <project> <file>
just set-key <project> <new-key>
```
