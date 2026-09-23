#!/usr/bin/env bash
set -euo pipefail

if (( $# < 5 || ($# - 2) % 3 != 0 )); then
  echo "usage: package-skill.sh OUTPUT SKILL_DIR OS ARCH BINARY [OS ARCH BINARY ...]" >&2
  exit 2
fi

output=$1
skill_dir=$2
shift 2
test -f "$skill_dir/SKILL.md"
test -f "$skill_dir/skill.toml"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/skill-bom-cli/assets"
cp "$skill_dir/SKILL.md" "$skill_dir/skill.toml" "$stage/skill-bom-cli/"

while (( $# > 0 )); do
  os=$1
  arch=$2
  binary=$3
  shift 3
  case "$os/$arch" in
    Linux/X64|Linux/ARM64|macOS/X64|macOS/ARM64|Windows/X64|Windows/ARM64) ;;
    *) echo "unsupported release platform: $os/$arch" >&2; exit 2 ;;
  esac
  test -s "$binary"
  destination="$stage/skill-bom-cli/assets/$os/$arch"
  mkdir -p "$destination"
  if [[ "$os" == Windows ]]; then
    cp "$binary" "$destination/skill-bom.exe"
  else
    cp "$binary" "$destination/skill-bom"
    chmod 755 "$destination/skill-bom"
  fi
done

mkdir -p "$(dirname "$output")"
tar --sort=name --mtime="@${SOURCE_DATE_EPOCH:-0}" --owner=0 --group=0 \
  --numeric-owner -C "$stage" -cf - skill-bom-cli | gzip -n > "$output"
tar -tzf "$output" | grep -Fx 'skill-bom-cli/SKILL.md' >/dev/null
tar -tzf "$output" | grep -Fx 'skill-bom-cli/skill.toml' >/dev/null
