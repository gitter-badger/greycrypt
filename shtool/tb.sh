#!/bin/bash
set -e

DIR=$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )
PROJECT_DIR="$DIR/.."

files=$(find src/ -type f -iname "*.rs")
linecount=$(cat $files | wc -l | tr -d '[[:space:]]')
touch $PROJECT_DIR/src/main.rs
mkdir -p $PROJECT_DIR/compiletimes
set +e
{ time cargo build --verbose >/dev/null; } 2> $PROJECT_DIR/compiletimes/temp
if [ $? -eq 0 ]; then
  rev=$(git rev-parse HEAD)
  echo "gitrev: $rev" >> $PROJECT_DIR/compiletimes/temp
  mv $PROJECT_DIR/compiletimes/temp $PROJECT_DIR/compiletimes/${linecount}_lines
  cat $PROJECT_DIR/compiletimes/${linecount}_lines
else
  echo "Build error"
fi
