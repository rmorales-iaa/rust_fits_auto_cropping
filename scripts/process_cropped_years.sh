#!/usr/bin/env bash
# Crop calibrated science images year by year into fixed yearly storage.
# See --help. Each selected year's dedicated science_calibrated/ output root is
# sanitized (all entries deleted) before processing; nothing else is deleted.
set -euo pipefail

readonly FIRST_YEAR=2002
readonly LAST_YEAR=2026
readonly DEFAULT_SOURCE_ROOT=/mnt/uxmal_groups/image_storage/rust_om_cal
readonly DEFAULT_OUTPUT_ROOT=/mnt/uxmal_groups/image_storage/cropped
SOURCE_ROOT="${RUST_FITS_AUTO_CROPPING_SOURCE_ROOT:-$DEFAULT_SOURCE_ROOT}"
OUTPUT_ROOT="${RUST_FITS_AUTO_CROPPING_OUTPUT_ROOT:-$DEFAULT_OUTPUT_ROOT}"
SCRIPT_PATH="$(readlink -f -- "${BASH_SOURCE[0]}")"
REPOSITORY_ROOT="$(cd -- "$(dirname -- "$SCRIPT_PATH")/.." && pwd -P)"
CONFIG_PATH="$REPOSITORY_ROOT/input/config.toml"
HEURISTICS_PATH="$REPOSITORY_ROOT/input/heuristics.toml"
readonly SCRIPT_PATH REPOSITORY_ROOT CONFIG_PATH

