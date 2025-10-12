#!/bin/bash

# Script to generate coverage report and list untested functions
# Usage: ./coverage-report.sh [path_filter]
# If path_filter is provided, only show results for files containing that path

echo "Generating coverage report..."

# Get optional path filter argument
PATH_FILTER="$1"
if [ -n "$PATH_FILTER" ]; then
    echo "Filtering results to paths containing: $PATH_FILTER"
fi

# Create temp directory for coverage output
TEMP_DIR=$(mktemp -d)
echo "Using temp directory: $TEMP_DIR"

# Generate text coverage report (always run full suite)
echo "Running tests and generating coverage report..."
cargo llvm-cov --text --output-dir "$TEMP_DIR" >"$TEMP_DIR/llvm_cov.log" 2>&1
if [ $? -ne 0 ]; then
    echo "Failed: cargo llvm-cov command failed"
    cat "$TEMP_DIR/llvm_cov.log"
    rm -r "$TEMP_DIR"
    exit 2
fi

# Find all coverage report files
COVERAGE_FILES=$(find "$TEMP_DIR" -name "*.txt")

if [ -z "$COVERAGE_FILES" ]; then
    echo "Error: Could not find any coverage report files"
    rm -r "$TEMP_DIR"
    exit 1
fi

# Extract untested functions/lines from all files
echo ""
echo "=== UNTESTED CODE LINES ==="

# Process each coverage file
for COVERAGE_FILE in $COVERAGE_FILES; do
    # Extract relative path from zzping project root
    FNAME="$(echo "$COVERAGE_FILE" | sed 's|.*/zzping/||' | sed 's|\.txt$||')"

    # Apply path filter if provided
    if [ -n "$PATH_FILTER" ] && [[ "$FNAME" != *"$PATH_FILTER"* ]]; then
        continue
    fi

    # Get untested lines for this file
    UNTESTED_LINES=$(grep -E "(^\s*[0-9]+\|\s*0\|)|(^[^|]*\|[^|]*$)" "$COVERAGE_FILE" | grep -v "Created:" | grep -v "Coverage Report")

    # Only process if there are untested lines
    if [ -n "$UNTESTED_LINES" ]; then
        echo "File: $FNAME"
        echo "$UNTESTED_LINES"
        echo ""
    fi
done

cat "$TEMP_DIR/text/index.txt"

# Cleanup
rm -r "$TEMP_DIR"

echo "Done!"
