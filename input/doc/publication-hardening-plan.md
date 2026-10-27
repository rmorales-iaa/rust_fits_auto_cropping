# Product-pair publication hardening

Status: implemented and validated on 2026-10-27.

## Scope and invariant

The image and companion mask form one logical output set. A completion manifest remains the only completion signal and is published after both products pass verification and product hashes are computed. Filesystem rename operations do not provide a transaction across the separate `images/` and `masks/` directories, so a process crash between final renames can still leave one visible product without a manifest. Readers must treat a missing marker as incomplete.

## Phases

1. **Done:** write and verify both products in temporary sibling directories on the destination filesystems. Do not expose either new product while encoding or verification is in progress.
2. **Done:** back up any explicit-overwrite targets, publish the verified staged files, and restore the prior pair if either final rename fails.
3. **Done:** deterministic failure injection at the pair-publication boundary verifies that a failure on the second rename restores both old files, or removes the first newly published file when no prior file existed.
4. **Done:** document the crash window and the requirement to trust the completion manifest. Do not imply cross-directory transactional atomicity.

## Acceptance

- **Passed:** existing mask and crop product writers use the staged pair helper.
- **Passed:** forced second-publication errors restore prior image and mask bytes; the no-prior-output case leaves no partial product.
- **Passed:** existing pixel/header/checksum tests, 64 Rust tests, formatting and strict Clippy; release `0.0.93` passed independent Astropy/fitsverify checks and source/product digest verification on the supplied corpus.
- **Done:** README, user guide, validation record and project memory describe the exact guarantee and remaining crash window.

## Deferred

Power-loss/fsync durability, whole-run transactions, recovery or deletion of stale unmanifested outputs, and filesystem-specific crash testing remain outside this implementation slice.
