# syntax=docker/dockerfile:1
# GBA build toolchain for Tauros. Pins must match toolchain/sources.json (checked by tests).
# Built and run by scripts/setup_toolchain.py; builds run without network and without root.
# libelf1 is needed by the expansion's bundled test runner (tools/mgba/mgba-rom-test).
FROM devkitpro/devkitarm:20260221@sha256:4debd5b33cf4361a557b6bf3be5ff823804868125ce1429912f1a4e773e7ac5d

RUN apt-get update \
    && apt-get install -y --no-install-recommends libpng-dev libelf1 unzip \
    && rm -rf /var/lib/apt/lists/*

ADD --checksum=sha256:249f9adeb3b6d7f2e33347c1d0bce5f3ff8fb06e914c75b81d08187f8404c4ff \
    https://github.com/huderlem/poryscript/releases/download/3.6.1/poryscript-linux.zip /tmp/poryscript.zip
ADD --checksum=sha256:68c904c3ff707ed2d6de0372073f165e627ec3ee7855eedbb3363184cc9212a4 \
    https://github.com/grunt-lucas/porytiles/releases/download/v2.0.0/porytiles-linux-amd64.zip /tmp/porytiles.zip

RUN unzip -q /tmp/poryscript.zip -d /opt \
    && unzip -q /tmp/porytiles.zip -d /opt \
    && rm -f /tmp/poryscript.zip /tmp/porytiles.zip \
       /opt/porytiles-linux-amd64/LegacyTests /opt/porytiles-linux-amd64/PorytilesIntegrationTests \
    && chmod 0755 /opt/poryscript-linux/poryscript /opt/porytiles-linux-amd64/porytiles \
    && ln -s /opt/poryscript-linux/poryscript /usr/local/bin/poryscript \
    && ln -s /opt/porytiles-linux-amd64/porytiles /usr/local/bin/porytiles

ENV HOME=/tmp
WORKDIR /work
