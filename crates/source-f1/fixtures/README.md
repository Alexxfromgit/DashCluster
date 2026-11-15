# F1 25 fixture excerpt

`f1-2025.ctlog` has four format-2025 UDP packets from a single time-trial session. The packets carry
the player's session history, lap data, car status and telemetry near a completed `1:49.505` lap.
Only the fields needed by the decoder are preserved. Player and session identifiers and the capture
timestamps are synthetic. Other bytes, including other cars' rows, are zero-filled.
