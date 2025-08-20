#!/bin/bash
# filepath: /Users/andrew/Documents/PROGRAM_LOCAL/iron/deploy.sh
set -e

echo "🚀 Starting deployment process..."

# Get the latest tag and increment patch version
LATEST_TAG="v0.0.18" # $(git describe --tags --abbrev=0 2>/dev/null || echo "v0.0.0")
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

echo "🏷️  Creating new tag: $NEW_TAG"

# Commit any pending changes
if ! git diff-index --quiet HEAD --; then
    echo "📝 Committing pending changes..."
    git add .
    git commit -m "Release $NEW_TAG"
fi

# Create and push the new tag
git tag "$NEW_TAG"
git push origin "$NEW_TAG"
git push origin master

echo "✅ Deployment triggered! Check GitHub Actions for build progress."
echo "🔗 https://github.com/$(git config --get remote.origin.url | sed 's/.*github.com[:/]\([^.]*\).*/\1/')/actions"