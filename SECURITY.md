# Security policy

DashCluster listens on your local network: UDP for game telemetry (port 5300 by default) and, when
"Share telemetry" is on, an HTTP/WebSocket server for phones and other computers (port 5380). It
also reads files that may come from other people: dashboard files, `.cluster` packages and `.ctlog`
recordings.

The hub has no authentication: anyone on the same network can open the dashboard and see your
telemetry. Turn sharing off in Settings on networks you don't trust.

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through
[GitHub's private vulnerability reporting](https://github.com/Alexxfromgit/DashCluster/security/advisories/new)
(Security tab → *Report a vulnerability*). Include the version, OS, steps to reproduce and the
impact you see.

You can expect an acknowledgement within a few days. Once a fix is released, the advisory is
published with credit to the reporter unless you prefer otherwise.

## Supported versions

Only the latest release receives fixes.

## Scope

Of particular interest:

- Crashes or unbounded resource use from network input: game packets (`crates/source-forza`), the
  hub's HTTP/WebSocket server (`crates/cluster-net`), and hub clients (`crates/source-net`,
  `apps/cluster-web`).
- The hub serving anything other than the web display, or being reachable in ways the settings
  don't suggest.
- Malformed dashboard files, `.cluster` packages (e.g. paths escaping the dashboards folder when
  importing) and `.ctlog` recordings.
