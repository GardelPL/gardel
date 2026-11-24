# The Linux toolchain for gardelc (P8): the versions CI pins for Windows, on Ubuntu 24.04.
# Build:  docker build -t gardel-linux scripts/linux
# Run:    scripts/linux/test.sh   (copies the tree into the container and runs the checks)
# Where apt cannot reach its mirrors (port 80 closed), BASE may be any Ubuntu 24.04 image that
# already has gcc, curl, unzip, git and python3: the apt step is skipped when gcc is there.
ARG BASE=ubuntu:24.04
FROM ${BASE}

ARG BOOGIE_VERSION=3.4.3
ARG Z3_VERSION=4.12.1
ARG Z3_SHA256=c5360fd157b0f861ec8780ba3e51e2197e9486798dc93cd878df69a4b0c2b7c5

ENV DEBIAN_FRONTEND=noninteractive
RUN if ! command -v gcc > /dev/null; then \
      apt-get update \
      && apt-get install -y --no-install-recommends build-essential ca-certificates curl unzip git python3 \
      && rm -rf /var/lib/apt/lists/*; \
    fi

# Rust, stable, as CI's dtolnay/rust-toolchain@stable.
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
 && rustc --version

# .NET 6, which Boogie 3.4.3 runs on, as CI's setup-dotnet 6.0.x. No ICU is needed in invariant mode.
ENV DOTNET_ROOT=/opt/dotnet DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1
RUN curl -sSL https://dot.net/v1/dotnet-install.sh -o /tmp/dotnet-install.sh \
 && bash /tmp/dotnet-install.sh --channel 6.0 --install-dir /opt/dotnet \
 && rm /tmp/dotnet-install.sh
RUN /opt/dotnet/dotnet tool install --tool-path /opt/boogie Boogie --version ${BOOGIE_VERSION}
ENV PATH=/opt/dotnet:/opt/boogie:$PATH BOOGIE_EXE=/opt/boogie/boogie

# Z3, checked against its hash.
RUN curl -sSL -o /tmp/z3.zip https://github.com/Z3Prover/z3/releases/download/z3-${Z3_VERSION}/z3-${Z3_VERSION}-x64-glibc-2.35.zip \
 && echo "${Z3_SHA256}  /tmp/z3.zip" | sha256sum -c - \
 && unzip -q /tmp/z3.zip -d /opt \
 && mv /opt/z3-${Z3_VERSION}-x64-glibc-2.35 /opt/z3 \
 && rm /tmp/z3.zip
ENV Z3_EXE=/opt/z3/bin/z3

RUN gcc --version | head -1 && rustc --version && /opt/z3/bin/z3 --version
