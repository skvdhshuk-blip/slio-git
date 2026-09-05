# Desktop and Mac App Store distribution

One source revision builds both channels. `app-store` selects the sandbox adapter;
without it, desktop execution is preserved from `d83a21d`. The import baseline is
`42af1d0`. Both remote refs matched these local refs at implementation start.

## Ownership

| Concern | Source of truth | Consumer |
|---|---|---|
| Channel | Cargo `app-store` feature, selected at module boundaries | Capability descriptions in the existing UI |
| Git rules | Shared public APIs, types and validation | Desktop and native execution adapters |
| Native operations | Versioned journal in the actual per-worktree Git directory | Both channels' status, conflict, amend and recovery APIs |
| File access | Lease retaining the original resolved security-scoped URL | Repository session, file watcher, queued task and credential snapshot |
| Identity | Repository name/email, with explicit application fallback for MAS | Commit, amend, merge, stash, tag and replay |
| Network credentials | One immutable config/identity/access snapshot per operation | Native transport callbacks |
| Distribution | Cargo's reported executable and package manifest | Signing, DMG/PKG creation and release uploads |

```text
shared Git API and validation
├── desktop.rs  main's existing execution for new operations
├── native.rs   MAS execution without external commands
└── native/
    ├── journal.rs    durable identity, transitions, snapshots and locking
    ├── sequencer.rs  replay, conflict/edit pauses, publish, continue/abort
    └── merge.rs      fast-forward, merge and squash through the same journal
```

The MAS binary has no executable system-Git, credential-helper, GPG, ssh-agent or
GitHub-updater implementation. Hooks and those integrations remain desktop
capabilities. Clipboard writes use Iced; macOS file/URL opening uses NSWorkspace.
The winit private-symbol patch remains in both builds.

## Native operation contract

The journal lives at `<actual Git directory>/slio-operation/state.json`, alongside
an OS process lock. A linked worktree therefore has its own operation identity.
No synthetic `rebase-merge` or `rebase-apply` directory is created. External Git
operations remain readable; MAS refuses to mutate their index, conflict files or
recovery markers and asks the user to finish in the originating tool. Desktop's
existing takeover paths remain available.

Each native operation records the original symbolic/detached HEAD, original index,
recovery reference, plan/cursor, generated commits, author/committer data, owned
paths, expected images and phase. Immutable Git objects precede a durable intent;
file/index/HEAD changes follow it. Reference/object writes use libgit2 fsync mode,
and journal/index/file writes and their directory entries are synced. Each file
transition accepts its recorded before/after state. File/directory replacements
also recognize their owned removal boundary. File contents and executable mode
are installed together. Unexpected third-party values stop recovery.

The original branch remains unchanged while replay runs on detached HEAD. Final
publication compares the original branch OID before replacing it. Completed and
aborted phases can finish cleanup after restart. Conflict, edit and squash-merge
pauses use existing conflict and commit/amend interfaces. Abort restores owned
changes and preserves unrelated files; unrelated staged changes or altered
repository identity require attention instead of being overwritten.

Interactive history keeps the unpublished, first-parent, no-merge restriction.
Root commits, reordered picks, edit/reword, fixup, squash, drop and empty commits
use the same replay engine. Fixup keeps the predecessor's message; squash combines
messages. The first retained entry cannot be fixup/squash. The existing UI's UTF-8
path contract is retained: native history stops on non-UTF-8 paths and preserves
its recovery data instead of guessing a lossy path.

Native pulls select the exact branch and honor merge/rebase, ff-only, no-ff, squash
and per-operation autocrlf. Incompatible combinations return an explicit error.
Force-with-lease freezes the expected remote OID, checks the server advertisement
before upload and checks every changed reference's final result. A rejection or
missing result cannot become success or trigger an automatic retry. Local Git
receive-pack also enforces the old OID if another writer wins during upload.

## Authorization and compatibility

