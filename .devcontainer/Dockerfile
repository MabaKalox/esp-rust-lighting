ARG VARIANT=bullseye
FROM debian:${VARIANT}
ENV DEBIAN_FRONTEND=noninteractive
ENV LC_ALL=C.UTF-8
ENV LANG=C.UTF-8

# Arguments
ARG TOOLCHAIN_VERSION=1.64.0.0
ARG ESP_BOARD=esp32
ARG INSTALL_RUST_TOOLCHAIN=install-rust-toolchain.sh

# Install dependencies
RUN apt-get update \
    && apt-get install -y git curl gcc clang ninja-build libudev-dev unzip xz-utils\
    python3 python3-pip python3-venv libusb-1.0-0 libssl-dev pkg-config libtinfo5 libpython2.7 \
    zsh \
    socat \
    && apt-get clean -y && rm -rf /var/lib/apt/lists/* /tmp/library-scripts

WORKDIR /root

# Install Rust toolchain, extra crates and esp-idf
ENV PATH=${PATH}:/root/.cargo/bin

ADD https://github.com/esp-rs/rust-build/releases/download/v${TOOLCHAIN_VERSION}/${INSTALL_RUST_TOOLCHAIN} \
    /root/${INSTALL_RUST_TOOLCHAIN}

RUN chmod a+x ${INSTALL_RUST_TOOLCHAIN} \
    && ./${INSTALL_RUST_TOOLCHAIN} \
    --extra-crates "ldproxy cargo-espflash wokwi-server web-flash" \
    --export-file /root/export-esp.sh \
    --build-target "${ESP_BOARD}" \
    && rustup component add clippy rustfmt

# Remove last line from export env script, because it breaks build
# RUN sed -i '$d' /root/export-esp.sh

ADD https://git.grml.org/f/grml-etc-core/etc/zsh/zshrc \
    /root/.zshrc
ADD https://git.grml.org/f/grml-etc-core/etc/skel/.zshrc \
    /root/.zshrc.local
# Activate ESP environment
RUN echo "source /root/export-esp.sh" >> ~/.zshrc.local \
    && echo "source /root/export-esp.sh" >> ~/.bashrc

# Download xdg proxy script
ADD https://raw.githubusercontent.com/kitsunyan/xdg-open-server/master/xdg-open \
    /usr/local/bin/
RUN chmod +x /usr/local/bin/xdg-open

CMD [ "/bin/bash" ]
