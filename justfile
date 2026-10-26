image := "asset-server"

# Client settings (override via env or .env)
set dotenv-load
url  := env("ASSET_URL", "http://localhost:3000")
key  := env("API_KEY", "")
curl := "curl --fail-with-body -sS " + env("CURL_OPTS", "")

# List available recipes
list:
    @just --list

# Build the release binary
build:
    cargo build --release

# Build the Docker image
build-docker:
    docker build -t {{image}} .

# Create a project (admin key): just create <name> <project-key>
create name project_key:
    @{{curl}} -X POST {{url}}/assets -H "X-Api-Key: {{key}}" -H "Content-Type: application/json" -d '{"name":"{{name}}","api_key":"{{project_key}}"}'

# Upload a file: just upload <project> <file> [true|public=true]
upload project file public="false":
    @{{curl}} -X POST {{url}}/assets/{{project}} -H "X-Api-Key: {{key}}" -F "file=@{{file}}" -F "public={{ trim_start_match(public, "public=") }}"

# List metadata of a project (all files with key, public only without)
files project:
    @{{curl}} {{ if key != "" { '-H "X-Api-Key: ' + key + '"' } else { "" } }} {{url}}/assets/{{project}}

# Download a file into the current directory: just download <project> <file>
download project file:
    @{{curl}} {{ if key != "" { '-H "X-Api-Key: ' + key + '"' } else { "" } }} -o {{file}} {{url}}/assets/{{project}}/{{file}}

# Change a project's key: just set-key <project> <new-key>
set-key project new_key:
    @{{curl}} -X PUT {{url}}/assets/{{project}} -H "X-Api-Key: {{key}}" -H "Content-Type: application/json" -d '{"api_key":"{{new_key}}"}'
