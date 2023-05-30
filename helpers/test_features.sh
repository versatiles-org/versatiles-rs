function test()  {
   echo "cargo test -F $1"
   cmd="cargo test --workspace --no-default-features"
   [ -n "$1" ] && cmd="$cmd -F $1"
   result=$(eval "$cmd 2>&1")
   if [ $? -ne 0 ]; then
      echo "$result"
      echo "ERROR DURING: cargo test -F $1"
      exit 1
   fi
}

test
test "default"
test "mbtiles"

exit 0
