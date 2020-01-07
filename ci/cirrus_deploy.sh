#!/usr/bin/env bash
set -e

if [[ "$CIRRUS_TAG" == "" ]]; then
  echo "Not a tag. No need to deploy!"
  exit 0
fi

if [[ "$GITHUB_TOKEN" == "" ]]; then
  echo "Please provide GitHub access token via GITHUB_TOKEN environment variable!"
  exit 1
fi

# Convert the tag to a Github ID
release_id=$(curl -sH "$GITHUB_TOKEN" https://api.github.com/repos/$CIRRUS_REPO_FULL_NAME/releases/tags/$CIRRUS_TAG | grep -m 1 "id.:" | grep -w id | tr -cd '[0-9]')
if [[ "$release_id" == "" ]]; then
  ech "Unable to get release ID from tag $CIRRUS_TAG"
  exit 1
fi

file_content_type="application/octet-stream"
files_to_upload=(
  # relative paths of assets to upload
  $CIRRUS_WORKING_DIR/artifacts/*
)

for fpath in $files_to_upload
do
  echo "Uploading $fpath..."
  name=$(basename "$fpath")
  url_to_upload="https://uploads.github.com/repos/$CIRRUS_REPO_FULL_NAME/releases/$release_id/assets?name=$name"
  curl -X POST \
    --data-binary @$fpath \
    --header "Authorization: token $GITHUB_TOKEN" \
    --header "Content-Type: $file_content_type" \
    $url_to_upload
done
