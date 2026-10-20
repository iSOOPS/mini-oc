# 三阶段构建：SPA → Rust → 运行时（公共镜像源，替换 V1 私有 harbor）
# build context = 仓库根（compose 已配置；手动构建：docker build -f apps/mini-oc-web/Dockerfile .）
# 国内网络可传 --build-arg REGISTRY=<镜像加速域名>（如 docker.m.daocloud.io）
ARG REGISTRY=docker.io
FROM ${REGISTRY}/library/node:23-alpine AS spa
WORKDIR /app/web
COPY apps/mini-oc-web/web/package.json apps/mini-oc-web/web/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --silent
COPY apps/mini-oc-web/web/ ./
RUN npm run build

FROM ${REGISTRY}/library/rust:1-slim AS build
WORKDIR /app
# 依赖层缓存：先拷 manifests 用 dummy 源码编译依赖
COPY Cargo.toml Cargo.lock ./
COPY crates/mini-oc-core/Cargo.toml crates/mini-oc-core/Cargo.toml
COPY apps/mini-oc-web/Cargo.toml apps/mini-oc-web/Cargo.toml
RUN mkdir -p crates/mini-oc-core/src apps/mini-oc-web/src \
    && echo "" > crates/mini-oc-core/src/lib.rs \
    && echo "fn main(){}" > apps/mini-oc-web/src/main.rs \
    && echo "" > apps/mini-oc-web/src/lib.rs \
    && cargo build --release -p mini-oc-web 2>/dev/null || true
COPY crates/ crates/
COPY apps/mini-oc-web/src/ apps/mini-oc-web/src/
RUN touch crates/mini-oc-core/src/lib.rs apps/mini-oc-web/src/main.rs apps/mini-oc-web/src/lib.rs \
    && cargo build --release -p mini-oc-web

FROM ${REGISTRY}/library/debian:bookworm-slim
# reqwest(rustls) 内置 webpki-roots 证书，运行时零额外依赖；
# healthcheck 用 bash /dev/tcp 探测（无需 wget/curl）
WORKDIR /app
COPY --from=build /app/target/release/mini-oc-web /app/mini-oc-web
COPY --from=spa /app/web/dist /app/web/dist
ENV WEB_PORT=8100 \
    WEB_BIND=0.0.0.0 \
    WEB_STATIC_DIR=/app/web/dist \
    COOKIE_KEY_PATH=/data/cookie_key
EXPOSE 8100
HEALTHCHECK --interval=30s --timeout=3s \
  CMD bash -c 'exec 3<>/dev/tcp/127.0.0.1/8100 && printf "GET /healthz HTTP/1.0\r\n\r\n" >&3 && grep -q ok <&3' || exit 1
CMD ["/app/mini-oc-web"]
