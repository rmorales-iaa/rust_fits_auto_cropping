#!/usr/bin/env bash
set -euo pipefail

readonly FIRST_YEAR=2002
readonly LAST_YEAR=2026
readonly SOURCE_ROOT="${RUST_FITS_AUTO_CROPPING_SOURCE_ROOT:-/mnt/uxmal_groups/image_storage/rust_om_cal}"
readonly OUTPUT_ROOT="${RUST_FITS_AUTO_CROPPING_OUTPUT_ROOT:-/mnt/uxmal_groups/image_storage/cropped}"
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
Missing source calibration directories are reported and omitted; science images
are still processed because calibration products are not inputs to this crop step.
Set RUST_FITS_AUTO_CROPPING_BIN to override the executable. By default the
script uses rust_fits_auto_cropping from PATH, then this repository's release binary.
Set RUST_FITS_AUTO_CROPPING_SOURCE_ROOT and RUST_FITS_AUTO_CROPPING_OUTPUT_ROOT
to override the data roots. A per-year progress bar counts completed FITS files.
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

count_fits_inputs() {
    find "$1" -type f \
        \( -iname '*.fits.gz' -o -iname '*.fit.gz' -o -iname '*.fts.gz' \
        -o -iname '*.fits.fz' -o -iname '*.fit.fz' -o -iname '*.fts.fz' \
        -o -iname '*.fits' -o -iname '*.fit' -o -iname '*.fts' -o -iname '*.fz' \) \
        -printf '.' | wc -c | tr -d '[:space:]'
}

render_progress() {
    local year=$1
    local completed=$2
    local total=$3
    local width=30
    local percent=$((completed * 100 / total))
    local filled=$((completed * width / total))
    local bar empty
    printf -v bar '%*s' "$filled" ''
    printf -v empty '%*s' "$((width - filled))" ''
    bar=${bar// /#}
    empty=${empty// /-}
    if [[ -t 2 ]]; then
        printf '\rYear %s [%s%s] %3d%% (%d/%d)' \
            "$year" "$bar" "$empty" "$percent" "$completed" "$total" >&2
    else
        printf 'Year %s [%s%s] %3d%% (%d/%d)\n' \
            "$year" "$bar" "$empty" "$percent" "$completed" "$total" >&2
    fi
}

run_year_with_progress() {
    local year=$1
    local input_dir=$2
    local output_dir=$3
    local total completed last_percent status_file program_pid activity_file candidate line status
    local had_nullglob=0
    local -A prior_logs=()
    total=$(count_fits_inputs "$input_dir")
    completed=0
    last_percent=-1

    if shopt -q nullglob; then
        had_nullglob=1
    fi
    shopt -s nullglob
    if [[ -d "$output_dir" ]]; then
        for candidate in "$output_dir"/activity_*.jsonl; do
            [[ -f "$candidate" ]] && prior_logs["$candidate"]=1
        done
    fi
    if ((had_nullglob == 0)); then
        shopt -u nullglob
    fi

    if ((total > 0)); then
        render_progress "$year" 0 "$total"
        last_percent=0
    else
        printf 'Year %s: no matching FITS files found.\n' "$year" >&2
    fi

    status_file=$(mktemp "${TMPDIR:-/tmp}/cropped-year-status.XXXXXX")
    (
        set +e
        "$PROGRAM" \
            --input-dir "$input_dir" \
            --output-dir "$output_dir" \
            --recursive \
            --mode crop \
            --flat-output \
            --png \
            --sanitize-output \
            --config "$CONFIG_PATH" \
            --heuristics "$HEURISTICS_PATH"
        status=$?
        printf '%s\n' "$status" >"$status_file"
    ) &
    program_pid=$!

    activity_file=''
    while [[ -z "$activity_file" && ! -s "$status_file" ]]; do
        shopt -s nullglob
        for candidate in "$output_dir"/activity_*.jsonl; do
            [[ -f "$candidate" ]] || continue
            if [[ -z ${prior_logs[$candidate]+known} ]]; then
                activity_file=$candidate
                break
            fi
        done
        if ((had_nullglob == 0)); then
            shopt -u nullglob
        fi
        [[ -n "$activity_file" || -s "$status_file" ]] && break
        sleep 0.1
    done

    if [[ -n "$activity_file" && $total -gt 0 ]]; then
        exec {activity_fd}< <(tail -n +1 -F -- "$activity_file")
        local tail_pid=$!
        while [[ ! -s "$status_file" ]]; do
            if IFS= read -r -t 0.2 line <&"$activity_fd"; then
                if [[ "$line" == *'"event":"file_finished"'* ]]; then
                    completed=$((completed + 1))
                    if ((completed > total)); then
                        completed=$total
                    fi
                    local percent=$((completed * 100 / total))
                    if ((percent != last_percent)); then
                        render_progress "$year" "$completed" "$total"
                        last_percent=$percent
                    fi
                fi
            fi
        done
        while IFS= read -r -t 0.1 line <&"$activity_fd"; do
            if [[ "$line" == *'"event":"file_finished"'* ]]; then
                completed=$((completed + 1))
                if ((completed > total)); then
                    completed=$total
                fi
            fi
        done
        if ((completed == total && last_percent != 100)); then
            render_progress "$year" "$completed" "$total"
        fi
        if [[ -t 2 ]]; then
            printf '\n' >&2
        fi
        kill "$tail_pid" 2>/dev/null || true
        wait "$tail_pid" 2>/dev/null || true
        exec {activity_fd}<&-
    fi

    wait "$program_pid" || true
    IFS= read -r status <"$status_file" || status=1
    rm -f -- "$status_file"
    return "$status"
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
        if [[ ! -d "$source_year/$calibration_dir" ]]; then
            printf 'Year %s has no source %s/ directory; continuing without that calibration link.\n' \
                "$year" "$calibration_dir" >&2
            continue
        fi
        if ! ensure_directory_link \
            "$source_year/$calibration_dir" \
            "$output_year/$calibration_dir"; then
            printf 'Skipping science processing for year %s because a calibration link conflicts with its source.\n' \
                "$year" >&2
            return 1
        fi
    done

    printf 'Processing year %s: %s -> %s\n' \
        "$year" "$source_year/science_calibrated" "$output_year/science_calibrated"
    run_year_with_progress \
        "$year" \
        "$source_year/science_calibrated" \
        "$output_year/science_calibrated"
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
