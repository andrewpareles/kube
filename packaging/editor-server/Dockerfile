FROM public.ecr.aws/docker/library/node:22-bookworm AS kube_ts_api_builder
WORKDIR /app/packages/kube-ts-api
COPY packages/kube-ts-api/package.json packages/kube-ts-api/package-lock.json packages/kube-ts-api/tsconfig.types.json packages/kube-ts-api/api-extractor.json ./
COPY packages/kube-ts-api/src ./src
RUN npm ci
RUN npm run bundle

FROM public.ecr.aws/docker/library/node:22-bookworm AS play_webview_builder
WORKDIR /app/packages/play-webview
COPY packages/play-webview/package.json packages/play-webview/package-lock.json packages/play-webview/build.mjs packages/play-webview/tsconfig.json ./
COPY packages/play-webview/src ./src
COPY packages/website/lib/kubeLogo /app/packages/website/lib/kubeLogo
RUN npm ci
RUN npm run bundle

FROM public.ecr.aws/docker/library/node:22-bookworm AS editor_node_builder
WORKDIR /app/crates/editor_server/node
COPY crates/editor_server/node/package.json crates/editor_server/node/package-lock.json ./
RUN npm ci
COPY crates/editor_server/node/src ./src

FROM public.ecr.aws/docker/library/rust:bookworm AS editor_server_builder
WORKDIR /app
RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    cmake \
    clang \
    libclang-dev \
    llvm-dev \
    ca-certificates \
    git \
    libasound2-dev \
    libudev-dev \
    libxkbcommon-dev \
    libwayland-dev \
    libx11-dev \
    libssl-dev \
    libdbus-1-dev \
  && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock /app/
COPY crates /app/crates
COPY packages/website/.env /app/packages/website/.env
COPY assets/cursors /app/assets/cursors
COPY assets/fonts /app/assets/fonts
COPY assets/scripts /app/assets/scripts
COPY certs /app/certs
COPY packages/kube-ts-api/src /app/packages/kube-ts-api/src
COPY --from=kube_ts_api_builder /app/packages/kube-ts-api/dist /app/packages/kube-ts-api/dist
COPY packages/play-webview/index.html /app/packages/play-webview/index.html
COPY --from=play_webview_builder /app/packages/play-webview/dist /app/packages/play-webview/dist
RUN LIBCLANG_PATH="$(llvm-config --libdir)" cargo build -p editor_server -p linux_sandbox --release --locked

FROM public.ecr.aws/docker/library/node:22-trixie-slim
WORKDIR /app/crates/editor_server
# Screenshots draw on the CPU through Mesa's lavapipe. Chromium draws players' GUI over playtest screenshots.
RUN apt-get update \
  && apt-get install -y --no-install-recommends blender chromium python3-numpy python3-pil ca-certificates libasound2t64 libudev1 libxkbcommon0 libwayland-client0 libx11-6 libssl3t64 libdbus-1-3 libvulkan1 mesa-vulkan-drivers \
  && rm -rf /var/lib/apt/lists/*
COPY --from=editor_server_builder /app/target/release/editor_server /app/editor_server
COPY --from=editor_server_builder /app/target/release/linux_sandbox_worker /app/linux_sandbox_worker
COPY --from=editor_node_builder /app/crates/editor_server/node /app/crates/editor_server/node
COPY --from=kube_ts_api_builder /app/packages/kube-ts-api/dist /app/packages/kube-ts-api/dist
RUN chmod +x /app/editor_server
ENV PATH="/app:${PATH}"
CMD ["editor_server"]
