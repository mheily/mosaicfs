#!/usr/bin/env bash
#
# Generates a structured inventory of the MosaicFS codebase.
# Output: Markdown suitable for injection into LLM context.
#
# Usage: bash .claude/skills/architect/inventory.sh
#
# Portable: uses only POSIX grep, sed, awk (no GNU -P flag).
#
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

echo "# Codebase Inventory"
echo ""
echo "_Auto-generated at $(date -u '+%Y-%m-%dT%H:%M:%SZ') from $(git rev-parse --short HEAD 2>/dev/null || echo 'unknown')_"
echo ""

# ── Workspace Crates ─────────────────────────────────────────────────────────

echo "## Workspace Crates"
echo ""
echo "| Crate | Type | Description |"
echo "|-------|------|-------------|"

# Parse workspace members from Cargo.toml
members=$(sed -n '/^members/,/]/p' Cargo.toml | grep '"' | sed 's/.*"\(.*\)".*/\1/')

for crate_dir in $members; do
    if [ ! -f "$crate_dir/Cargo.toml" ]; then
        continue
    fi
    name=$(grep '^name' "$crate_dir/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')
    if [ -f "$crate_dir/src/main.rs" ]; then
        crate_type="binary"
    elif [ -f "$crate_dir/src/lib.rs" ]; then
        crate_type="library"
    else
        crate_type="unknown"
    fi
    rs_count=$(find "$crate_dir/src" -name '*.rs' 2>/dev/null | wc -l | tr -d ' ')
    loc=$(find "$crate_dir/src" -name '*.rs' -exec cat {} + 2>/dev/null | wc -l | tr -d ' ')
    echo "| $name | $crate_type | ${rs_count} files, ${loc} lines |"
done
echo ""

# Excluded crates
excludes=$(sed -n '/^exclude/,/]/p' Cargo.toml | grep '"' | sed 's/.*"\(.*\)".*/\1/' | tr '\n' ', ' | sed 's/,$//' || true)
if [ -n "$excludes" ]; then
    echo "**Excluded from workspace:** $excludes"
    echo ""
fi

# ── Binaries Produced ────────────────────────────────────────────────────────

echo "## Binaries"
echo ""
for crate_dir in $members; do
    if [ -f "$crate_dir/src/main.rs" ]; then
        name=$(grep '^name' "$crate_dir/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')
        echo "- **$name** ($crate_dir/src/main.rs)"
    fi
done
echo ""

# ── Deployment Topology ──────────────────────────────────────────────────────

echo "## Deployment"
echo ""
if [ -f deploy/mosaicfs.yaml ]; then
    echo "**Manifest:** deploy/mosaicfs.yaml"
    echo ""
    echo "Containers in pod:"
    # Use awk to extract container name, image, and ports as a unit
    awk '
    /^  containers:/ { in_containers=1; next }
    /^  volumes:/    { in_containers=0 }
    in_containers && /^    - name:/ {
        if (cname != "") {
            printf "- **%s**: image=%s", cname, image
            if (ports != "") printf ", ports=%s", ports
            print ""
        }
        cname=$NF; image=""; ports=""
    }
    in_containers && /image:/ { image=$NF }
    in_containers && /containerPort:/ {
        if (ports != "") ports = ports "," $NF
        else ports = $NF
    }
    END {
        if (cname != "") {
            printf "- **%s**: image=%s", cname, image
            if (ports != "") printf ", ports=%s", ports
            print ""
        }
    }
    ' deploy/mosaicfs.yaml
    echo ""
fi
if [ -f Dockerfile.mosaicfs ]; then
    echo "**Container image:** Dockerfile.mosaicfs builds mosaicfs-server + mosaicfs-agent + web UI into localhost/mosaicfs:latest"
    echo ""
fi

# ── REST API Surface ─────────────────────────────────────────────────────────

echo "## REST API"
echo ""
find . -name 'routes.rs' -not -path '*/target/*' 2>/dev/null | sort | while read -r routes_file; do
    crate_dir=$(echo "$routes_file" | sed 's|/src/.*||' | sed 's|^\./||')
    route_count=$(grep -c '\.route(' "$routes_file" 2>/dev/null || echo 0)
    echo "**$crate_dir** ($routes_file): ~${route_count} routes"
    echo ""
    # Group by API prefix
    grep '\.route("' "$routes_file" 2>/dev/null | \
        sed 's/.*\.route("//; s/".*//' | \
        awk -F/ '{
            if (NF >= 3) print "/" $2 "/" $3;
            else if (NF >= 2) print "/" $2;
        }' | sort | uniq -c | sort -rn | while read -r count prefix; do
        echo "  - \`$prefix/*\`: $count endpoints"
    done
    echo ""
done

# Also check for embedded HTTP servers (not in routes.rs)
(find . -name '*.rs' -not -path '*/target/*' -not -name 'routes.rs' 2>/dev/null | \
    xargs grep -l 'Router::new()' 2>/dev/null || true) | while read -r file; do
    route_count=$(grep -c '\.route(' "$file" 2>/dev/null || echo 0)
    if [ "$route_count" -gt 0 ]; then
        crate_dir=$(echo "$file" | sed 's|/src/.*||' | sed 's|^\./||')
        echo "**$crate_dir** ($file): ${route_count} routes (embedded server)"
        echo ""
    fi
done

# ── Frontend / UI ────────────────────────────────────────────────────────────

echo "## Frontend"
echo ""
if [ -d web/src ]; then
    page_count=$(find web/src -name '*Page.tsx' -o -name '*page.tsx' 2>/dev/null | wc -l | tr -d ' ')
    component_count=$(find web/src/components -name '*.tsx' 2>/dev/null | wc -l | tr -d ' ')
    type_count=$(find web/src/types -name '*.ts' 2>/dev/null | wc -l | tr -d ' ')
    echo "**React/Tauri app** (web/src/): ${page_count} pages, ${component_count} components, ${type_count} generated types"
    echo ""
    if [ "$page_count" -gt 0 ]; then
        echo "Pages:"
        find web/src -name '*Page.tsx' -o -name '*page.tsx' 2>/dev/null | sort | while read -r f; do
            basename "$f" .tsx | sed 's/Page$//' | sed 's/^/- /'
        done
        echo ""
    fi
fi
if [ -d web/src-tauri ]; then
    echo "**Tauri shell** (web/src-tauri/): native desktop wrapper"
    echo ""
fi

# ── Cross-Crate Duplication ──────────────────────────────────────────────────

echo "## Cross-Crate Code Duplication"
echo ""
# Find .rs filenames that appear in multiple crates (excluding main/lib/mod)
find . -path '*/src/*.rs' -not -path '*/target/*' \
    -not -name 'main.rs' -not -name 'lib.rs' -not -name 'mod.rs' 2>/dev/null | \
    while read -r f; do basename "$f"; done | \
    sort | uniq -c | sort -rn | while read -r count name; do
    if [ "$count" -gt 1 ]; then
        locations=$(find . -path "*/src/*$name" -not -path '*/target/*' 2>/dev/null | \
            sed 's|^\./||' | tr '\n' ', ' | sed 's/,$//')
        echo "- **$name** (${count}x): $locations"
    fi
done
echo ""

# ── Key Workspace Dependencies ───────────────────────────────────────────────

echo "## Key Workspace Dependencies"
echo ""
# Show dependencies from [workspace.dependencies] that indicate architectural choices
# (skip ubiquitous utility crates)
(sed -n '/\[workspace.dependencies\]/,/^\[/p' Cargo.toml | \
    grep -E '^[a-z]' | \
    grep -v -E '^(serde|serde_json|chrono|uuid|tokio|tokio-util|tracing|tracing-subscriber|rand|anyhow|thiserror|bytes|hex|sha2|hmac|async-trait|mime_guess|urlencoding) ' || true) | \
    while IFS= read -r line; do
        dep=$(echo "$line" | sed 's/ *=.*//')
        version=$(echo "$line" | sed -n 's/.*version *= *"\([^"]*\)".*/\1/p')
        echo "- **$dep** $version"
    done
echo ""

# ── External Services ────────────────────────────────────────────────────────

echo "## External Services"
echo ""
if grep -rq 'CouchDB\|couchdb\|COUCHDB' --include='*.rs' --include='*.toml' --include='*.yaml' . 2>/dev/null; then
    echo "- **CouchDB**: primary data store (federation, metadata)"
fi
if grep -rq 'struct.*S3\|s3_\|S3Client\|S3Backend' --include='*.rs' . 2>/dev/null; then
    echo "- **S3-compatible storage**: replication backend"
fi
echo ""