usage() {
    cat <<USAGE
Usage: $(basename "$0") [options]

Crop the calibrated science images of each selected year from
  SOURCE_ROOT/<year>/science_calibrated/  into  OUTPUT_ROOT/<year>/science_calibrated/
with flat output, linking SOURCE_ROOT/<year>/{bias,dark,flat} beside it.

Year selection (default: every year from $FIRST_YEAR through $LAST_YEAR):
  --year YEAR          Process YEAR; repeat for several years.
  --years LIST         Comma-separated years, e.g. 2009,2011,2017.
  --from YEAR --to YEAR
                       Inclusive range (either bound may be omitted).

Processing:
  --mode MODE          crop (default), crop_masked or mask.
  --no-png             Do not write PNG previews (default writes them).
  --heuristics PATH    Science rule file (default input/heuristics.toml), e.g.
                       a copy selecting the legacy detector = "tile_v1".
  --plan               Validate paths and print what would run (FITS counts,
                       sanitation targets); write and delete nothing.
  -h, --help           Show this help.

DELETION: before each year runs, every existing entry inside that year's
dedicated OUTPUT_ROOT/<year>/science_calibrated/ is deleted (--sanitize-output).
Sibling bias/dark/flat links and the source tree are never modified. The script
refuses to run if the roots are relative, the filesystem root, equal, or nested
in each other, or if the output directory is a symlink.

Environment:
  RUST_FITS_AUTO_CROPPING_SOURCE_ROOT  (default $DEFAULT_SOURCE_ROOT)
  RUST_FITS_AUTO_CROPPING_OUTPUT_ROOT  (default $DEFAULT_OUTPUT_ROOT)
  RUST_FITS_AUTO_CROPPING_BIN          executable (default: PATH, then this
                                       repository's target/release binary)

Exit status: 0 all years complete; 1 a year failed, was interrupted, or
completed with input errors (malformed inputs are preserved as annotated
symlinks); 2 usage or configuration error; 130 interrupted by the user.
Missing source calibration directories are reported and skipped; they are not
inputs to the crop step. Two runs cannot use the same OUTPUT_ROOT at once.
USAGE
}

die() {
    printf '%s\n' "$*" >&2
    exit 2
}

valid_year() {
    [[ "$1" =~ ^[0-9]{4}$ ]] && ((10#$1 >= FIRST_YEAR && 10#$1 <= LAST_YEAR))
}

# ---------------------------------------------------------------- arguments
requested_years=()
range_from=''
range_to=''
MODE=crop
PNG=1
PLAN_ONLY=0
while (($# > 0)); do
    case "$1" in
    -h | --help)
        usage
        exit 0
        ;;
    --year)
        (($# >= 2)) || die "--year needs a value"
        valid_year "$2" || die "Year must be between $FIRST_YEAR and $LAST_YEAR: $2"
        requested_years+=("$((10#$2))")
        shift 2
        ;;
    --years)
        (($# >= 2)) || die "--years needs a value"
        IFS=',' read -r -a listed <<<"$2"
        ((${#listed[@]} > 0)) || die "--years needs at least one year"
        for value in "${listed[@]}"; do
            valid_year "$value" || die "Year must be between $FIRST_YEAR and $LAST_YEAR: $value"
            requested_years+=("$((10#$value))")
        done
        shift 2
        ;;
    --from)
        if (($# < 2)) || ! valid_year "$2"; then
            die "--from needs a year between $FIRST_YEAR and $LAST_YEAR"
        fi
        range_from=$((10#$2))
        shift 2
        ;;
    --to)
        if (($# < 2)) || ! valid_year "$2"; then
            die "--to needs a year between $FIRST_YEAR and $LAST_YEAR"
        fi
        range_to=$((10#$2))
        shift 2
        ;;
    --mode)
        (($# >= 2)) || die "--mode needs a value"
        case "$2" in
        crop | crop_masked | mask) MODE=$2 ;;
        *) die "--mode must be crop, crop_masked or mask: $2" ;;
        esac
        shift 2
        ;;
    --no-png)
        PNG=0
        shift
        ;;
    --heuristics)
        (($# >= 2)) || die "--heuristics needs a path"
        HEURISTICS_PATH=$2
        shift 2
        ;;
    --plan)
        PLAN_ONLY=1
        shift
        ;;
    *)
        usage >&2
        exit 2
        ;;
    esac
done
readonly MODE PNG PLAN_ONLY

if [[ -n "$range_from$range_to" ]]; then
    ((${#requested_years[@]} == 0)) || die "Use either --year/--years or --from/--to, not both"
    from=${range_from:-$FIRST_YEAR}
    to=${range_to:-$LAST_YEAR}
    ((from <= to)) || die "--from must not be after --to"
    for ((year = from; year <= to; year++)); do
        requested_years+=("$year")
    done
elif ((${#requested_years[@]} == 0)); then
    for ((year = FIRST_YEAR; year <= LAST_YEAR; year++)); do
        requested_years+=("$year")
    done
fi
# Sorted, de-duplicated year list.
mapfile -t selected_years < <(printf '%s\n' "${requested_years[@]}" | sort -n -u)
readonly selected_years
if [[ "$MODE" == mask && $PNG -eq 1 ]]; then
    die "PNG previews require a crop mode; pass --no-png with --mode mask"
fi

# ------------------------------------------------------ executable and config
if [[ -n "${RUST_FITS_AUTO_CROPPING_BIN:-}" ]]; then
    PROGRAM="$RUST_FITS_AUTO_CROPPING_BIN"
elif command -v rust_fits_auto_cropping >/dev/null 2>&1; then
    PROGRAM="$(command -v rust_fits_auto_cropping)"
else
    PROGRAM="$REPOSITORY_ROOT/target/release/rust_fits_auto_cropping"
fi
readonly PROGRAM
if [[ ! -x "$PROGRAM" ]]; then
    printf 'Executable not found or not executable: %s\n' "$PROGRAM" >&2
    printf 'Build it with: cargo build --release --manifest-path %q/Cargo.toml\n' "$REPOSITORY_ROOT" >&2
    exit 2
fi
for required_file in "$CONFIG_PATH" "$HEURISTICS_PATH"; do
    [[ -f "$required_file" ]] || die "Required configuration file not found: $required_file"
done
HEURISTICS_PATH="$(readlink -f -- "$HEURISTICS_PATH")"
readonly HEURISTICS_PATH

# ---------------------------------------------------------- path safety guards
canonical_root() {
    # Absolute, existing-or-creatable root, resolved through symlinks.
    local label=$1 path=$2 resolved
    [[ "$path" == /* ]] || die "$label must be an absolute path: $path"
    resolved="$(readlink -m -- "$path")"
    [[ "$resolved" != / ]] || die "$label must not be the filesystem root"
    printf '%s\n' "$resolved"
}
SOURCE_ROOT="$(canonical_root RUST_FITS_AUTO_CROPPING_SOURCE_ROOT "$SOURCE_ROOT")"
OUTPUT_ROOT="$(canonical_root RUST_FITS_AUTO_CROPPING_OUTPUT_ROOT "$OUTPUT_ROOT")"
readonly SOURCE_ROOT OUTPUT_ROOT
if [[ "$SOURCE_ROOT" == "$OUTPUT_ROOT" || "$OUTPUT_ROOT/" == "$SOURCE_ROOT/"* || "$SOURCE_ROOT/" == "$OUTPUT_ROOT/"* ]]; then
    die "Source and output roots must be distinct and not nested: $SOURCE_ROOT vs $OUTPUT_ROOT"
fi
[[ -d "$SOURCE_ROOT" ]] || die "Source root not found: $SOURCE_ROOT"

# ------------------------------------------------- child processes and signals
active_program_pid=''
active_tail_pid=''
cleanup_children() {
    # Background jobs of a non-interactive shell ignore SIGINT, so Ctrl-C would
    # otherwise leave the crop process writing into a sanitized directory.
    local pid
    for pid in "$active_tail_pid" "$active_program_pid"; do
        [[ -n "$pid" ]] && kill -TERM "$pid" 2>/dev/null || true
    done
    for pid in "$active_tail_pid" "$active_program_pid"; do
        [[ -n "$pid" ]] && wait "$pid" 2>/dev/null || true
    done
    active_program_pid=''
    active_tail_pid=''
}
on_signal() {
    printf '\nInterrupted: stopping the running crop process. The current year output is incomplete (it was sanitized before processing).\n' >&2
    cleanup_children
    exit 130
}
trap on_signal INT TERM HUP
trap cleanup_children EXIT

# --------------------------------------------------------------- helpers
ensure_directory_link() {
    local source_dir=$1
    local output_link=$2
    local resolved_source resolved_link

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

# Same extension set and symlink policy as the program's discovery
# (case-insensitive, regular files only, symlinks not followed).
count_fits_inputs() {
    find "$1" -type f \
        \( -iname '*.fits.gz' -o -iname '*.fit.gz' -o -iname '*.fts.gz' \
        -o -iname '*.fits.fz' -o -iname '*.fit.fz' -o -iname '*.fts.fz' \
        -o -iname '*.fits' -o -iname '*.fit' -o -iname '*.fts' -o -iname '*.fz' \) \
        -printf '.' | wc -c | tr -d '[:space:]'
}

count_entries() {
    if [[ -d "$1" ]]; then
        find "$1" -mindepth 1 -maxdepth 1 -printf '.' | wc -c | tr -d '[:space:]'
    else
        printf '0'
    fi
}

format_duration() {
    local total=$1
    printf '%dh%02dm%02ds' $((total / 3600)) $((total % 3600 / 60)) $((total % 60))
}

render_progress() {
    local year=$1 completed=$2 total=$3 width=30
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

# Print one top-level integer field of a pretty-printed summary.json.
summary_field() {
    sed -n "s/^  \"$2\": \([0-9][0-9]*\),\{0,1\}$/\1/p" "$1" | head -n 1
}

report_summary() {
    local year=$1 output_dir=$2 discovered=$3 elapsed=$4 summary="$2/summary.json"
    if [[ ! -f "$summary" ]]; then
        printf 'Year %s: no summary.json was written (%s).\n' "$year" "$(format_duration "$elapsed")" >&2
        return 1
    fi
    local total output clipped kept ignored errors
    total=$(summary_field "$summary" total)
    output=$(summary_field "$summary" output_images)
    clipped=$(summary_field "$summary" clipped)
    kept=$(summary_field "$summary" kept)
    ignored=$(summary_field "$summary" ignored)
    errors=$(summary_field "$summary" errors)
    printf 'Year %s: %s inputs, %s output images; %s clipped, %s kept, %s ignored, %s input errors; %s\n' \
        "$year" "${total:-?}" "${output:-?}" "${clipped:-?}" "${kept:-?}" "${ignored:-?}" "${errors:-?}" \
        "$(format_duration "$elapsed")"
    if [[ -n "$total" && "$total" != "$discovered" ]]; then
        printf 'Year %s: warning: the program discovered %s inputs but the script counted %s.\n' \
            "$year" "$total" "$discovered" >&2
    fi
    if [[ -n "$total" && -n "$output" && "$total" != "$output" ]]; then
        printf 'Year %s: warning: output parity failed (%s inputs, %s output images).\n' \
            "$year" "$total" "$output" >&2
        return 1
    fi
}

run_year_with_progress() {
    local year=$1 input_dir=$2 output_dir=$3 total=$4
    local completed=0 last_percent=-1 status_file activity_file candidate line status
    local -a png_flag=()
    local -A prior_logs=()
    ((PNG == 1)) && png_flag=(--png)

    shopt -s nullglob
    if [[ -d "$output_dir" ]]; then
        for candidate in "$output_dir"/activity_*.jsonl; do
            prior_logs["$candidate"]=1
        done
    fi
    shopt -u nullglob

    if ((total > 0)); then
        render_progress "$year" 0 "$total"
        last_percent=0
    else
        printf 'Year %s: no matching FITS files found.\n' "$year" >&2
    fi

    status_file=$(mktemp "${TMPDIR:-/tmp}/cropped-year-status.XXXXXX")
    (
        set +e
        trap - INT HUP EXIT
        "$PROGRAM" \
            --input-dir "$input_dir" \
            --output-dir "$output_dir" \
            --recursive \
            --mode "$MODE" \
            --flat-output \
            "${png_flag[@]}" \
            --sanitize-output \
            --config "$CONFIG_PATH" \
            --heuristics "$HEURISTICS_PATH" &
        child=$!
        # Forward termination to the crop process itself; signalling only
        # this wrapper would orphan it.
        trap 'kill -TERM "$child" 2>/dev/null; wait "$child"; exit 143' TERM
        wait "$child"
        printf '%s\n' "$?" >"$status_file"
    ) &
    active_program_pid=$!

    # Wait for this run's activity log (a log not present before the run).
    activity_file=''
    while [[ -z "$activity_file" && ! -s "$status_file" ]]; do
        shopt -s nullglob
        for candidate in "$output_dir"/activity_*.jsonl; do
            if [[ -z ${prior_logs[$candidate]+known} ]]; then
                activity_file=$candidate
                break
            fi
        done
        shopt -u nullglob
        [[ -n "$activity_file" || -s "$status_file" ]] && break
        sleep 0.1
    done

    if [[ -n "$activity_file" ]] && ((total > 0)); then
        exec {activity_fd}< <(exec tail -n +1 -F -- "$activity_file" 2>/dev/null)
        active_tail_pid=$!
        # Live progress until the program has finished.
        while [[ ! -s "$status_file" ]]; do
            if IFS= read -r -t 0.2 line <&"$activity_fd"; then
                if [[ "$line" == *'"event":"file_finished"'* ]]; then
                    ((completed < total)) && completed=$((completed + 1))
                    local percent=$((completed * 100 / total))
                    if ((percent != last_percent)); then
                        render_progress "$year" "$completed" "$total"
                        last_percent=$percent
                    fi
                fi
            fi
        done
        kill "$active_tail_pid" 2>/dev/null || true
        wait "$active_tail_pid" 2>/dev/null || true
        active_tail_pid=''
        exec {activity_fd}<&-
    fi
    wait "$active_program_pid" 2>/dev/null || true
    active_program_pid=''
    # A short run can finish before its log was found or fully streamed:
    # locate the log once more and render the final count from the file.
    if [[ -z "$activity_file" ]]; then
        shopt -s nullglob
        for candidate in "$output_dir"/activity_*.jsonl; do
            if [[ -z ${prior_logs[$candidate]+known} ]]; then
                activity_file=$candidate
                break
            fi
        done
        shopt -u nullglob
    fi
    if [[ -n "$activity_file" ]] && ((total > 0)); then
        completed=$(grep -c '"event":"file_finished"' -- "$activity_file" || true)
        ((completed > total)) && completed=$total
        if ((completed * 100 / total != last_percent)); then
            render_progress "$year" "$completed" "$total"
        fi
        [[ -t 2 ]] && printf '\n' >&2
    fi

    IFS= read -r status <"$status_file" || status=1
    rm -f -- "$status_file"
    return "$status"
}

year_status=()
process_year() {
    local year=$1
    local source_year="$SOURCE_ROOT/$year"
    local output_year="$OUTPUT_ROOT/$year"
    local input_dir="$source_year/science_calibrated"
    local output_dir="$output_year/science_calibrated"
    local calibration_dir total started status

    if [[ ! -d "$input_dir" ]]; then
        printf 'Skipping year %s: science input directory is missing: %s\n' "$year" "$input_dir" >&2
        year_status+=("$year missing-input")
        return 1
    fi
    if [[ -L "$output_year" || -L "$output_dir" ]]; then
        printf 'Skipping year %s: output path is a symlink (refusing to sanitize through it): %s\n' \
            "$year" "$output_dir" >&2
        year_status+=("$year unsafe-output")
        return 1
    fi
    total=$(count_fits_inputs "$input_dir")

    if ((PLAN_ONLY == 1)); then
        printf 'Plan year %s: %s FITS inputs in %s -> %s (would delete %s existing entries); links:' \
            "$year" "$total" "$input_dir" "$output_dir" "$(count_entries "$output_dir")"
        for calibration_dir in bias dark flat; do
            if [[ -d "$source_year/$calibration_dir" ]]; then
                printf ' %s' "$calibration_dir"
            else
                printf ' %s(missing)' "$calibration_dir"
            fi
        done
        printf '\n'
        year_status+=("$year planned")
        return 0
    fi

    mkdir -p -- "$output_year"
    for calibration_dir in bias dark flat; do
        if [[ ! -d "$source_year/$calibration_dir" ]]; then
            printf 'Year %s has no source %s/ directory; continuing without that calibration link.\n' \
                "$year" "$calibration_dir" >&2
            continue
        fi
        if ! ensure_directory_link "$source_year/$calibration_dir" "$output_year/$calibration_dir"; then
            printf 'Skipping science processing for year %s because a calibration link conflicts with its source.\n' \
                "$year" >&2
            year_status+=("$year link-conflict")
            return 1
        fi
    done

    printf 'Processing year %s (%s mode, %s FITS inputs): %s -> %s\n' \
        "$year" "$MODE" "$total" "$input_dir" "$output_dir"
    started=$SECONDS
    status=0
    run_year_with_progress "$year" "$input_dir" "$output_dir" "$total" || status=$?
    local elapsed=$((SECONDS - started))
    local summary_ok=0
    report_summary "$year" "$output_dir" "$total" "$elapsed" || summary_ok=1
    case "$status" in
    0)
        if ((summary_ok == 0)); then
            year_status+=("$year complete")
            return 0
        fi
        year_status+=("$year incomplete")
        ;;
    1) year_status+=("$year complete-with-input-errors") ;;
    *) year_status+=("$year failed(exit $status)") ;;
    esac
    return 1
}

# ------------------------------------------------------------------- main
if ((PLAN_ONLY == 0)); then
    mkdir -p -- "$OUTPUT_ROOT"
    # One batch per output root: concurrent runs would sanitize each other.
    exec {lock_fd}>"$OUTPUT_ROOT/.process_cropped_years.lock"
    if ! flock -n "$lock_fd"; then
        die "Another process_cropped_years run holds $OUTPUT_ROOT/.process_cropped_years.lock"
    fi
fi

printf 'Years: %s\nSource: %s\nOutput: %s\nExecutable: %s (%s)\nRules: %s\n' \
    "${selected_years[*]}" "$SOURCE_ROOT" "$OUTPUT_ROOT" "$PROGRAM" \
    "$("$PROGRAM" --version 2>/dev/null || printf 'version unknown')" "$HEURISTICS_PATH"

batch_started=$SECONDS
failures=0
for year in "${selected_years[@]}"; do
    if ! process_year "$year"; then
        failures=$((failures + 1))
    fi
done

printf 'Batch finished in %s:\n' "$(format_duration $((SECONDS - batch_started)))"
printf '  %s\n' "${year_status[@]}"
if ((failures > 0)); then
    printf '%s year(s) did not complete cleanly; see the messages, summaries and activity logs above.\n' "$failures" >&2
    exit 1
fi
