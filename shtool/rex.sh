#!/bin/bash

if [ "$1" == "" ]; then
  echo "Enter an error number to explain"
  exit 1
else
  rustc --explain $1 | less
fi
