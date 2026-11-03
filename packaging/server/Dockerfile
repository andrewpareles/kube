FROM public.ecr.aws/docker/library/node:22-bookworm AS kube_ts_api_builder
WORKDIR /app/packages/kube-ts-api
COPY packages/kube-ts-api/package.json packages/kube-ts-api/package-lock.json packages/kube-ts-api/tsconfig.types.json packages/kube-ts-api/api-extractor.json ./
COPY packages/kube-ts-api/src ./src
RUN npm ci
RUN npm run bundle

FROM public.ecr.aws/docker/library/rust:bookworm AS server_builder
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
  && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock /app/
COPY crates /app/crates
COPY packages/website/.env /app/packages/website/.env
COPY assets/fonts /app/assets/fonts
COPY assets/scripts /app/assets/scripts
COPY certs /app/certs
COPY packages/kube-ts-api/src /app/packages/kube-ts-api/src
COPY --from=kube_ts_api_builder /app/packages/kube-ts-api/dist /app/packages/kube-ts-api/dist
RUN LIBCLANG_PATH="$(llvm-config --libdir)" cargo build -p server --release --locked

FROM public.ecr.aws/docker/library/debian:bookworm-slim
WORKDIR /app
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*
COPY --from=server_builder /app/target/release/server /app/server
RUN chmod +x /app/server
CMD ["/app/server"]
