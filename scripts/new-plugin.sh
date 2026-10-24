#!/usr/bin/env bash
# Scaffold a FetchBox extension (plugin) from a template in plugins/templates/, so work can start
# from a manifest and scripts that already validate. See plugins/README.md for the full guide.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
templates="$root/plugins/templates"

usage() {
  cat <<EOF
Usage: scripts/new-plugin.sh <id> [options]

Creates the folder <id>/ with a plugin.json, a README.md and, depending on the template, scripts.

Options:
  -t, --template NAME   hook (default), resolver or process; see --list
  -n, --name TEXT       display name (default: the id in words)
  -a, --author TEXT     author (default: git user.name, else "Your Name")
  -o, --out DIR         folder to create <id>/ in (default: the current directory)
      --dev             create it in ./.dev/data/plugins, where the dev backend looks:
                        cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads
  -l, --list            list the templates
  -h, --help            this text

The id is 2 to 32 lowercase letters, digits and "-", starting with a letter.
EOF
}

list_templates() {
  local dir name desc
  for dir in "$templates"/*/; do
    name="$(basename "$dir")"
    desc="$(sed -n 's/^  "description": "\(.*\)",$/\1/p' "$dir/plugin.json" | head -n 1)"
    printf '  %-9s %s\n' "$name" "$desc"
  done
}

die() {
  echo "new-plugin: $*" >&2
  exit 1
}

template=hook
name=""
author=""
out=""
id=""

while [ $# -gt 0 ]; do
  case "$1" in
    -t|--template) [ $# -ge 2 ] || die "$1 needs a value"; template="$2"; shift 2 ;;
    -n|--name) [ $# -ge 2 ] || die "$1 needs a value"; name="$2"; shift 2 ;;
    -a|--author) [ $# -ge 2 ] || die "$1 needs a value"; author="$2"; shift 2 ;;
    -o|--out) [ $# -ge 2 ] || die "$1 needs a value"; out="$2"; shift 2 ;;
    --dev) out="./.dev/data/plugins"; shift ;;
    -l|--list) list_templates; exit 0 ;;
    -h|--help) usage; exit 0 ;;
    -*) usage >&2; die "unknown option $1" ;;
    *)
      [ -z "$id" ] || die "only one id is expected (got \"$id\" and \"$1\")"
      id="$1"; shift ;;
  esac
done

[ -n "$id" ] || { usage >&2; exit 2; }
[[ "$id" =~ ^[a-z][a-z0-9-]{1,31}$ ]] || die "\"$id\" is not a valid id (2 to 32 lowercase letters, digits and \"-\", starting with a letter)"
[ -d "$templates/$template" ] || die "no template \"$template\"; available:"$'\n'"$(list_templates)"

if [ -z "$name" ]; then
  name="$(echo "$id" | tr '-' ' ' | awk '{ for (i = 1; i <= NF; i++) $i = toupper(substr($i, 1, 1)) substr($i, 2) } 1')"
fi
if [ -z "$author" ]; then
  author="$(git config user.name 2>/dev/null || true)"
  author="${author:-Your Name}"
fi

# The values end up inside JSON strings, so refuse what would need escaping.
for value in "$name" "$author"; do
  case "$value" in
    *\"*|*\\*) die "the name and the author cannot contain \" or \\" ;;
  esac
  [[ "$value" != *[[:cntrl:]]* ]] || die "the name and the author cannot contain control characters"
done

dest="${out:-.}/$id"
[ ! -e "$dest" ] || die "$dest already exists"

mkdir -p "$dest"
cp -R "$templates/$template/." "$dest/"

# Plain text replacement in bash, not sed: the values may contain "&", "/" or "|".
shopt -u patsub_replacement 2>/dev/null || true
while IFS= read -r -d '' file; do
  content="$(cat "$file"; printf x)"
  content="${content%x}"
  content="${content//__ID__/"$id"}"
  content="${content//__NAME__/"$name"}"
  content="${content//__AUTHOR__/"$author"}"
  printf '%s' "$content" > "$file"
done < <(find "$dest" -type f -print0)

echo "Created $dest (template: $template)"
echo
echo "Files:"
(cd "$dest" && find . -type f | sed 's|^\./|  |' | sort)
echo
echo "Next:"
if [ -n "$out" ] && [ "$out" = "./.dev/data/plugins" ]; then
  echo "  1. cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads"
else
  echo "  1. Copy the folder into <data dir>/plugins/ (or: cp -r $dest ./.dev/data/plugins/)"
fi
echo "  2. Open Extensions, press Rescan, review the permissions, approve and switch it on"
echo "  3. Read $dest/README.md, then plugins/README.md for everything the manifest can do"
