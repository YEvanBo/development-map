#!/bin/sh
set -eu
cd "$(dirname "$0")"
shasum -a 256 -c MANIFEST.sha256
ssh-keygen -Y verify -f provenance/allowed_signers -I EvanBo -n file -s MANIFEST.sha256.sig < MANIFEST.sha256
