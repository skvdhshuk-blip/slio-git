## ADDED Requirements

### Requirement: Prefill commit message from Git-prepared merge template

When the repository is in the merging state, slio-git SHALL prefill the commit dialog's message editor with the commit message Git has prepared on disk (`.git/MERGE_MSG`), so the user can commit the merge without retyping the summary and conflicts block.

#### Scenario: MERGE_MSG present after conflict resolution

- **WHEN** the repository transitions to the merging state and `.git/MERGE_MSG` exists with non-empty contents
- **AND** the commit dialog's message editor is empty or still contains the previously prefilled default
- **THEN** the commit dialog's message is set to the exact contents of `.git/MERGE_MSG` (preserving newlines and the conflicts block, with trailing whitespace trimmed)

#### Scenario: MERGE_MSG present but editor already edited

- **WHEN** the repository is in the merging state
- **AND** the user has typed or modified the commit message so that it no longer matches the previously prefilled default
- **THEN** slio-git MUST NOT overwrite the user's content when re-checking repository state

### Requirement: Synthesize a fallback merge message when MERGE_MSG is absent

When the repository is in the merging state but `.git/MERGE_MSG` is missing, unreadable, or empty, slio-git SHALL synthesize a default message from `MERGE_HEAD` and the current branch, preserving Git's conventional English wording so the resulting commit is consistent with the rest of `git log`.

#### Scenario: Single-source merge with named branch

- **WHEN** `MERGE_HEAD` contains exactly one commit OID that resolves to a branch short name `<source>` and the current branch is `<target>`
- **AND** `.git/MERGE_MSG` is missing, unreadable, or empty
- **THEN** the synthesized message is `Merge branch '<source>' into <target>`

#### Scenario: Single-source merge with unresolvable name

- **WHEN** `MERGE_HEAD` contains one commit OID that does not resolve to a branch short name
- **AND** `.git/MERGE_MSG` is missing, unreadable, or empty
- **THEN** the synthesized message is `Merge commit '<short-oid>' into <target>`, where `<short-oid>` is the 7-character prefix of the OID

#### Scenario: Multi-source (octopus) merge fallback

- **WHEN** `MERGE_HEAD` contains multiple commit OIDs
- **AND** `.git/MERGE_MSG` is missing, unreadable, or empty
- **THEN** the synthesized message is `Merge branches '<a>', '<b>'[, '<c>' ...] into <target>`, substituting a short OID for any OID that cannot be resolved to a branch name

#### Scenario: Detached HEAD

- **WHEN** the current `HEAD` is detached and has no branch short name
- **AND** `.git/MERGE_MSG` is missing, unreadable, or empty
- **THEN** the synthesized message omits the ` into <target>` suffix, yielding `Merge branch '<source>'` or `Merge commit '<short-oid>'`

### Requirement: Clear the prefilled default when the merge completes or is aborted

slio-git SHALL treat the prefilled merge message as state scoped to the current merge session, so the next non-merge commit starts with an empty editor as before.

#### Scenario: Successful merge commit

- **WHEN** the merge commit is created and the repository leaves the merging state
- **THEN** the stored default merge message is cleared, and the next time the commit dialog opens it starts empty

#### Scenario: Merge aborted via Quit Merge

- **WHEN** the user aborts the in-progress merge (clearing `MERGE_HEAD` and `MERGE_MSG`)
- **THEN** the stored default merge message is cleared, and the next time the commit dialog opens it starts empty

### Requirement: Non-merge commit flows remain unchanged

slio-git MUST NOT prefill any default message when the repository is not in the merging state, so normal commits, amends, rebase continuations, cherry-picks, and reverts retain their current behavior.

#### Scenario: Normal commit with clean repository state

- **WHEN** the repository state is `Clean` and the user opens the commit dialog for a new commit
- **THEN** the commit dialog's message editor starts empty, identical to the behavior before this change

#### Scenario: Amending the previous commit

- **WHEN** the user switches the commit dialog to amend mode on a non-merge commit
- **THEN** the message editor is populated from the amended commit's original message, not from any merge template
