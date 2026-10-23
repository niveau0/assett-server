image := "asset-server"

# List available recipes
list:
    @just --list

# Build the release binary
build:
    cargo build --release

# Build the Docker image
build-docker:
    docker build -t {{image}} .
