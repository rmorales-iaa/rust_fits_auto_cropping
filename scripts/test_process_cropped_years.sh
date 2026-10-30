#!/usr/bin/env bash
# Regression tests for scripts/process_cropped_years.sh using a mock executable.
# Run: bash scripts/test_process_cropped_years.sh
set -uo pipefail

SCRIPT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)/process_cropped_years.sh"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/test-cropped-years.XXXXXX")"
trap 'rm -rf -- "$WORK"' EXIT
passed=0
failed=0

check() {
    local name=$1
    shift
    if "$@"; then
        passed=$((passed + 1))
        printf 'ok   %s\n' "$name"
    else
        failed=$((failed + 1))
        printf 'FAIL %s\n' "$name"
    fi
}

# Mock: records argv, emulates --sanitize-output, writes one file_finished
# event per FITS input, a pretty summary.json, then exits with MOCK_EXIT.
MOCK="$WORK/mock_program"
cat >"$MOCK" <<'MOCK'
#!/usr/bin/env bash
if [[ "${1:-}" == --version ]]; then echo "mock 9.9.9"; exit 0; fi
printf '%s\n' "$*" >>"$MOCK_LOG"
[[ -n "${MOCK_PIDFILE:-}" ]] && echo $$ >"$MOCK_PIDFILE"
while (($#)); do
    case "$1" in
    --input-dir) in=$2; shift 2 ;;
    --output-dir) out=$2; shift 2 ;;
    *) shift ;;
    esac
done
mkdir -p "$out"
find "$out" -mindepth 1 -maxdepth 1 ! -name 'activity_*' -exec rm -rf {} +
log="$out/activity_$(date +%s%N).jsonl"
n=0
while IFS= read -r f; do
    n=$((n + 1))
    printf '{"event":"file_finished","input":"%s"}\n' "$f" >>"$log"
done < <(find "$in" -type f -iname '*.fits')
sleep "${MOCK_SLEEP:-0}"
printf '{\n  "schema_version": 12,\n  "total": %d,\n  "output_images": %d,\n  "clipped": 1,\n  "proposed": 0,\n  "kept": %d,\n  "ignored": 0,\n  "errors": 0,\n  "files": []\n}\n' \
    "$n" "${MOCK_OUTPUT_IMAGES:-$n}" "$((n - 1))" >"$out/summary.json"
exit "${MOCK_EXIT:-0}"
MOCK
chmod +x "$MOCK"

make_tree() {
    local root=$1
    for year in 2002 2003; do
        mkdir -p "$root/src/$year/science_calibrated/night1" "$root/src/$year/bias" "$root/src/$year/flat"
        : >"$root/src/$year/science_calibrated/night1/a.fits"
        : >"$root/src/$year/science_calibrated/night1/b.FITS"
        : >"$root/src/$year/science_calibrated/notes.txt"
    done
}

run() {
    # run ROOT [args...]; captures stdout+stderr in $ROOT/out.txt, returns status
    local root=$1
    shift
    RUST_FITS_AUTO_CROPPING_BIN="$MOCK" \
        RUST_FITS_AUTO_CROPPING_SOURCE_ROOT="$root/src" \
        RUST_FITS_AUTO_CROPPING_OUTPUT_ROOT="$root/out" \
        MOCK_LOG="$root/mock.log" \
        bash "$SCRIPT" "$@" >"$root/out.txt" 2>&1
}

status_is() { [[ "$1" == "$2" ]]; }
contains() { grep -q -- "$2" "$1"; }

# --- help and argument validation
T="$WORK/args"; make_tree "$T"
run "$T" --help; check "help exits 0" status_is $? 0
run "$T" --year 1999; check "out-of-range year is a usage error" status_is $? 2
run "$T" --from 2002 --to 2003; check "year ranges are refused" status_is $? 2
check "range refusal explains the sequence" contains "$T/out.txt" "list the years instead"
run "$T" --years 2002,20x3; check "malformed year in a list is a usage error" status_is $? 2
run "$T" 2002 1999; check "out-of-range positional year is a usage error" status_is $? 2
run "$T" --mode mask; check "mask mode with PNG is rejected" status_is $? 2
run "$T" --mode bogus; check "unknown mode is rejected" status_is $? 2
run "$T" --unknown; check "unknown option is rejected" status_is $? 2

# --- path safety guards
T="$WORK/guards"; make_tree "$T"
RUST_FITS_AUTO_CROPPING_BIN="$MOCK" RUST_FITS_AUTO_CROPPING_SOURCE_ROOT="$T/src" \
    RUST_FITS_AUTO_CROPPING_OUTPUT_ROOT="relative/out" bash "$SCRIPT" --year 2002 >"$T/o" 2>&1
check "relative output root is refused" status_is $? 2
RUST_FITS_AUTO_CROPPING_BIN="$MOCK" RUST_FITS_AUTO_CROPPING_SOURCE_ROOT="$T/src" \
    RUST_FITS_AUTO_CROPPING_OUTPUT_ROOT="$T/src/2002" bash "$SCRIPT" --year 2002 >"$T/o" 2>&1
