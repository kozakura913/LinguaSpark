FROM public.ecr.aws/docker/library/debian:bookworm AS builder

RUN apt-get update && apt-get install -y curl gpg clang build-essential git cmake
RUN curl https://apt.repos.intel.com/intel-gpg-keys/GPG-PUB-KEY-INTEL-SW-PRODUCTS.PUB | gpg --dearmor -o /usr/share/keyrings/oneapi-archive-keyring.gpg && echo "deb [signed-by=/usr/share/keyrings/oneapi-archive-keyring.gpg] https://apt.repos.intel.com/oneapi all main" > /etc/apt/sources.list.d/oneAPI.list
RUN apt-get update && apt-get install -y libpcre2-dev liblapack-dev libblas-dev libopenblas-dev intel-oneapi-mkl-devel

ENV MKLROOT=/opt/intel/oneapi/mkl/latest

ENV CC=clang
ENV CXX=clang++
ARG UID="1000"
ARG GID="1000"
RUN groupadd -g "${GID}" linguaspark && useradd -l -u "${UID}" -g "${GID}" -m -d /home/linguaspark linguaspark -s /bin/bash
WORKDIR /home/linguaspark/
USER linguaspark
COPY --link --chown=linguaspark:linguaspark . .
RUN git submodule update --init --recursive
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | bash -s -- -y --default-toolchain stable
ENV RUSTFLAGS="-C target-cpu=x86-64-v2 -C linker=clang"
RUN bash -c "source /opt/intel/oneapi/setvars.sh ; source ~/.cargo/env ; cargo build --release"
FROM public.ecr.aws/docker/library/debian:bookworm AS oneapi

RUN apt-get update && apt-get install -y curl gpg
RUN curl https://apt.repos.intel.com/intel-gpg-keys/GPG-PUB-KEY-INTEL-SW-PRODUCTS.PUB | gpg --dearmor -o /usr/share/keyrings/oneapi-archive-keyring.gpg && echo "deb [signed-by=/usr/share/keyrings/oneapi-archive-keyring.gpg] https://apt.repos.intel.com/oneapi all main" > /etc/apt/sources.list.d/oneAPI.list
RUN apt-get clean && apt-get update && apt-get install -y --download-only intel-oneapi-mkl
RUN mkdir -p /root/intel-oneapi-mkl && cp /var/cache/apt/archives/*.deb /root/intel-oneapi-mkl

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends libopenblas0
COPY --from=oneapi /root/intel-oneapi-mkl /root/intel-oneapi-mkl
RUN apt-get update && apt-get install -y /root/intel-oneapi-mkl/*.deb
ARG UID="1000"
ARG GID="1000"
RUN groupadd -g "${GID}" linguaspark && useradd -l -u "${UID}" -g "${GID}" -m -d /home/linguaspark linguaspark -s /bin/bash
WORKDIR /home/linguaspark/
USER linguaspark
COPY --chown=linguaspark:linguaspark --from=builder /home/linguaspark/target/release/linguaspark-server /home/linguaspark/linguaspark-server
ENV MODELS_DIR=/home/linguaspark/models
ENV NUM_WORKERS=1
ENV IP=0.0.0.0
ENV PORT=3000
# ENV ENV_API_KEY=
ENV RUST_LOG=info

EXPOSE 3000
RUN echo "#!/bin/bash\nsource /opt/intel/oneapi/setvars.sh\nexec \"\$@\"" | tee entrypoint.sh && chmod +x entrypoint.sh
ENTRYPOINT ["/home/linguaspark/entrypoint.sh"]
CMD ["/home/linguaspark/linguaspark-server"]
