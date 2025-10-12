# How to Contribute

We'd love to accept your patches and contributions to this project. There are
just a few small guidelines you need to follow.

## Contributor License Agreement

Contributions to this project must be accompanied by a Contributor License
Agreement (CLA). You (or your employer) retain the copyright to your
contribution; this simply gives us permission to use and redistribute your
contributions as part of the project. Head over to
<https://cla.developers.google.com/> to see your current agreements on file or
to sign a new one.

You generally only need to submit a CLA once, so if you've already submitted one
(even if it was for a different project), you probably don't need to do it
again.

## Code reviews

All submissions, including submissions by project members, require review. We
use GitHub pull requests for this purpose. Consult
[GitHub Help](https://help.github.com/articles/about-pull-requests/) for more
information on using pull requests.

## Community Guidelines

This project follows
[Google's Open Source Community Guidelines](https://opensource.google/conduct/).

## Testing

### zzintent-config component

The `zzintent-config` component has integration tests that rely on debug-mode permissive behavior when `SessionManager` is not configured. This is intentional for test ergonomics but ensures production safety by rejecting config changes in release builds.

**Important**: Always run tests for `zzintent-config` in debug mode (the default for `cargo test`). The component's behavior differs between debug and release builds for security reasons.

```bash
# Run zzintent-config tests (debug mode by default)
cargo test -p zzintent-config --lib

# If you need to run in release mode, note that some integration tests may fail
# due to the intentional security restrictions
cargo test -p zzintent-config --lib --release  # May fail some tests
```

## Component template

See `COMPONENT_TEMPLATE_GUIDE.md` at the repository root for a short, practical template and examples to start new components.

## Runtime limits (environment variables)

You can configure SessionManager connection limits at runtime using environment variables. These are read automatically by the server builder and passed to the underlying `ConnectionManager`.

- `ZZPING_MAX_PEERS` — optional integer. When set, the SessionManager will reject additional peer registrations once this many peers are present.
- `ZZPING_MAX_ROOMS_PER_PEER` — optional integer. When set, adding a peer that has more than this number of rooms will be rejected.

Example (bash):

```bash
# Allow at most 50 peers and at most 4 rooms per peer
export ZZPING_MAX_PEERS=50
export ZZPING_MAX_ROOMS_PER_PEER=4
cargo run --bin some-server
```

If the variables are not set, no limits are enforced (legacy behavior).
