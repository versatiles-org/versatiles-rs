#!/usr/bin/env bash

function test()  {
   bin=$1
   features=$2
   cmd="cargo test"
   if [[ $bin =~ "bin" ]]; then
      cmd="$cmd --bin versatiles"
   else
      cmd="$cmd --lib"
   fi
   cmd="$cmd --no-default-features"
   if [[ -n "$features" ]]; then
      cmd="$cmd --features $features"
   fi
   cmd="$cmd 2>&1 | grep -vE 'info:' | sed 's/^/   /'"

   echo -e "\033[1;30mrun: $bin $features\033[0m"
   result=$(eval "$cmd 2>&1")
   if [ $? -ne 0 ]; then
      echo "$result"
      echo "ERROR DURING: cargo test --features $1"
      exit 1
   fi
}

test bin "default"
test bin "cli"
test bin "cli,image"
test bin "cli,mbtiles"
test bin "cli,request"
test bin "cli,server"
test bin "cli,tar"
test lib

exit 0
