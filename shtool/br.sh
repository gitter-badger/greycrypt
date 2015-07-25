#!/bin/bash

uname=$(uname)

# these guys need help killing the process on CTRL-C...and the taskkill command varies slightly
if [[ $uname == *"CYGWIN"* ]] 
then
	trap "ps -W | grep -i grey_crypt | grep -i debug | awk '{print \$1}' | xargs -i taskkill /f /pid {}" EXIT
fi
if [[ $uname == *"MSYS"* ]]
then
	trap "ps -W | grep -i grey_crypt | grep -i debug | awk '{print \$1}' | xargs -i taskkill -f -pid {}" EXIT
fi

clear
export RUST_BACKTRACE=1 
cargo run --verbose 

