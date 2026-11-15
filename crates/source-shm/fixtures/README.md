# Le Mans Ultimate fixture excerpt

`lmu-race.lmucap` contains seven samples of the native Windows x64 player rows from a race, including
neutral through fifth gear and the rollover into lap 2. Only the numeric fields needed by the decoder
are retained. Slot IDs and timestamps are synthetic, and unused fields are zero-filled.

The binary starts with `LMURAW01` and the telemetry and scoring row sizes as two little-endian `u32`s.
Each sample contains a little-endian `u64` timestamp in microseconds, a one-byte scoring-presence flag,
then the telemetry and scoring rows. The decoder test reads these rows directly on any host. Live
attachment to LMU's shared-memory mapping uses the Windows source.
