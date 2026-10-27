# Primary technical references

Consulted 2026-10-27. Pin dependency versions during the feasibility phase; documentation URLs may evolve.

- [Rust fitsio API](https://docs.rs/fitsio/latest/fitsio/): candidate CFITSIO wrapper, HDU inspection and image operations. Prototype exact copy and update semantics before selecting the writer.
- [IAU FITS Standard](https://fits.gsfc.nasa.gov/fits_standard.html): authoritative format specification.
- [NASA FITS keyword dictionary](https://heasarc.gsfc.nasa.gov/docs/fcg/standard_dict.html): structural keywords and reference-pixel conventions.
- [FITS reserved keywords](https://fits.gsfc.nasa.gov/users_guide/users_guide/node22.html): stored integer BLANK versus floating NaN and scaling behavior.
- [CFITSIO concurrent access](https://heasarc.gsfc.nasa.gov/docs/software/fitsio/c/c_user/node30.html): reentrant builds, independently owned handles and thread constraints.
- [FITS checksum convention](https://fits.gsfc.nasa.gov/registry/checksum.html): integrity updates after pixel/header modifications.

These references support file-format and implementation choices. They do not validate the proposed border detector. The algorithm phase must add method-specific references and empirical validation for its final estimators and decision rules.
