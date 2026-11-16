# Logging vectors

One set, read by every writer: `components.txt`, `line.re`, `secrets.txt` and the cases under `redact/*.json`.
A case is `{ name, level, component, message, fields: [{ type, key, value }], env: { home, host, user, salt }, expected }`;
`expected` is the whole line without its timestamp.

Readers: the Python writer (`tests/test_logging_writer.py`) and the Rust crate (`ragon-rs/log/tests/logging_vectors.rs`)
run every case byte for byte; `tests/test_log_vectors.py`
and `ragon-rs/src/log_vectors.rs` check the shape of every file and every case against `line.re`.

## Open: cases the writers do not yet agree with

An earlier draft of these cases pinned choices that the Python writer does not make, so they are left out until the
spec settles them: a value of the wrong type written `[invalid]` (the writer writes `[number]`, `[id]`, `[version]`);
a `path` outside Ragon's folders written `[path]`; a lone field over 4 KiB dropped whole; a credential after a header
name written as one `[secret]` including trailing text; a bare `ratio` kept as a number; and `size_of` fields.
