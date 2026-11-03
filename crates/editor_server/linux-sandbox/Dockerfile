# The dev image runs the same Linux worker and sandbox as the editor's Edgegap container.
FROM public.ecr.aws/docker/library/rust:trixie AS builder
WORKDIR /worker
COPY crates/editor_server/linux-sandbox/Cargo.toml ./Cargo.toml
COPY Cargo.lock ./Cargo.lock
RUN sed -i 's/version.workspace = true/version = "0.1.0"/' Cargo.toml && printf '\n[workspace]\n' >> Cargo.toml
COPY crates/editor_server/linux-sandbox/src ./src
RUN cargo build --release --bin linux_sandbox_worker

FROM builder AS sandbox_tests
RUN apt-get update && apt-get install -y --no-install-recommends blender python3-numpy python3-pil
COPY crates/editor_server/linux-sandbox/tests ./tests
RUN cargo test --locked -- --include-ignored --test-threads=1

FROM public.ecr.aws/docker/library/debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends blender python3-numpy python3-pil \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /worker/target/release/linux_sandbox_worker /usr/local/bin/linux_sandbox_worker
# The dev launcher isolates this image from editor files and credentials with Docker.
# Docker's init makes the worker a process group leader, which cannot start the session the worker needs, so setsid
# starts it in one.
ENTRYPOINT ["setsid", "--wait", "linux_sandbox_worker", "--isolated-container"]
