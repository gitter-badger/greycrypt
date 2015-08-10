#!/bin/sh

TEST=$1
ITERS=$2

if [ "$TEST" == "" ]; then
   echo "First argument is test name"
   exit 1
fi

if [ "$ITERS" == "" ]; then
   echo "Second argument is number of iterations"
   exit 1
fi

FAILURES=0
for ((n=0;n < ${ITERS};n++)); do
   outfile=out_$n
   cargo test $TEST -- --nocapture >$outfile 2>&1
   if [ $? -ne 0 ]; then
      echo "Failed on iteration $n"
      FAILURES=$((FAILURES+1))
   else
      #X=0
      rm -f $outfile
   fi
done

echo "$ITERS run; $FAILURES failures"