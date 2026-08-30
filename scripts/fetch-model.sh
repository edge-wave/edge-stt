#!/bin/sh
# Fetch a Whisper model into a directory this repository will not track.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
record=$root/THIRD-PARTY-LICENSES
origin=https://huggingface.co/ggerganov/whisper.cpp/resolve/main

usage() {
    cat <<'USAGE'
usage: fetch-model.sh [--dir DIR] <model>...
       fetch-model.sh --list

A model is named as it is published: base-q5_1, small.en-q5_1,
large-v3-turbo-q5_0. The ggml- prefix and the .bin suffix are optional.

Without --dir the file goes to $EDGE_STT_MODEL_DIR, or to models/ under
this repository, which git ignores.

docs/models.md says which one belongs on a device and which on a server.
USAGE
}

# The checksums live in the licence record, so the file that says where a
# model came from is the same file that decides what it must be.
known() {
    awk -F'|' '$2 ~ /ggml-.*\.bin/ {
        gsub(/[ \t`]/, "", $2); gsub(/[ \t`]/, "", $4); print $2 " " $4
    }' "$record"
}

checksum_of() {
    known | awk -v want="$1" '$1 == want { print $2 }'
}

hash_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fSL --progress-bar -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q --show-progress -O "$2" "$1"
    else
        echo "need curl or wget" >&2
        exit 1
    fi
}

tracked_here() {
    git -C "$1" rev-parse --is-inside-work-tree >/dev/null 2>&1 || return 1
    git -C "$1" check-ignore -q "$2" && return 1
    return 0
}

dir=${EDGE_STT_MODEL_DIR:-$root/models}
wanted=

while [ $# -gt 0 ]; do
    case $1 in
        --list) known | cut -d' ' -f1 | sed 's/^ggml-//; s/\.bin$//'; exit 0 ;;
        --dir) shift; [ $# -gt 0 ] || { usage >&2; exit 2; }; dir=$1 ;;
        -h|--help) usage; exit 0 ;;
        -*) usage >&2; exit 2 ;;
        *) wanted="$wanted $1" ;;
    esac
    shift
done

[ -n "$wanted" ] || { usage >&2; exit 2; }

mkdir -p "$dir"
dir=$(CDPATH= cd -- "$dir" && pwd)

for name in $wanted; do
    file=$name
    case $file in ggml-*) ;; *) file=ggml-$file ;; esac
    case $file in *.bin) ;; *) file=$file.bin ;; esac

    want=$(checksum_of "$file")
    if [ -z "$want" ]; then
        echo "$name: no checksum for it in THIRD-PARTY-LICENSES." >&2
        echo "Add the file, its origin, and its SHA-256 there first." >&2
        exit 1
    fi

    # A model somewhere git would track is one commit from being
    # published. Refuse rather than rely on remembering.
    if tracked_here "$dir" "$dir/$file"; then
        echo "$dir is a git work tree and would track $file." >&2
        echo "Choose a directory outside it, or add an ignore rule." >&2
        exit 1
    fi

    at=$dir/$file
    if [ -f "$at" ] && [ "$(hash_file "$at")" = "$want" ]; then
        echo "$file is already here, and is the published file."
        continue
    fi

    echo "fetching $file"
    if ! download "$origin/$file" "$at.part"; then
        rm -f "$at.part"
        echo "$file did not download." >&2
        exit 1
    fi
    got=$(hash_file "$at.part")
    if [ "$got" != "$want" ]; then
        rm -f "$at.part"
        echo "$file hashed $got, not the $want written down. Not kept." >&2
        exit 1
    fi
    mv "$at.part" "$at"
    echo "$at"
done
