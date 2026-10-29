//! Pixel-action bookkeeping codes; these are not science-validity labels.

/// Build a full-frame reason plane from original sample validity, detector
/// evidence, and pixels excluded by the selected action.
///
/// Codes are 0 retained unflagged sample, 1 detector-selected artifact
/// exclusion, 2 crop-only sacrifice of a defined pixel, 3 pre-existing
/// undefined input sample, and 4 detector-flagged sample retained by the crop.
pub fn build(
    values: &[f64],
    detector_mask: &[bool],
    action_exclusion: &[bool],
) -> anyhow::Result<Vec<u8>> {
    if values.len() != detector_mask.len() || values.len() != action_exclusion.len() {
        anyhow::bail!("reason-plane inputs must have identical lengths");
    }
    Ok(values
        .iter()
        .zip(detector_mask)
        .zip(action_exclusion)
        .map(|((value, detected), excluded)| {
            if !value.is_finite() {
                3
            } else if *excluded && *detected {
                1
            } else if *excluded {
                2
            } else if *detected {
                4
            } else {
                0
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::build;

    #[test]
    fn codes_distinguish_artifact_crop_sacrifice_and_preexisting_undefined() {
        let values = [10.0, 11.0, 12.0, f64::NAN, 14.0];
        let detector = [true, false, true, false, true];
        let excluded = [true, true, false, true, false];
        assert_eq!(
            build(&values, &detector, &excluded).unwrap(),
            [1, 2, 4, 3, 4]
        );
    }

    #[test]
    fn detector_evidence_is_not_reported_as_removed_when_action_keeps_it() {
        assert_eq!(build(&[1.0], &[true], &[false]).unwrap(), [4]);
    }

    #[test]
    fn rejects_mismatched_input_lengths() {
        assert!(build(&[1.0], &[false, true], &[false]).is_err());
    }
}