check "output nested in source is refused" status_is $? 2
RUST_FITS_AUTO_CROPPING_BIN="$MOCK" RUST_FITS_AUTO_CROPPING_SOURCE_ROOT="$T/src" \
    RUST_FITS_AUTO_CROPPING_OUTPUT_ROOT="/" bash "$SCRIPT" --year 2002 >"$T/o" 2>&1
check "filesystem root output is refused" status_is $? 2
mkdir -p "$T/elsewhere"
mkdir -p "$T/out"
ln -s "$T/elsewhere" "$T/out/2002"
run "$T" --year 2002
check "symlinked output year is skipped" status_is $? 1
check "symlink skip is reported" contains "$T/out.txt" "unsafe-output"
check "nothing written through the symlink" test ! -e "$T/elsewhere/science_calibrated"

# --- plan mode writes nothing
T="$WORK/plan"; make_tree "$T"
run "$T" --plan --years 2003,2002
check "plan exits 0" status_is $? 0
check "plan creates no output root" test ! -e "$T/out"
check "plan never invokes the program" test ! -e "$T/mock.log"
check "plan counts FITS case-insensitively" contains "$T/out.txt" "Plan year 2002: 2 FITS inputs"
check "plan reports a missing dark link" contains "$T/out.txt" "dark(missing)"

# --- normal run, selection, links, summary, arguments
T="$WORK/run"; make_tree "$T"
run "$T" --years 2003,2002,2003 --mode crop_masked
check "clean run exits 0" status_is $? 0
check "years run once each" test "$(grep -c -- '--input-dir' "$T/mock.log")" = 2
check "years run in the order given (2003 first)" bash -c "head -n 1 '$T/mock.log' | grep -q '/2003/science_calibrated'"
check "mode is passed through" grep -q -- '--mode crop_masked' "$T/mock.log"
check "sanitize, flat output and png are passed" grep -q -- '--flat-output --png --sanitize-output' "$T/mock.log"
check "bias link created" test -L "$T/out/2002/bias"
check "missing dark source is not linked" test ! -e "$T/out/2002/dark"
check "summary line printed" contains "$T/out.txt" "Year 2002: 2 inputs, 2 output images"
check "progress reaches 100%" contains "$T/out.txt" "100% (2/2)"
check "batch status lists complete years" contains "$T/out.txt" "2003 complete"
: >"$T/mock.log"
run "$T" 2002 "2003 2002" --years "2003, 2002"
check "positional and space-separated years exit 0" status_is $? 0
check "positional years run once each" test "$(grep -c -- '--input-dir' "$T/mock.log")" = 2
check "positional order is kept (2002 first)" bash -c "head -n 1 '$T/mock.log' | grep -q '/2002/science_calibrated'"
run "$T" --year 2002 --no-png
check "rerun with --no-png exits 0" status_is $? 0
check "--no-png omits --png" bash -c "tail -n 1 '$T/mock.log' | grep -vq -- '--png'"

# --- exit-status classification
T="$WORK/status"; make_tree "$T"
MOCK_EXIT=1 run "$T" --year 2002
check "input errors make the batch exit 1" status_is $? 1
check "input errors are classified" contains "$T/out.txt" "complete-with-input-errors"
MOCK_EXIT=2 run "$T" --year 2002
check "program failure exits 1" status_is $? 1
check "program failure is classified" contains "$T/out.txt" "failed(exit 2)"
MOCK_OUTPUT_IMAGES=1 run "$T" --year 2002
check "output parity failure exits 1" status_is $? 1
check "parity failure is reported" contains "$T/out.txt" "output parity failed"

# --- one batch per output root
T="$WORK/lock"; make_tree "$T"; mkdir -p "$T/out"
exec {held}>"$T/out/.process_cropped_years.lock"
flock -n "$held"
run "$T" --year 2002
check "concurrent run is refused" status_is $? 2
exec {held}>&-

# --- TERM and Ctrl-C (INT under job control, as from a terminal) stop the crop process
for signal in TERM INT; do
    T="$WORK/signal-$signal"; make_tree "$T"
    set -m
    RUST_FITS_AUTO_CROPPING_BIN="$MOCK" RUST_FITS_AUTO_CROPPING_SOURCE_ROOT="$T/src" \
        RUST_FITS_AUTO_CROPPING_OUTPUT_ROOT="$T/out" MOCK_LOG="$T/mock.log" \
        MOCK_SLEEP=30 MOCK_PIDFILE="$T/mock.pid" bash "$SCRIPT" --year 2002 >"$T/out.txt" 2>&1 &
    script_pid=$!
    set +m
    for _ in $(seq 100); do [[ -s "$T/mock.pid" ]] && break; sleep 0.1; done
    sleep 0.3
    kill "-$signal" "$script_pid"
    wait "$script_pid"
    check "$signal: interrupted batch exits 130" status_is $? 130
    sleep 0.3
    check "$signal: crop process is not left running" bash -c "! kill -0 \$(cat '$T/mock.pid') 2>/dev/null"
    check "$signal: interruption is explained" contains "$T/out.txt" "Interrupted"
done

printf '\n%d passed, %d failed\n' "$passed" "$failed"
((failed == 0))
