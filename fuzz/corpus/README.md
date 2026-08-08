# Deterministic fuzz corpus

These small raw-byte seeds are replayed by `scripts/fuzz-replay.sh`. The
integer and floating-point boolean seeds encode overlapping rectangles and
crossing rings in the byte layouts consumed by their targets. The geometry
seed exercises full-width integer decoding.

The replay command uses `-runs=1`: libFuzzer initializes and executes every
checked-in corpus entry but does not start an open-ended mutation campaign.
Run longer fuzz campaigns separately and minimize any failure before adding it
here as a permanent regression seed.
