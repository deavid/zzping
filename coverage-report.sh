#!/bin/bash

# Script to generate coverage report and list untested functions
# Usage: ./coverage-report.sh

echo "Generating coverage report..."

# Create temp directory for coverage output
TEMP_DIR=$(mktemp -d)
echo "Using temp directory: $TEMP_DIR"

# Generate text coverage report
echo "Running tests and generating coverage report..."
cargo llvm-cov --text --output-dir "$TEMP_DIR" > /dev/null 2>&1 || exit 2

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
    # Get untested lines for this file
    UNTESTED_LINES=$(grep -E "(^\s*[0-9]+\|\s*0\|)|(^[^|]*\|[^|]*$)" "$COVERAGE_FILE" | grep -v "Created:" | grep -v "Coverage Report")

    # Only process if there are untested lines
    if [ -n "$UNTESTED_LINES" ]; then
        # Extract relative path from zzping project root
        FNAME="$(echo "$COVERAGE_FILE" | sed 's|.*/zzping/||' | sed 's|\.txt$||')"
        echo "File: $FNAME"
        echo "$UNTESTED_LINES"
        echo ""
    fi
done

cat "$TEMP_DIR/text/index.txt"

# Cleanup
rm -r "$TEMP_DIR"

echo "Done!"