The resolved bookmark URL owns start/stop access until the final lease disappears.
Refreshing a stale bookmark updates the same persisted bookmark format. The grant
root and requested repository path are distinct, including moved parent folders.
Open/recent/local-clone/worktree paths acquire their external Git/common directories
before opening libgit2. Directory reselection resumes the same requested operation.
SSH key snapshots hold the exact key's lease; editing settings cannot revoke an
in-flight transport. No adjacent `.pub` file is guessed. A save-panel grant permits
writing the selected patch file, not an arbitrary sibling temporary file.

Bundle ID, settings locations and bookmark encoding are unchanged. This is not a
side-by-side installation feature, configuration migration or App Store submission.

## Build and release

Existing shell entry points delegate to `scripts/package-macos.py`:

```sh
bash scripts/package-macos-dmg.sh
bash scripts/package-macos-appstore.sh --preflight-only
bash scripts/package-macos-appstore.sh
bash scripts/package-macos-appstore.sh --mode sandbox-test
```

`MACOS_TARGET` selects `aarch64-apple-darwin` or `x86_64-apple-darwin`; `MACOS_ARCH`,
when supplied, must agree. Build caches are isolated by channel and architecture.
Output paths are `dist/desktop/<arch>/` and
`dist/mas/<arch>/<distribution|sandbox-test>/`. Each invocation assembles separately
and accepts only the current Cargo JSON executable, never an old-path fallback.

Distribution preflight validates the application/installer identities, team,
certificate membership, profile expiry, bundle ID and restricted entitlements
before Cargo runs. Missing inputs fail. Sandbox-test is explicitly ad-hoc signed,
keeps sandbox capabilities, excludes distribution-only identity claims and never
produces a distribution PKG. Final architecture, signature, entitlements and MAS
private-symbol absence are checked. Manifests identify source commit/dirty digest,
channel, features, version, build number, architecture, exact binary and package
SHA-256. Release uploads carry the manifest. Pages only publishes from `main`.

## Verification and rollback

Run the channels separately; `--all-features` does not test desktop behavior:

```sh
cargo test --workspace --no-default-features
cargo test --workspace --features app-store
cargo clippy -p git-core -p src-ui --all-targets --no-default-features
cargo clippy -p git-core -p src-ui --all-targets --features app-store
python3 -m unittest discover -s scripts/tests
```

`native::tests` uses temporary real Git repositories and standard Git as an
independent oracle. It checks trees, ancestry, authors/messages, index/worktree,
root/reordered/empty operations, conflict continue/skip/abort, linked-worktree
identity and crashes at journal/file/index/HEAD/publication/directory-replacement
boundaries. `native_remote` uses a real local git daemon and receive hooks, including
stale lease, frozen confirmation, rejected references, partial success and a remote
update during upload. The advanced-operation tests previously excluded from MAS
run in both channels. The editor's font regression asserts finite public layout
metrics, which include the existing missing-font fallback, instead of requiring
uninitialized raw font measurement to succeed.

Signed sandbox acceptance is separate from tests: open a chosen repository, pause
reword, restart, amend and continue; move the directory and restore its bookmark;
clone, select an SSH key, export a patch outside the repository, use clipboard,
and reopen a linked worktree with its external Git directory. Verify the exported
email with standard `git am`. Exercise desktop recovery of a native pause too.
Record exact loaded artifact identity and results alongside the package manifests.
Build the channels alternately and compare the other channel's hashes.

Keep `mas-sandbox` at its historical tip and retain existing release artifacts.
Before downgrading, complete or abort native transactions and keep a new binary
for journal recovery. Do not delete or migrate user configuration. If outside
changes prevent safe recovery, preserve the journal, images and
`refs/slio/recovery/<operation-id>` until the user reconciles them. No performance
claim or App Review approval follows from build/test/package success.

References: [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html),
[Git pull](https://git-scm.com/docs/git-pull),
[libgit2 negotiation](https://libgit2.org/docs/reference/main/remote/git_push_negotiation.html),
[reference results](https://libgit2.org/docs/reference/main/remote/git_push_update_reference_cb.html),
[Apple scoped access](https://developer.apple.com/documentation/foundation/url/startaccessingsecurityscopedresource()).
