#!/usr/bin/env bash
# Build all DiPECS academic reports.
# Can be run from any directory.

set -euo pipefail

# Switch to the academic reports directory.
cd "$(dirname "$0")/../docs/academic-src"

for tex in \
  01_Survey_Report/main.tex \
  02_Feasibility_Report/main.tex \
  03_Midterm_Report/main.tex \
  04_Final_Report/main.tex
do
  echo "=== Building $tex ==="
  # -cd switches to the source directory so each main.pdf lands in its own
  # report directory and relative paths (../refs, ../icon.png) resolve.
  latexmk -cd -xelatex -halt-on-error -interaction=nonstopmode "$tex"
done

echo "=== All reports built ==="
