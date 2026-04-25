FROM public.ecr.aws/docker/library/debian:bookworm as build_app

RUN apt-get update && apt-get install -y curl gpg clang build-essential git cmake
RUN curl https://apt.repos.intel.com/intel-gpg-keys/GPG-PUB-KEY-INTEL-SW-PRODUCTS.PUB | gpg --dearmor -o /usr/share/keyrings/oneapi-archive-keyring.gpg && echo "deb [signed-by=/usr/share/keyrings/oneapi-archive-keyring.gpg] https://apt.repos.intel.com/oneapi all main" > /etc/apt/sources.list.d/oneAPI.list
RUN apt-get update && apt-get install -y libpcre2-dev liblapack-dev libblas-dev libopenblas-dev intel-oneapi-mkl-devel

ENV MKLROOT=/opt/intel/oneapi/mkl/latest

ENV CC=clang
ARG UID="1000"
ARG GID="1000"
RUN groupadd -g "${GID}" linguaspark && useradd -l -u "${UID}" -g "${GID}" -m -d /home/linguaspark linguaspark -s /bin/bash
WORKDIR /home/linguaspark/
USER linguaspark
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | bash -s -- -y --default-toolchain stable
ENV RUSTFLAGS="-C target-cpu=x86-64-v2 -C linker=clang"
CMD ["bash"]

