#!/bin/bash
# filepath: /Users/andrew/Documents/PROGRAM_LOCAL/iron/deploy.sh
set -e
set -o pipefail

echo "🚀 Starting deployment process..."

# Get the latest tag and increment patch version
git fetch --quiet --tags --prune 2>/dev/null || true
LATEST_TAG=$(git tag -l 'v*' | sort -V | tail -1)
[ -z "$LATEST_TAG" ] && LATEST_TAG="v0.0.0"
echo "📋 Latest tag: $LATEST_TAG"

# Extract version numbers
VERSION=${LATEST_TAG#v}
IFS='.' read -ra VERSION_PARTS <<< "$VERSION"
MAJOR=${VERSION_PARTS[0]:-0}
MINOR=${VERSION_PARTS[1]:-0}
PATCH=${VERSION_PARTS[2]:-0}

# Increment patch version
NEW_PATCH=$((PATCH + 1))
NEW_TAG="v${MAJOR}.${MINOR}.${NEW_PATCH}"

# Ensure uniqueness against both local and remote (handles concurrent runs)
while git rev-parse "$NEW_TAG" >/dev/null 2>&1 || git ls-remote --tags origin | grep -q "refs/tags/$NEW_TAG$"; do
  NEW_PATCH=$((NEW_PATCH + 1))
  NEW_TAG="v${MAJOR}.${MINOR}.${NEW_PATCH}"
done

echo "🏷️  Creating new tag: $NEW_TAG"

# Commit any pending changes
if ! git diff-index --quiet HEAD --; then
    echo "📝 Committing pending changes..."
    git add .
    git commit -m "Release $NEW_TAG"
fi

# Create and push the new tag
git tag -m "Release $NEW_TAG" "$NEW_TAG"

# Retry push if a race occurs (very unlikely after loop, but safe)
if ! git push origin "$NEW_TAG"; then
  echo "⚠️ Tag push failed (race). Recomputing..."
  git fetch --quiet --tags
  while git ls-remote --tags origin | grep -q "refs/tags/$NEW_TAG$"; do
    NEW_PATCH=$((NEW_PATCH + 1))
    NEW_TAG="v${MAJOR}.${MINOR}.${NEW_PATCH}"
  done
  git tag -m "Release $NEW_TAG" "$NEW_TAG"
  git push origin "$NEW_TAG"
fi

git push origin master

echo "✅ Deployment triggered! Check GitHub Actions for build progress."
echo "🔗 https://github.com/$(git config --get remote.origin.url | sed 's/.*github.com[:/]\([^.]*\).*/\1/')/actions"