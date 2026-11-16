#!/bin/sh
# Original author: EvanBo. Apache-2.0.
set -eu
[ "$#" -eq 1 ] || { echo 'usage: install-skill.sh /absolute/project-root' >&2; exit 2; }
case "$1" in /*) ;; *) echo 'absolute project root required' >&2; exit 2;; esac
[ -d "$1" ] || { echo 'existing project directory required' >&2; exit 2; }
dest="$1/.agents/skills/development-map"
[ ! -e "$dest" ] && [ ! -L "$dest" ] || { echo 'skill destination already exists; review it before updating' >&2; exit 1; }
source_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/skills/development-map"
mkdir -p "$1/.agents/skills"
cp -R "$source_dir" "$dest"
printf 'Installed EvanBo Development Map Skill: %s\n' "$dest"
