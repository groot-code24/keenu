# Support

Keenu is an alpha-stage volunteer project with no paid support.

## Before asking

1. Check [what works where](docs/platform-support.md): what you want may not be supported on your system yet.
2. Look at the [issues](https://github.com/groot-code24/keenu/issues), including closed ones.
3. Run `keenu --capabilities`, which says what works on your computer and why not where it does not.

## Reporting a bug

[Open an issue](https://github.com/groot-code24/keenu/issues/new/choose) with:

- what you did, what you expected and what happened;
- your system and Keenu's version, with `keenu --diagnostics` (it holds no secrets, but read it first);
- on Linux, `keenud --report`, and on Windows or macOS the log file `keenu.log` in the data folder. For more detail,
  start Keenu with the environment variable `KEENU_LOG=debug`.

Security problems go privately instead: see [SECURITY.md](SECURITY.md).
