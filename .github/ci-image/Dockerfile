# The image both Linux CI jobs run in (ADR-0008). It holds everything those jobs used to fetch from
# Ubuntu's package servers, Launchpad and kernel.org on every run, so they fetch none of it:
#
# - the Tauri prerequisites, WebKitWebDriver and xvfb, for the build and the smoke test;
# - the latest stable git from ppa:git-core/ppa, first on the PATH, for Linux (latest git);
# - git at MINIMUM_GIT_VERSION, built from source into /opt/git-<version>, off the PATH, which
#   Linux (minimum git) puts first;
# - tauri-driver, in /usr/local/bin.
#
# Built and published to GHCR by .github/workflows/ci-image.yml, which passes MINIMUM_GIT_VERSION
# as gikli-core declares it and tags each build with .github/scripts/ci-image-tag.sh, so a change
# to this file, to that workflow or to that version gets a new image on the next push to main.

# Built apart, so the Rust toolchain that builds it stays out of the image.
FROM rust:1-bookworm AS tauri-driver
RUN cargo install tauri-driver --locked --root /out

# Pinned to the Ubuntu release the Linux jobs ran on before: the Tauri prerequisites below use
# Ubuntu 24.04 package names, and 26.04 renamed webkit2gtk-driver to webkitgtk-webdriver.
FROM ubuntu:24.04
ARG MINIMUM_GIT_VERSION
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV DEBIAN_FRONTEND=noninteractive
# The GitHub-hosted runner's locale.
ENV LANG=C.UTF-8

# apt tries a server again, gives up on one that stops answering instead of stalling, and apt-get
# update fails when any repository could not be fetched, so the build fails rather than install
# from a stale list.
RUN printf '%s\n' 'Acquire::Retries "3";' 'Acquire::http::Timeout "30";' \
      'Acquire::https::Timeout "30";' 'APT::Update::Error-Mode "any";' \
      > /etc/apt/apt.conf.d/80-ci-network

# The Tauri prerequisites and the smoke test's WebDriver and display, as the Linux jobs installed
# them, with what the GitHub-hosted runner had and this base image lacks: a compiler, curl for the
# Rust toolchain the jobs install, zstd for their caches, ssh-keygen and perl for the core tests,
# and fonts and a software renderer for the smoke test.
RUN apt-get update \
 && apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
      libayatana-appindicator3-dev webkit2gtk-driver xvfb \
      build-essential pkg-config file curl ca-certificates xz-utils zstd \
      openssh-client perl xauth fonts-dejavu-core libgl1-mesa-dri \
      software-properties-common gnupg \
 && rm -rf /var/lib/apt/lists/*

# The latest stable git. Never Ubuntu's own git in its place: the CI job checks the package.
RUN add-apt-repository -y ppa:git-core/ppa \
 && apt-get install -y git \
 && dpkg-query -W -f='${Version}\n' git | grep -q ppa \
 && rm -rf /var/lib/apt/lists/*

# git at MINIMUM_GIT_VERSION, with only what the tests use: remotes are local folders and a local
# HTTP server, and no translations. This git predates C23, which newer compilers default to.
RUN test -n "${MINIMUM_GIT_VERSION}" \
 && apt-get update \
 && apt-get install -y zlib1g-dev libcurl4-openssl-dev \
 && rm -rf /var/lib/apt/lists/* \
 && cd /tmp \
 && curl -fsSL --retry 3 --connect-timeout 30 --max-time 150 -o git.tar.gz \
      "https://mirrors.edge.kernel.org/pub/software/scm/git/git-${MINIMUM_GIT_VERSION}.tar.gz" \
 && tar -xzf git.tar.gz \
 && options=(prefix="/opt/git-${MINIMUM_GIT_VERSION}" CFLAGS="-O2 -std=gnu17" NO_EXPAT=1 \
      NO_OPENSSL=1 NO_GETTEXT=1 NO_TCLTK=1 NO_PERL=1 NO_PYTHON=1) \
 && make -C "git-${MINIMUM_GIT_VERSION}" -j"$(nproc)" "${options[@]}" all \
 && make -C "git-${MINIMUM_GIT_VERSION}" "${options[@]}" install \
 && rm -rf /tmp/git.tar.gz "/tmp/git-${MINIMUM_GIT_VERSION}" \
 && "/opt/git-${MINIMUM_GIT_VERSION}/bin/git" --version

COPY --from=tauri-driver /out/bin/tauri-driver /usr/local/bin/tauri-driver

# A container cannot give WebKitGTK the namespaces its sandbox needs; without this its web process
# would not start there. The smoke test only loads gikli's own pages.
ENV WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
