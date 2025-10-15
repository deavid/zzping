This directory holds generated test certificates for integration tests.

Do not commit private keys or generated certificates. Use the scripts in `scripts/` to regenerate certificates before running the integration tests:

  bash scripts/generate_multi_certs.sh 1
  bash scripts/generate_two_cas.sh

The repository ignores this directory; the .gitkeep keeps the directory in git.
