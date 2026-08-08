#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
jts_version=1.20.0
jts_sha256=6a783d8f9dba3d3cf7265435f134402f63c05838aa6cbcc4297ad3a5b2842baf
jts_root=$repo_root/target/reference/jts
jts_jar=$jts_root/jts-core-$jts_version.jar
build_root=$jts_root/classes
source_file=$repo_root/benchmarks/reference/jts/KnipsaJtsAdapter.java

mkdir -p "$jts_root"
actual_sha256=
if [ -f "$jts_jar" ]; then
  actual_sha256=$(shasum -a 256 "$jts_jar" | awk '{print $1}')
fi
if [ "$actual_sha256" != "$jts_sha256" ]; then
  temporary_jar=$jts_jar.download.$$
  trap 'rm -f "$temporary_jar"' EXIT HUP INT TERM
  curl --fail --location --silent --show-error \
    "https://repo.maven.apache.org/maven2/org/locationtech/jts/jts-core/$jts_version/jts-core-$jts_version.jar" \
    --output "$temporary_jar"
  downloaded_sha256=$(shasum -a 256 "$temporary_jar" | awk '{print $1}')
  if [ "$downloaded_sha256" != "$jts_sha256" ]; then
    echo "JTS checksum mismatch: expected $jts_sha256, got $downloaded_sha256" >&2
    exit 1
  fi
  mv "$temporary_jar" "$jts_jar"
  trap - EXIT HUP INT TERM
fi

mkdir -p "$build_root"
javac --release 17 -encoding UTF-8 -Xlint:all -Werror \
  -classpath "$jts_jar" -d "$build_root" "$source_file"

printf '%s:%s\n' "$build_root" "$jts_jar"
