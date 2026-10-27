# Primary technical references

Consulted 2026-10-27. Pin dependency versions during the feasibility phase; documentation URLs may evolve.

- [fitskit crate documentation](https://docs.rs/fitskit/): pure-Rust FITS HDU, pixel, header and checksum read/write API used by this implementation.
- [fitskit crate source](https://github.com/ssmichael1/fitskit): implementation details and issue tracker for supported FITS conventions.
- [IAU FITS Standard](https://fits.gsfc.nasa.gov/fits_standard.html): authoritative format specification.
- [NASA FITS keyword dictionary](https://heasarc.gsfc.nasa.gov/docs/fcg/standard_dict.html): structural keywords and reference-pixel conventions.
- [FITS reserved keywords](https://fits.gsfc.nasa.gov/users_guide/users_guide/node22.html): stored integer BLANK versus floating NaN and scaling behavior.
- [FITS checksum convention](https://fits.gsfc.nasa.gov/registry/checksum.html): integrity updates after pixel/header modifications.

## Methods and validation references checked during the deep review

- [Photutils background estimation](https://photutils.readthedocs.io/en/stable/user_guide/background.html) and [Background2D API](https://photutils.readthedocs.io/en/latest/api/photutils.background.Background2D.html): source masking, local mesh estimates and valid/coverage masks. Independent reference tools for estimator experiments; not a production Python dependency or endorsement of this detector's thresholds.
- [Statsmodels HuberT](https://www.statsmodels.org/stable/generated/statsmodels.robust.norms.HuberT.html): Huber criterion/IRLS weight definitions for independent numerical comparison. Its defaults are not calibrated astronomical detection thresholds.
- [Felzenszwalb and Huttenlocher (2012), Distance Transforms of Sampled Functions](https://theoryofcomputing.org/articles/v008a019/), DOI `10.4086/toc.2012.v008a019`: linear-time grid distance-transform method. Does not prove maximality of an application-specific circle-selection/clearance rule.
- [Lahiri and Zhu (2006), Resampling methods for spatial regression models under a class of stochastic designs](https://projecteuclid.org/journals/annals-of-statistics/volume-34/issue-4/Resampling-methods-for-spatial-regression-models-under-a-class-of/10.1214/009053606000000551.pdf), DOI `10.1214/009053606000000551`: spatial resampling assumptions matter; naive extension across sampling designs can fail. The planned image/grid/group resampling scheme must establish its own validity and empirical coverage.
- [Astropy Cutout2D](https://docs.astropy.org/en/stable/api/astropy.nddata.Cutout2D.html) and [WCS reference](https://docs.astropy.org/en/stable/wcs/reference_api.html): independent coordinate checks and explicit limitations around distortion lookup tables. Passing a basic CRPIX shift is not evidence for all WCS conventions.

These primary references support specific design choices. They do not validate the border detector or supply universal clipping thresholds. Consult the [active plan](scientific-improvement-plan.md) for experiments, assumptions and release evidence. Pin the exact reference-library versions used in each evaluation; live documentation may describe a newer version than the local tools.
