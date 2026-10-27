# Optional pixel-reason plane plan

Date: 2026-10-27

## Goal and scope

Add an opt-in FITS sidecar that explains the processing action for every input
pixel. Codes describe the program's bookkeeping, not ground-truth science
validity or calibrated detector confidence:

| Code | Meaning |
| --- | --- |
| 0 | Defined input sample retained by the selected output geometry/action |
| 1 | Pixel excluded by the accepted detector artifact mask |
| 2 | Defined pixel sacrificed only by crop geometry |
| 3 | Input sample was already undefined (NaN or FITS integer BLANK) |

Code 3 takes precedence over crop/action codes. The plane is emitted only for
accepted actions that already produce a science image and companion mask.
Unchanged symlink outputs and proposals do not get a reason plane.

## Implementation phases

1. Add a typed, deterministic code-plane builder with checked dimensions and
   precedence tests for mask and crop actions.
2. Add optional CLI control and place the FITS sidecar in the existing `masks/`
   output category. Preserve the source header's WCS and provenance while
   recording the code definitions. Verify written dimensions, samples, and
   FITS readability before reporting success.
3. Add the output path to JSON/CSV outcomes and the completion manifest's
   SHA-256 product list. Preserve the existing binary mask product.
4. Cover opt-in/off behavior, undefined pixels, crop-only sacrifice, FITS
   sample/header checks, report and manifest digests, and write failure behavior.
5. Update CLI/readme/user/algorithm/validation docs and memory, run all
   project gates, inspect the diff, and commit the coherent verified change.

## Acceptance criteria

- Default invocation emits no additional product and preserves existing output
  semantics.
- Every emitted reason plane has dimensions matching the source frame and only
  codes 0..3.
- Existing undefined samples are represented separately from proposed artifact
  candidates and crop geometry sacrifices.
- A completion marker cannot list the reason plane until it has been written,
  verified, and hashed.
- Tests do not interpret these codes as labels of astrophysical validity.
- Documentation states that the feature is diagnostic and does not close the
  external science-validation gate.

## Remaining constraints

The independent, grouped, labeled-data gate in `scientific-improvement-plan.md`
remains open. This feature improves auditability only; it does not increase
detector accuracy or replace source-aware boundary inference.


## Completion record

Implemented in source commit after verification: optional CLI flag and isolated `reason_masks/` output category; reason schema/code builder; writer FITS verification and legacy HISTORY normalization; schema-7 JSON/CSV path and manifest product checksum; unit, end-to-end, crop-sacrifice and interoperability coverage. The requested development dataset was processed and independently checked with Astropy 7.2.0 and fitsverify 4.16. Five paired measurements compared release runs with the option on/off; the results were inconclusive for performance because environmental variance dominated. See `validation.md` for run IDs, counts and benchmark evidence.
