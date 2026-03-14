build:
  cargo build --release

run:
  ./target/debug/aprsfeed-rs -v -I ax25.local -u W3POG -H noam.aprs2.net -f ./aprsfeed.log

test:
  cargo test

release-patch: test
  cargo release --no-publish --no-verify patch --execute
release-minor: test
  cargo release --no-publish --no-verify minor --execute
release-major: test
  cargo release --no-publish --no-verify minor --execute

