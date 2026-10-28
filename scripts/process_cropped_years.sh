#!/usr/bin/env bash
set -euo pipefail

readonly FIRST_YEAR=2002
readonly LAST_YEAR=2026
readonly SOURCE_ROOT=/mnt/uxmal_groups/image_storage/rust_om_cal
readonly OUTPUT_ROOT=/mnt/uxmal_groups/image_storage/cropped
readonly SCRIPT_PATH="$(readlink -f -- "${BASH_SOURCE[0]}")"
readonly REPOSITORY_ROOT="$(cd -- "$(dirname -- "$SCRIPT_PATH")/.." && pwd -P)"
readonly CONFIG_PATH="$REPOSITORY_ROOT/input/config.toml"
readonly HEURISTICS_PATH="$REPOSITORY_ROOT/input/heuristics.toml"

usage() {
    cat <<USAGE
Usage: $(basename "$0") [--year YEAR]

Without arguments, process every year from $FIRST_YEAR through $LAST_YEAR.
Use --year YEAR to process one year (for example, --year 2002).
Each run deletes existing contents of that year's dedicated science_calibrated
output directory before processing. Sibling bias/dark/flat links are unaffected.
Set RUST_FITS_AUTO_CROPPING_BIN to override the executable. By default the
script uses rust_fits_auto_cropping from PATH, then this repository's release binary.
USAGE
}

selected_years=()
if (($# == 1)) && [[ "$1" == "--help" || "$1" == "-h" ]]; then
    usage
    exit 0
elif (($# == 0)); then
    for ((year = FIRST_YEAR; year <= LAST_YEAR; year++)); do
        selected_years+=("$year")
    done
elif (($# == 2)) && [[ "$1" == "--year" && "$2" =~ ^[0-9]{4}$ ]]; then
    if ((10#$2 < FIRST_YEAR || 10#$2 > LAST_YEAR)); then
        printf 'Year must be between %s and %s: %s\n' "$FIRST_YEAR" "$LAST_YEAR" "$2" >&2
        exit 2
    fi
    selected_years+=("$((10#$2))")
else
    usage >&2
    exit 2
fi

if [[ -n "${RUST_FITS_AUTO_CROPPING_BIN:-}" ]]; then
    readonly PROGRAM="$RUST_FITS_AUTO_CROPPING_BIN"
elif command -v rust_fits_auto_cropping >/dev/null 2>&1; then
    readonly PROGRAM="$(command -v rust_fits_auto_cropping)"
else
    readonly PROGRAM="$REPOSITORY_ROOT/target/release/rust_fits_auto_cropping"
fi

if [[ ! -x "$PROGRAM" ]]; then
    printf 'Executable not found or not executable: %s\n' "$PROGRAM" >&2
    printf 'Build it with: cargo build --release --manifest-path %q/Cargo.toml\n' "$REPOSITORY_ROOT" >&2
    exit 2
fi
for required_file in "$CONFIG_PATH" "$HEURISTICS_PATH"; do
    if [[ ! -f "$required_file" ]]; then
        printf 'Required configuration file not found: %s\n' "$required_file" >&2
        exit 2
    fi
done

ensure_directory_link() {
    local source_dir=$1
    local output_link=$2
    local resolved_source
    local resolved_link

    if [[ ! -d "$source_dir" ]]; then
        printf 'Required source directory not found: %s\n' "$source_dir" >&2
        return 1
    fi
    resolved_source="$(readlink -f -- "$source_dir")"
    if [[ -L "$output_link" ]]; then
        resolved_link="$(readlink -f -- "$output_link")"
        if [[ "$resolved_link" == "$resolved_source" ]]; then
            return 0
        fi
        printf 'Existing symlink points elsewhere: %s -> %s (expected %s)\n' \
            "$output_link" "$resolved_link" "$resolved_source" >&2
        return 1
    fi
    if [[ -e "$output_link" ]]; then
        printf 'Refusing to replace existing non-symlink path: %s\n' "$output_link" >&2
        return 1
    fi
    ln -s -- "$resolved_source" "$output_link"
}

process_year() {
    local year=$1
    local source_year="$SOURCE_ROOT/$year"
    local output_year="$OUTPUT_ROOT/$year"
    local calibration_dir

    if [[ ! -d "$source_year/science_calibrated" ]]; then
        printf 'Skipping year %s: science input directory is missing: %s\n' \
            "$year" "$source_year/science_calibrated" >&2
        return 1
    fi
    mkdir -p -- "$output_year"
    for calibration_dir in bias dark flat; do
        if ! ensure_directory_link \
            "$source_year/$calibration_dir" \
            "$output_year/$calibration_dir"; then
            printf 'Skipping science processing for year %s because calibration links are incomplete.\n' \
                "$year" >&2
            return 1
        fi
    done

    printf 'Processing year %s: %s -> %s\n' \
        "$year" "$source_year/science_calibrated" "$output_year/science_calibrated"
    "$PROGRAM" \
        --input-dir "$source_year/science_calibrated" \
        --output-dir "$output_year/science_calibrated" \
        --recursive \
        --mode crop \
        --flat-output \
        --png \
        --sanitize-output \
        --config "$CONFIG_PATH" \
        --heuristics "$HEURISTICS_PATH"
}

failures=0
for year in "${selected_years[@]}"; do
    if ! process_year "$year"; then
        failures=$((failures + 1))
    fi
done

if ((failures > 0)); then
    printf '%s year(s) failed; see the run summaries and activity logs above.\n' "$failures" >&2
    exit 1
fi
