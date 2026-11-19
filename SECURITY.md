# Security

## Reporting a vulnerability

Please report security problems privately through GitHub's
[private vulnerability reporting](https://github.com/udvlp/UmlCraft/security/advisories/new),
not in a public issue. Include the UmlCraft version, the platform and, if possible, a file or
steps that show the problem.

You get an answer within two weeks. Once a fix is released, the advisory is published with it.

## Supported versions

Only the latest release gets fixes.

## Scope

UmlCraft reads input from others, and each way in is in scope:

- **Files:** its own `.ucraft` projects, Astah's `.asta` files, structograms as `.nsd` files,
  SQL dumps (MySQL, MariaDB, PostgreSQL) and source code for the import. A file that crashes
  UmlCraft, makes it hang or use unbounded memory, or makes it write outside the chosen target
  is in scope.
- **The MCP server** (`umlcraft-cli mcp`): AI clients call its tools, and a call is untrusted
  input like a file. A call that crashes UmlCraft or the server, makes it hang or use unbounded
  memory, or reads or writes a file outside the folders the user allowed (the Documents folder,
  or those named with `--allow-path`) is in scope. The tools can do whatever the user can do to
  the open project: edit it, save it and export images and SQL into the allowed folders.
- **The live channel** between `umlcraft-cli` and the running app: the app listens on
  127.0.0.1 only, on a free port, and writes the port and a random token into a file only the
  user can read; a connection without the token is closed. Reaching the app from another
  computer or from another user's account, or without the token, is in scope. Programs of the
  same user can read the token, as they can read the user's files.
- **The update:** the check asks api.github.com for the latest release; the download fetches
  the package and `SHA256SUMS.txt` from that GitHub release over HTTPS and installs the package
  only if its checksum matches. Getting UmlCraft to install anything else is in scope. The
  checksum file comes from the same release, so it detects damage, not a release published by
  someone who took over the repository. The app can check a minisign signature of the
  checksum file, made by the release workflow with a key kept outside the repository; this
  is switched on once the signing key exists (`UPDATE_KEY` in `update.rs`). The packages
  themselves are not code-signed.

The Astah export runs Astah's own Java runtime; problems in Astah itself belong to its vendor.
